//! OpenAI-Compatible Provider
//!
//! Supports any API that implements the OpenAI Chat Completions format,
//! including: OpenAI, DeepSeek, Groq, Together, Ollama, vLLM, etc.
//!
//! Feature-gated behind `http`.

use async_trait::async_trait;
use futures::{stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, warn};

use crate::agent::message::{Content, ContentPart, Message, Role};
use crate::agent::provider::{ChatRequest, Provider};
use crate::agent::streaming::{
    FinishReason, ProviderTelemetry, StreamingChoice, StreamingResponse, Usage,
};
use crate::error::{Error, Result};
use benshu_provider_core::ContinuationTelemetry;

/// Configuration for an OpenAI-compatible provider.
#[derive(Debug, Clone)]
pub struct OpenAiCompatConfig {
    /// API base URL (e.g. "https://api.openai.com/v1")
    pub base_url: String,
    /// API key (can be empty for local models e.g. Ollama)
    pub api_key: String,
    /// Default model name (can be overridden per request)
    pub default_model: String,
    /// Provider display name (for logging)
    pub name: String,
    /// Request timeout
    pub timeout: Duration,
    /// Maximum retry attempts on transient errors (429, 500, 502, 503)
    pub max_retries: u32,
    /// Base delay for exponential backoff
    pub retry_base_delay: Duration,
    /// Optional organization ID (OpenAI specific)
    pub organization: Option<String>,
}

impl Default for OpenAiCompatConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            default_model: "benshu-unconfigured-model".to_string(),
            name: "openai".to_string(),
            timeout: Duration::from_secs(120),
            max_retries: 3,
            retry_base_delay: Duration::from_millis(500),
            organization: None,
        }
    }
}

impl OpenAiCompatConfig {
    /// Create from environment variables.
    /// Reads: OPENAI_API_KEY, OPENAI_BASE_URL, OPENAI_MODEL, OPENAI_ORG_ID
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            base_url: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            default_model: std::env::var("OPENAI_MODEL")
                .unwrap_or_else(|_| "benshu-unconfigured-model".to_string()),
            organization: std::env::var("OPENAI_ORG_ID").ok(),
            ..Default::default()
        }
    }

    /// Create a DeepSeek configuration.
    pub fn deepseek() -> Self {
        Self {
            base_url: "https://api.deepseek.com/v1".to_string(),
            api_key: std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
            default_model: "deepseek-chat".to_string(),
            name: "deepseek".to_string(),
            ..Default::default()
        }
    }

    /// Create an Ollama (local) configuration.
    pub fn ollama() -> Self {
        Self {
            base_url: std::env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".to_string()),
            api_key: String::new(), // Ollama doesn't need a key
            default_model: std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3".to_string()),
            name: "ollama".to_string(),
            timeout: Duration::from_secs(300), // Local models can be slow
            max_retries: 1,
            ..Default::default()
        }
    }
}

/// An OpenAI-compatible LLM provider with retry, timeout, and error categorization.
pub struct OpenAiCompatProvider {
    config: OpenAiCompatConfig,
    client: Client,
}

impl OpenAiCompatProvider {
    const LOCAL_PSEUDO_TOOL_CALL_OPEN: &'static str = "<|tool_call>";
    const LOCAL_PSEUDO_TOOL_CALL_CLOSE: &'static str = "<tool_call|>";

    fn tool_replay_blocks_from_metadata(metadata: &HashMap<String, String>) -> HashMap<String, String> {
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
            matches!(part, ContentPart::ToolCall { id, .. } if replay_blocks.contains_key(id))
        })
    }

    fn messages_use_sampled_tool_replay(messages: &[Message]) -> bool {
        messages.iter().any(Self::message_uses_sampled_tool_replay)
    }

    fn summarize_request_messages(messages: &[OaiMessage]) -> String {
        messages
            .iter()
            .enumerate()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|(idx, message)| {
                let content_preview = message
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(|text| benshu_compression::preview_text(text, 120))
                    .unwrap_or_else(|| "<none>".to_string());
                let tool_call_count = message.tool_calls.as_ref().map(|calls| calls.len()).unwrap_or(0);
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

    fn extract_pseudo_tool_calls(content: &str) -> Vec<OaiResponseToolCall> {
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
                calls.push(OaiResponseToolCall {
                    id: format!("pseudo-tool-call-{ordinal}"),
                    r#type: "function".to_string(),
                    function: OaiResponseFunction {
                        name,
                        arguments: arguments.to_string(),
                    },
                });
            }

            remaining = &after_open[end_idx + Self::LOCAL_PSEUDO_TOOL_CALL_CLOSE.len()..];
        }

        calls
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

    /// Create a new provider from config.
    pub fn new(config: OpenAiCompatConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .pool_max_idle_per_host(4)
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }

    /// Create from environment variables.
    pub fn from_env() -> Result<Self> {
        Self::new(OpenAiCompatConfig::from_env())
    }

    /// Convert BenShu messages to OpenAI format.
    fn convert_messages(messages: &[Message]) -> Vec<OaiMessage> {
        messages
            .iter()
            .map(|msg| {
                let replay_blocks = Self::tool_replay_blocks_from_metadata(&msg.metadata);
                let role = match msg.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };

                // Handle tool results specially
                if msg.role == Role::Tool {
                    if let Content::Parts(parts) = &msg.content {
                        for part in parts {
                            if let ContentPart::ToolResult {
                                tool_call_id,
                                content,
                                ..
                            } = part
                            {
                                return OaiMessage {
                                    role: "tool".to_string(),
                                    content: Some(content.clone()),
                                    tool_call_id: Some(tool_call_id.clone()),
                                    tool_calls: None,
                                    name: None,
                                };
                            }
                        }
                    }
                }

                // Preserve assistant tool-call history using the native OpenAI
                // tool_calls envelope instead of flattening it into assistant
                // text. Some local OpenAI-compatible runtimes interpret a
                // flattened assistant tool call as response prefill, which
                // conflicts with thinking-enabled generation.
                if msg.role == Role::Assistant {
                    if let Content::Parts(parts) = &msg.content {
                        let mut text_parts = Vec::new();
                        let mut tool_calls = Vec::new();

                        for part in parts {
                            match part {
                                ContentPart::Text { text } => {
                                    if !text.trim().is_empty() {
                                        text_parts.push(text.clone());
                                    }
                                }
                                ContentPart::ToolCall {
                                    id,
                                    name,
                                    arguments,
                                } => {
                                    if let Some(block) = replay_blocks.get(id) {
                                        if !text_parts.iter().any(|text| text.contains(block)) {
                                            text_parts.push(block.clone());
                                        }
                                    }
                                    tool_calls.push(OaiResponseToolCall {
                                        id: id.clone(),
                                        r#type: "function".to_string(),
                                        function: OaiResponseFunction {
                                            name: name.clone(),
                                            arguments: arguments.to_string(),
                                        },
                                    });
                                }
                                _ => {}
                            }
                        }

                        if !tool_calls.is_empty() {
                            let content = if text_parts.is_empty() {
                                None
                            } else {
                                Some(text_parts.join("\n"))
                            };

                            return OaiMessage {
                                role: "assistant".to_string(),
                                content,
                                tool_call_id: None,
                                tool_calls: Some(tool_calls),
                                name: msg.name.clone(),
                            };
                        }
                    }
                }

                OaiMessage {
                    role: role.to_string(),
                    content: Some(msg.text()),
                    tool_call_id: None,
                    tool_calls: None,
                    name: msg.name.clone(),
                }
            })
            .collect()
    }

    fn normalize_system_messages_for_chat_template(messages: Vec<OaiMessage>) -> Vec<OaiMessage> {
        let mut system_parts = Vec::new();
        let mut non_system = Vec::new();

        for message in messages {
            if message.role == "system" {
                if let Some(content) = message.content {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        system_parts.push(trimmed.to_string());
                    }
                }
            } else {
                non_system.push(message);
            }
        }

        if system_parts.is_empty() {
            return non_system;
        }

        let mut normalized = Vec::with_capacity(non_system.len() + 1);
        normalized.push(OaiMessage {
            role: "system".to_string(),
            content: Some(system_parts.join("\n\n")),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
        normalized.extend(non_system);
        normalized
    }

    /// Convert BenShu tool definitions to OpenAI format.
    fn convert_tools(tools: &[crate::skills::tool::ToolDefinition]) -> Vec<OaiTool> {
        tools
            .iter()
            .map(|t| OaiTool {
                r#type: "function".to_string(),
                function: OaiFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    }

    /// Execute a request with exponential backoff retry.
    async fn execute_with_retry(&self, request: &OaiChatRequest) -> Result<OaiChatResponse> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                let delay = self.config.retry_base_delay * 2u32.pow(attempt - 1);
                debug!(
                    provider = %self.config.name,
                    attempt = attempt,
                    delay_ms = delay.as_millis(),
                    "Retrying after transient error"
                );
                tokio::time::sleep(delay).await;
            }

            let mut req = self
                .client
                .post(&url)
                .header("Content-Type", "application/json");

            if !self.config.api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", self.config.api_key));
            }

            if let Some(ref org) = self.config.organization {
                req = req.header("OpenAI-Organization", org);
            }

            let response = match req.json(request).send().await {
                Ok(r) => r,
                Err(e) => {
                    if e.is_timeout() {
                        warn!(
                            provider = %self.config.name,
                            attempt = attempt,
                            "Request timed out"
                        );
                        last_error = Some(Error::ProviderApi(format!("Timeout: {}", e)));
                        continue;
                    }
                    if e.is_connect() {
                        last_error = Some(Error::ProviderApi(format!(
                            "Connection error (is {} reachable?): {}",
                            self.config.base_url, e
                        )));
                        continue;
                    }
                    return Err(Error::ProviderApi(format!("Request failed: {}", e)));
                }
            };

            let status = response.status();

            // Categorize HTTP status codes
            match status.as_u16() {
                200..=299 => {
                    let body_bytes = response.bytes().await.map_err(|e| {
                        Error::ProviderApi(format!("Failed to read response body: {}", e))
                    })?;
                    let body = String::from_utf8_lossy(&body_bytes).into_owned();

                    let parsed: OaiChatResponse = serde_json::from_str(&body).map_err(|e| {
                        Error::ProviderApi(format!(
                            "Failed to parse response (first 500 chars): {}: {}",
                            &body[..body.len().min(500)],
                            e
                        ))
                    })?;
                    return Ok(parsed);
                }
                401 => {
                    return Err(Error::ProviderAuth(format!(
                        "Invalid API key for provider '{}'",
                        self.config.name
                    )));
                }
                429 => {
                    let retry_after = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(5);

                    warn!(
                        provider = %self.config.name,
                        retry_after_secs = retry_after,
                        "Rate limited"
                    );

                    if attempt < self.config.max_retries {
                        tokio::time::sleep(Duration::from_secs(retry_after)).await;
                        last_error = Some(Error::ProviderRateLimit {
                            retry_after_secs: retry_after,
                        });
                        continue;
                    }
                    return Err(Error::ProviderRateLimit {
                        retry_after_secs: retry_after,
                    });
                }
                500 | 502 | 503 => {
                    let body = response.text().await.unwrap_or_default();
                    warn!(
                        provider = %self.config.name,
                        status = status.as_u16(),
                        "Server error, retrying"
                    );
                    last_error = Some(Error::ProviderApi(format!(
                        "Server error {}: {}",
                        status.as_u16(),
                        &body[..body.len().min(200)]
                    )));
                    continue;
                }
                _ => {
                    let body = response.text().await.unwrap_or_default();
                    if status.as_u16() == 400
                        && body.contains("Assistant response prefill is incompatible with enable_thinking")
                    {
                        warn!(
                            provider = %self.config.name,
                            request_messages = %Self::summarize_request_messages(&request.messages),
                            "OpenAI-compatible request hit assistant-prefill incompatibility"
                        );
                    }
                    return Err(Error::ProviderApi(format!(
                        "HTTP {}: {}",
                        status.as_u16(),
                        &body[..body.len().min(500)]
                    )));
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| Error::ProviderApi("All retry attempts exhausted".to_string())))
    }

    async fn execute_streaming_request(
        &self,
        request: &OaiChatRequest,
        started_at: std::time::Instant,
        tool_contract_mode: &'static str,
        mainline_stability: &'static str,
        continuation_requested: bool,
        tool_exact_replay_used: bool,
    ) -> Result<StreamingResponse> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");

        if !self.config.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.config.api_key));
        }

        if let Some(ref org) = self.config.organization {
            req = req.header("OpenAI-Organization", org);
        }

        let response = req
            .json(request)
            .send()
            .await
            .map_err(|e| Error::ProviderApi(format!("Streaming request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Error::ProviderApi(format!(
                "Streaming HTTP {}: {}",
                status.as_u16(),
                &body[..body.len().min(500)]
            )));
        }

        let provider_name = self.config.name.clone();
        let model = request.model.clone();
        let bytes_stream = response.bytes_stream();

        let stream = async_stream::stream! {
            let mut buffer = String::new();
            let mut finish_reason = FinishReason::Stop;
            let mut usage: Option<Usage> = None;
            let mut done_received = false;
            futures::pin_mut!(bytes_stream);

            while !done_received {
                let Some(chunk) = bytes_stream.next().await else {
                    break;
                };
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield Err(Error::ProviderApi(format!("Streaming chunk failed: {error}")));
                        return;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(newline_idx) = buffer.find('\n') {
                    let raw_line: String = buffer.drain(..=newline_idx).collect();
                    let line = raw_line.trim();
                    if line.is_empty() || line.starts_with("event:") {
                        continue;
                    }
                    let Some(payload) = line.strip_prefix("data:").map(str::trim) else {
                        continue;
                    };
                    if payload == "[DONE]" {
                        done_received = true;
                        break;
                    }

                    let chunk = match serde_json::from_str::<OaiStreamChunk>(payload) {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            warn!(
                                provider = %provider_name,
                                error = %error,
                                payload = %benshu_compression::preview_text(payload, 200),
                                "Failed to parse OpenAI-compatible streaming chunk"
                            );
                            continue;
                        }
                    };

                    if let Some(chunk_usage) = chunk.usage {
                        usage = Some(Usage {
                            prompt_tokens: chunk_usage.prompt_tokens,
                            completion_tokens: chunk_usage.completion_tokens,
                            total_tokens: chunk_usage.total_tokens,
                        });
                    }

                    for choice in chunk.choices {
                        if let Some(content) = choice.delta.content {
                            if !content.is_empty() {
                                yield Ok(StreamingChoice::Message(content));
                            }
                        }
                        if let Some(thought) = choice.delta.reasoning_content {
                            if !thought.is_empty() {
                                yield Ok(StreamingChoice::Thought(thought));
                            }
                        }
                        if let Some(provider_finish_reason) = choice.finish_reason {
                            finish_reason = FinishReason::from_provider_reason(&provider_finish_reason);
                        }
                    }
                }
            }

            yield Ok(StreamingChoice::Finish(finish_reason.clone()));
            if let Some(usage) = usage {
                yield Ok(StreamingChoice::Usage(usage));
            }

            let mut continuation = if continuation_requested || tool_exact_replay_used {
                Some(ContinuationTelemetry {
                    mode: if tool_exact_replay_used {
                        "tool_replay".to_string()
                    } else {
                        "requested".to_string()
                    },
                    cache_source: if tool_exact_replay_used {
                        "message_history".to_string()
                    } else {
                        "streaming_response".to_string()
                    },
                    ..Default::default()
                })
            } else {
                None
            };
            if tool_exact_replay_used {
                if let Some(telemetry) = continuation.as_mut() {
                    telemetry.tool_exact_replay_used = true;
                }
            }

            let mut extra = std::collections::HashMap::new();
            extra.insert("finish_reason".to_string(), finish_reason.as_str().to_string());
            extra.insert("streaming_mode".to_string(), "sse".to_string());
            extra.insert("tool_contract_mode".to_string(), tool_contract_mode.to_string());
            extra.insert("mainline_stability".to_string(), mainline_stability.to_string());
            yield Ok(StreamingChoice::Telemetry(ProviderTelemetry {
                provider_name: Some(provider_name),
                model: Some(model),
                latency_ms: Some(started_at.elapsed().as_millis() as u64),
                continuation,
                extra,
            }));
            yield Ok(StreamingChoice::Done);
        };

        Ok(StreamingResponse::from_stream(stream))
    }
}

#[async_trait]
impl Provider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn tool_contract_mode(&self) -> &'static str {
        "native_tool_calling"
    }

    fn mainline_stability(&self) -> &'static str {
        "stable"
    }

    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        let started_at = std::time::Instant::now();
        let model = if request.model.is_empty() {
            self.config.default_model.clone()
        } else {
            request.model.clone()
        };

        let oai_tools = if request.tools.is_empty() {
            None
        } else {
            Some(Self::convert_tools(&request.tools))
        };
        let tool_exact_replay_used = Self::messages_use_sampled_tool_replay(&request.messages);

        let mut messages = Vec::new();
        if let Some(ref sp) = request.system_prompt {
            messages.push(OaiMessage {
                role: "system".to_string(),
                content: Some(sp.clone()),
                tool_call_id: None,
                tool_calls: None,
                name: None,
            });
        }
        messages.extend(Self::convert_messages(&request.messages));
        let messages = Self::normalize_system_messages_for_chat_template(messages);

        let mut oai_request = OaiChatRequest {
            model,
            messages,
            tools: oai_tools,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: false,
        };

        let continuation_requested = request.enable_cache_control;
        if oai_request.tools.is_none() {
            oai_request.stream = true;
            match self
                .execute_streaming_request(
                    &oai_request,
                    started_at,
                    self.tool_contract_mode(),
                    self.mainline_stability(),
                    continuation_requested,
                    tool_exact_replay_used,
                )
                .await
            {
                Ok(response) => return Ok(response),
                Err(error) => {
                    warn!(
                        provider = %self.config.name,
                        error = %error,
                        "OpenAI-compatible streaming failed; falling back to non-stream completion"
                    );
                    oai_request.stream = false;
                }
            }
        }

        let response = self.execute_with_retry(&oai_request).await?;
        let choices = Self::convert_response_to_choices(
            response,
            self.config.name.clone(),
            oai_request.model.clone(),
            started_at.elapsed().as_millis() as u64,
            self.tool_contract_mode(),
            self.mainline_stability(),
            continuation_requested,
            tool_exact_replay_used,
        );

        Ok(StreamingResponse::from_stream(stream::iter(choices)))
    }
}

impl OpenAiCompatProvider {
    fn convert_response_to_choices(
        response: OaiChatResponse,
        provider_name: String,
        model: String,
        latency_ms: u64,
        tool_contract_mode: &str,
        mainline_stability: &str,
        continuation_requested: bool,
        tool_exact_replay_used: bool,
    ) -> Vec<Result<StreamingChoice>> {
        let mut choices = Vec::new();
        let mut tool_call_count = 0usize;
        let mut finish_reason = FinishReason::Stop;

        if let Some(choice) = response.choices.first() {
            let native_tool_calls = choice.message.tool_calls.clone().unwrap_or_default();
            let pseudo_tool_calls = if native_tool_calls.is_empty() {
                choice
                    .message
                    .content
                    .as_deref()
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

            if effective_tool_calls.is_empty() {
                if let Some(ref content) = choice.message.content {
                    if !content.is_empty() {
                        choices.push(Ok(StreamingChoice::Message(content.clone())));
                    }
                }
            } else {
                tool_call_count = effective_tool_calls.len();
                for tc in &effective_tool_calls {
                    let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                    choices.push(Ok(StreamingChoice::ToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: args,
                    }));
                }
            }

            finish_reason = if let Some(ref provider_finish_reason) = choice.finish_reason {
                FinishReason::from_provider_reason(provider_finish_reason)
            } else if tool_call_count > 0 {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            };
            choices.push(Ok(StreamingChoice::Finish(finish_reason.clone())));
        }

        let mut continuation =
            Self::continuation_telemetry_from_openai_response(&response, continuation_requested);
        if tool_exact_replay_used {
            let telemetry = continuation.get_or_insert_with(|| ContinuationTelemetry {
                mode: "tool_replay".to_string(),
                cache_source: "message_history".to_string(),
                ..Default::default()
            });
            telemetry.tool_exact_replay_used = true;
        }

        if let Some(usage) = response.usage {
            choices.push(Ok(StreamingChoice::Usage(Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            })));
        }

        let mut extra = std::collections::HashMap::new();
        extra.insert(
            "finish_reason".to_string(),
            finish_reason.as_str().to_string(),
        );
        extra.insert("tool_call_count".to_string(), tool_call_count.to_string());
        extra.insert(
            "tool_contract_mode".to_string(),
            tool_contract_mode.to_string(),
        );
        extra.insert(
            "mainline_stability".to_string(),
            mainline_stability.to_string(),
        );
        choices.push(Ok(StreamingChoice::Telemetry(ProviderTelemetry {
            provider_name: Some(provider_name),
            model: Some(model),
            latency_ms: Some(latency_ms),
            continuation,
            extra,
        })));

        choices.push(Ok(StreamingChoice::Done));
        choices
    }

    fn continuation_telemetry_from_openai_response(
        response: &OaiChatResponse,
        continuation_requested: bool,
    ) -> Option<ContinuationTelemetry> {
        let usage = response.usage.as_ref();
        let timings = response.timings.as_ref();

        if usage.is_none() && timings.is_none() {
            return None;
        }

        let prompt_tokens = usage
            .map(|usage| usage.prompt_tokens)
            .or_else(|| timings.and_then(|timings| u32::try_from(timings.prompt_n).ok()));
        let miss_reason = if continuation_requested {
            None
        } else {
            Some("cache_control_disabled".to_string())
        };

        Some(ContinuationTelemetry {
            mode: if continuation_requested {
                "openai_compatible_provider_reported".to_string()
            } else {
                "disabled".to_string()
            },
            cache_source: "provider_usage".to_string(),
            prompt_tokens,
            prefill_ms: timings.map(|timings| timings.prompt_ms.max(0.0) as u64),
            decode_ms: timings.map(|timings| timings.predicted_ms.max(0.0) as u64),
            miss_reason,
            ..Default::default()
        })
    }
}

// ─── OpenAI API Types ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct OaiChatRequest {
    model: String,
    messages: Vec<OaiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OaiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OaiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OaiResponseToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Debug, Serialize)]
struct OaiTool {
    r#type: String,
    function: OaiFunction,
}

#[derive(Debug, Serialize)]
struct OaiFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct OaiChatResponse {
    choices: Vec<OaiChoice>,
    usage: Option<OaiUsage>,
    #[serde(default)]
    timings: Option<OaiTimings>,
}

#[derive(Debug, Deserialize)]
struct OaiChoice {
    message: OaiResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OaiStreamChunk {
    #[serde(default)]
    choices: Vec<OaiStreamChoice>,
    usage: Option<OaiUsage>,
}

#[derive(Debug, Deserialize)]
struct OaiStreamChoice {
    #[serde(default)]
    delta: OaiStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OaiStreamDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OaiResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OaiResponseToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OaiResponseToolCall {
    id: String,
    r#type: String,
    function: OaiResponseFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OaiResponseFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OaiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct OaiTimings {
    #[serde(default)]
    prompt_n: u64,
    #[serde(default)]
    prompt_ms: f64,
    #[serde(default)]
    predicted_ms: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::streaming::StreamingChoice;

    #[test]
    fn test_config_defaults() {
        let config = OpenAiCompatConfig::default();
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_config_deepseek() {
        let config = OpenAiCompatConfig::deepseek();
        assert!(config.base_url.contains("deepseek"));
        assert_eq!(config.default_model, "deepseek-chat");
    }

    #[test]
    fn test_config_ollama() {
        let config = OpenAiCompatConfig::ollama();
        assert!(config.base_url.contains("localhost"));
        assert!(config.api_key.is_empty());
        assert_eq!(config.timeout, Duration::from_secs(300));
    }

    #[test]
    fn test_message_conversion() {
        let messages = vec![
            Message::system("You are helpful"),
            Message::user("Hello"),
            Message::assistant("Hi there!"),
        ];

        let converted = OpenAiCompatProvider::convert_messages(&messages);
        assert_eq!(converted.len(), 3);
        assert_eq!(converted[0].role, "system");
        assert_eq!(converted[1].role, "user");
        assert_eq!(converted[2].role, "assistant");
    }

    #[test]
    fn test_message_conversion_preserves_assistant_tool_calls_as_native_tool_calls() {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "tool_replay_receipts".to_string(),
            serde_json::json!({
                "call_123": {
                    "tool_call_id": "call_123",
                    "replay_mode": "sampled_text_exact",
                    "sampled_call_block": "<|tool_call>{\"name\":\"delegate\",\"arguments\":{\"role\":\"researcher\"}}<tool_call|>",
                    "sampled_call_fingerprint": "sampled",
                    "sampled_call_ref": "message://assistant/text/tool_call/call_123",
                    "normalized_call_fingerprint": "normalized"
                }
            })
            .to_string(),
        );
        let messages = vec![Message {
            role: Role::Assistant,
            content: Content::Parts(vec![ContentPart::ToolCall {
                id: "call_123".to_string(),
                name: "delegate".to_string(),
                arguments: serde_json::json!({
                    "role": "researcher",
                    "task": "search latest Lancet heart treatment papers"
                }),
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

        assert!(OpenAiCompatProvider::messages_use_sampled_tool_replay(
            &messages
        ));
        let converted = OpenAiCompatProvider::convert_messages(&messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "assistant");
        assert!(converted[0]
            .content
            .as_deref()
            .is_some_and(|content| content.contains("<|tool_call>")));
        let tool_calls = converted[0]
            .tool_calls
            .as_ref()
            .expect("assistant tool_calls should be preserved");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "delegate");
        assert!(tool_calls[0]
            .function
            .arguments
            .contains("\"researcher\""));
    }

    #[test]
    fn test_message_conversion_preserves_truth_verification_system_prompt() {
        let messages = vec![
            Message::system("### TRUTH AND VERIFICATION CONTRACT\nNever present unverified claims as confirmed facts."),
            Message::user("Hello"),
        ];

        let converted = OpenAiCompatProvider::convert_messages(&messages);
        assert_eq!(converted[0].role, "system");
        let system = converted[0]
            .content
            .as_ref()
            .expect("system content preserved");
        assert!(system.contains("TRUTH AND VERIFICATION CONTRACT"));
        assert!(system.contains("unverified claims as confirmed facts"));
    }

    #[test]
    fn normalizes_scattered_system_messages_for_local_chat_templates() {
        let messages = vec![
            OaiMessage {
                role: "user".to_string(),
                content: Some("Hello".to_string()),
                tool_call_id: None,
                tool_calls: None,
                name: None,
            },
            OaiMessage {
                role: "system".to_string(),
                content: Some("Runtime contract".to_string()),
                tool_call_id: None,
                tool_calls: None,
                name: None,
            },
            OaiMessage {
                role: "assistant".to_string(),
                content: Some("Hi".to_string()),
                tool_call_id: None,
                tool_calls: None,
                name: None,
            },
            OaiMessage {
                role: "system".to_string(),
                content: Some("Verification contract".to_string()),
                tool_call_id: None,
                tool_calls: None,
                name: None,
            },
        ];

        let normalized =
            OpenAiCompatProvider::normalize_system_messages_for_chat_template(messages);
        assert_eq!(normalized.len(), 3);
        assert_eq!(normalized[0].role, "system");
        let system = normalized[0]
            .content
            .as_deref()
            .expect("system content is merged");
        assert!(system.contains("Runtime contract"));
        assert!(system.contains("Verification contract"));
        assert_eq!(normalized[1].role, "user");
        assert_eq!(normalized[2].role, "assistant");
        assert!(normalized[1..].iter().all(|message| message.role != "system"));
    }

    #[test]
    fn convert_response_to_choices_emits_provider_contract_extras() {
        let response = OaiChatResponse {
            choices: vec![OaiChoice {
                message: OaiResponseMessage {
                    content: None,
                    tool_calls: Some(vec![OaiResponseToolCall {
                        id: "call_1".to_string(),
                        r#type: "function".to_string(),
                        function: OaiResponseFunction {
                            name: "weather_lookup".to_string(),
                            arguments: "{\"location\":\"Shanghai\"}".to_string(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(OaiUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            timings: Some(OaiTimings {
                prompt_n: 10,
                prompt_ms: 12.0,
                predicted_ms: 7.0,
            }),
        };

        let choices = OpenAiCompatProvider::convert_response_to_choices(
            response,
            "openai".to_string(),
            "gpt-test".to_string(),
            12,
            "native_tool_calling",
            "stable",
            true,
            false,
        );

        assert!(choices.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::ToolCall { name, .. }) if name == "weather_lookup"
        )));
        assert!(choices
            .iter()
            .any(|choice| matches!(choice, Ok(StreamingChoice::Finish(FinishReason::ToolCalls)))));
        assert!(choices.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::Telemetry(telemetry))
                if telemetry.extra.get("finish_reason").map(String::as_str) == Some("tool_calls")
                    && telemetry.extra.get("tool_call_count").map(String::as_str) == Some("1")
                    && telemetry.extra.get("tool_contract_mode").map(String::as_str)
                        == Some("native_tool_calling")
                    && telemetry.extra.get("mainline_stability").map(String::as_str)
                        == Some("stable")
                    && telemetry
                        .continuation
                        .as_ref()
                        .and_then(|continuation| continuation.prompt_tokens)
                        == Some(10)
        )));
    }

    #[test]
    fn extract_pseudo_tool_calls_parses_default_tool_tag_contract() {
        let calls = OpenAiCompatProvider::extract_pseudo_tool_calls(
            "<|tool_call>call:default_tool:web_search{queries: [\"latest lancet heart treatment papers\"]}<tool_call|>\n\nnotice",
        );

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "web_search");
        assert_eq!(
            calls[0].function.arguments,
            serde_json::json!({
                "queries": ["latest lancet heart treatment papers"]
            })
            .to_string()
        );
    }

    #[test]
    fn extract_pseudo_tool_calls_parses_parenthesized_tool_contract() {
        let calls = OpenAiCompatProvider::extract_pseudo_tool_calls(
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
    fn convert_response_to_choices_repairs_pseudo_tool_call_content() {
        let response = OaiChatResponse {
            choices: vec![OaiChoice {
                message: OaiResponseMessage {
                    content: Some(
                        "<|tool_call>call:default_tool:web_search{queries: [\"latest lancet heart treatment papers\"]}<tool_call|>\n\nnotice"
                            .to_string(),
                    ),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            timings: None,
        };

        let choices = OpenAiCompatProvider::convert_response_to_choices(
            response,
            "openai".to_string(),
            "local-bridge".to_string(),
            8,
            "native_tool_calling",
            "stable",
            false,
            false,
        );

        assert!(choices.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::ToolCall { name, .. }) if name == "web_search"
        )));
        assert!(choices.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::Finish(FinishReason::ToolCalls))
        )));
        assert!(!choices.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::Message(text)) if text.contains("<|tool_call>")
        )));
    }
}
