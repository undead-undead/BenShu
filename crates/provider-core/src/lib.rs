use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod streaming;

pub use streaming::{
    FinishReason, MockStreamBuilder, ProviderTelemetry, StreamingChoice, StreamingResponse,
    StreamingResult, Usage,
};

pub const API_SESSION_TOKEN_QUOTA: usize = 180_000;
pub const LOCAL_SESSION_TOKEN_QUOTA: usize = 600_000;

/// Request for a chat completion.
#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    /// Model name to use.
    pub model: String,
    /// Optional system prompt.
    pub system_prompt: Option<String>,
    /// Conversation history.
    pub messages: Vec<benshu_protocol_core::Message>,
    /// Available tools.
    pub tools: Vec<benshu_infra::traits::tool::ToolDefinition>,
    /// Optional temperature setting.
    pub temperature: Option<f64>,
    /// Optional max tokens.
    pub max_tokens: Option<u64>,
    /// Optional provider-specific parameters.
    pub extra_params: Option<serde_json::Value>,
    /// Whether to emit provider-specific prompt cache-control markers.
    pub enable_cache_control: bool,
    /// Session identifier for telemetry and continuation correlation.
    pub session_id: Option<String>,
    /// Optional continuation metadata for provider/runtime correlation.
    pub continuation_hint: Option<ContinuationHint>,
}

/// Metadata field for provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub description: String,
    pub required: bool,
    pub default: Option<String>,
}

/// Metadata describing an LLM provider's capabilities and schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub fields: Vec<ProviderField>,
    pub capabilities: Vec<String>,
    pub preferred_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProviderCapabilityView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<usize>,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default)]
    pub supports_tools: bool,
    #[serde(default)]
    pub supports_streaming: bool,
    #[serde(default = "default_provider_locality")]
    pub locality: String,
    #[serde(default)]
    pub has_fallback: bool,
    #[serde(default = "default_tool_contract_mode")]
    pub tool_contract_mode: String,
    #[serde(default = "default_mainline_stability")]
    pub mainline_stability: String,
    #[serde(default)]
    pub continuation: ProviderContinuationCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderContinuationCapability {
    #[serde(default)]
    pub tool_call_exact_replay: bool,
    #[serde(default)]
    pub protocol_live_continuation: bool,
    #[serde(default)]
    pub thinking_final_split: bool,
    #[serde(default)]
    pub structured_context_errors: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContinuationHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_frontier_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_prompt_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContinuationTelemetry {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub cache_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miss_reason: Option<String>,
    #[serde(default)]
    pub tool_exact_replay_used: bool,
    #[serde(default)]
    pub protocol_live_continuation_used: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextLimitError {
    pub prompt_tokens: u32,
    pub configured_context_tokens: u32,
    pub requested_output_tokens: u32,
    pub overflow_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub largest_section: Option<String>,
    #[serde(default)]
    pub recommended_actions: Vec<String>,
}

impl ContextLimitError {
    pub const PROVIDER_ERROR_MARKER: &'static str = "BENSHU_CONTEXT_LIMIT_ERROR";

    pub fn new(
        prompt_tokens: u32,
        configured_context_tokens: u32,
        requested_output_tokens: u32,
    ) -> Self {
        let requested_total = prompt_tokens.saturating_add(requested_output_tokens);
        let overflow_tokens = requested_total.saturating_sub(configured_context_tokens);
        Self {
            prompt_tokens,
            configured_context_tokens,
            requested_output_tokens,
            overflow_tokens,
            largest_section: None,
            recommended_actions: vec![
                "compress_context".to_string(),
                "split_task".to_string(),
                "reduce_single_step_output".to_string(),
                "move_large_content_to_artifact_or_context_package".to_string(),
                "increase_runtime_context_window_if_available".to_string(),
            ],
        }
    }

    pub fn with_largest_section(mut self, section: impl Into<String>) -> Self {
        let section = section.into();
        if !section.trim().is_empty() {
            self.largest_section = Some(section);
        }
        self
    }

    pub fn to_provider_error_message(&self) -> String {
        let payload = serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string());
        format!("{}:{}", Self::PROVIDER_ERROR_MARKER, payload)
    }

    pub fn from_provider_error_message(message: &str) -> Option<Self> {
        let (_, payload) = message.split_once(Self::PROVIDER_ERROR_MARKER)?;
        let payload = payload.trim_start_matches(':').trim();
        let end = payload.rfind('}').map(|idx| idx + 1)?;
        serde_json::from_str(&payload[..end]).ok()
    }

    pub fn looks_like_context_limit_message(message: &str) -> bool {
        let lowered = message.to_ascii_lowercase();
        [
            "context length",
            "maximum context",
            "context window",
            "prompt is too long",
            "too many tokens",
            "tokens exceed",
            "exceeds the context",
            "exceed context",
            "n_ctx",
            "context_size",
            "max context",
            "上下文",
            "提示词过长",
            "超过",
        ]
        .iter()
        .any(|needle| lowered.contains(needle))
    }

    pub fn as_user_blocker(&self, prefers_chinese: bool) -> String {
        if prefers_chinese {
            format!(
                "status: blocked\nerror_kind: context_limit_exceeded\nblockers: 上下文超过当前运行时窗口，系统没有静默裁剪。\nprompt_tokens: {}\nconfigured_context_tokens: {}\nrequested_output_tokens: {}\noverflow_tokens: {}\nlargest_section: {}\nnext_step_hint: 上下文超过当前运行时窗口，系统没有静默裁剪。下一步应压缩上下文、拆分任务、把大内容放入 artifact/context package，或在面板调大可用上下文后重试。",
                self.prompt_tokens,
                self.configured_context_tokens,
                self.requested_output_tokens,
                self.overflow_tokens,
                self.largest_section.as_deref().unwrap_or("unknown")
            )
        } else {
            format!(
                "status: blocked\nerror_kind: context_limit_exceeded\nblockers: The context exceeds the current runtime window, so the runtime did not silently truncate it.\nprompt_tokens: {}\nconfigured_context_tokens: {}\nrequested_output_tokens: {}\noverflow_tokens: {}\nlargest_section: {}\nnext_step_hint: The context exceeds the current runtime window, so the runtime did not silently truncate it. Compress context, split the task, move large content into artifacts/context packages, or increase the runtime context window before retrying.",
                self.prompt_tokens,
                self.configured_context_tokens,
                self.requested_output_tokens,
                self.overflow_tokens,
                self.largest_section.as_deref().unwrap_or("unknown")
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRuntimePolicy {
    pub locality: String,
    pub unlocks_full_context_window: bool,
    pub session_token_quota: usize,
}

/// Shared provider contract implemented by concrete LLM backends.
#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse>;

    fn name(&self) -> &str;

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn is_local(&self) -> bool {
        false
    }

    fn tool_contract_mode(&self) -> &'static str {
        "native_tool_calling"
    }

    fn mainline_stability(&self) -> &'static str {
        "stable"
    }

    fn runtime_policy(&self) -> ProviderRuntimePolicy {
        if self.is_local() {
            ProviderRuntimePolicy {
                locality: "local".to_string(),
                unlocks_full_context_window: true,
                session_token_quota: LOCAL_SESSION_TOKEN_QUOTA,
            }
        } else {
            ProviderRuntimePolicy {
                locality: "remote".to_string(),
                unlocks_full_context_window: false,
                session_token_quota: API_SESSION_TOKEN_QUOTA,
            }
        }
    }

    fn metadata() -> ProviderMetadata
    where
        Self: Sized;

    async fn get_dynamic_metadata(&self) -> benshu_infra::error::Result<ProviderMetadata> {
        Ok(ProviderMetadata {
            id: self.name().to_string(),
            name: self.name().to_string(),
            description: "Default provider metadata".to_string(),
            icon: "🤖".to_string(),
            fields: vec![],
            capabilities: vec![],
            preferred_models: vec![],
        })
    }

    async fn batch_completion(
        &self,
        requests: Vec<ChatRequest>,
    ) -> benshu_infra::error::Result<Vec<benshu_infra::error::Result<StreamingResponse>>> {
        let mut results = Vec::new();
        for req in requests {
            results.push(self.stream_completion(req).await);
        }
        Ok(results)
    }

    fn get_context_window(&self, model: &str) -> usize {
        let _ = model;
        self.runtime_policy().session_token_quota
    }

    fn trim_messages(
        &self,
        messages: Vec<benshu_protocol_core::Message>,
        model: &str,
    ) -> Vec<benshu_protocol_core::Message> {
        let limit = self.get_context_window(model);
        let char_limit = (limit as f32 * 4.0 * 0.8) as usize;
        let mut current_chars = 0;
        let mut trimmed = Vec::new();

        for msg in messages.into_iter().rev() {
            let msg_len = msg.text().len();
            if current_chars + msg_len < char_limit {
                current_chars += msg_len;
                trimmed.push(msg);
            } else if trimmed.is_empty() {
                let mut m = msg;
                m.content.soft_trim(char_limit);
                trimmed.push(m);
                break;
            } else {
                break;
            }
        }

        trimmed.reverse();
        trimmed
    }

    async fn get_session_usage(&self) -> Option<benshu_infra::TokenUsage> {
        None
    }
}

#[async_trait]
impl Provider for std::sync::Arc<dyn Provider> {
    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        self.as_ref().stream_completion(request).await
    }

    fn name(&self) -> &str {
        self.as_ref().name()
    }

    fn supports_streaming(&self) -> bool {
        self.as_ref().supports_streaming()
    }

    fn supports_tools(&self) -> bool {
        self.as_ref().supports_tools()
    }

    fn is_local(&self) -> bool {
        self.as_ref().is_local()
    }

    fn tool_contract_mode(&self) -> &'static str {
        self.as_ref().tool_contract_mode()
    }

    fn mainline_stability(&self) -> &'static str {
        self.as_ref().mainline_stability()
    }

    fn runtime_policy(&self) -> ProviderRuntimePolicy {
        self.as_ref().runtime_policy()
    }

    fn metadata() -> ProviderMetadata
    where
        Self: Sized,
    {
        ProviderMetadata {
            id: "dynamic".to_string(),
            name: "Dynamic Provider wrapper".to_string(),
            description: "A dynamically assigned provider".to_string(),
            icon: "🧬".to_string(),
            fields: vec![],
            capabilities: vec![],
            preferred_models: vec![],
        }
    }

    async fn get_dynamic_metadata(&self) -> benshu_infra::error::Result<ProviderMetadata> {
        self.as_ref().get_dynamic_metadata().await
    }

    async fn batch_completion(
        &self,
        requests: Vec<ChatRequest>,
    ) -> benshu_infra::error::Result<Vec<benshu_infra::error::Result<StreamingResponse>>> {
        self.as_ref().batch_completion(requests).await
    }

    fn get_context_window(&self, model: &str) -> usize {
        self.as_ref().get_context_window(model)
    }

    fn trim_messages(
        &self,
        messages: Vec<benshu_protocol_core::Message>,
        model: &str,
    ) -> Vec<benshu_protocol_core::Message> {
        self.as_ref().trim_messages(messages, model)
    }

    async fn get_session_usage(&self) -> Option<benshu_infra::TokenUsage> {
        self.as_ref().get_session_usage().await
    }
}

fn default_provider_locality() -> String {
    "remote".to_string()
}

fn default_tool_contract_mode() -> String {
    "native_tool_calling".to_string()
}

fn default_mainline_stability() -> String {
    "stable".to_string()
}

impl ProviderMetadata {
    pub fn capability_view(&self) -> ProviderCapabilityView {
        let capabilities = self
            .capabilities
            .iter()
            .map(|capability| capability.to_lowercase())
            .collect::<Vec<_>>();
        let context_window_tokens = capabilities
            .iter()
            .filter_map(|capability| parse_context_window_tokens(capability))
            .max();
        let supports_vision = capabilities
            .iter()
            .any(|capability| capability.contains("vision") || capability.contains("multimodal"));
        let supports_tools = capabilities
            .iter()
            .any(|capability| capability.contains("tool"));
        let supports_streaming = capabilities
            .iter()
            .any(|capability| capability.contains("stream"));
        let has_fallback = capabilities
            .iter()
            .any(|capability| capability.contains("fallback"));
        let tool_contract_mode = capabilities
            .iter()
            .find_map(|capability| {
                capability
                    .strip_prefix("contract:")
                    .map(|value| value.to_string())
            })
            .unwrap_or_else(|| {
                if capabilities
                    .iter()
                    .any(|capability| capability.contains("prompt-json"))
                {
                    "prompt_json_tools".to_string()
                } else {
                    "native_tool_calling".to_string()
                }
            });
        let has_local_signal = capabilities.iter().any(|capability| {
            capability.contains("runtime:local")
                || capability == "local"
                || capability.contains("on-device")
        });
        let has_remote_signal = capabilities.iter().any(|capability| {
            capability.contains("runtime:registry")
                || capability.contains("remote")
                || capability.contains("cloud")
                || capability.contains("api")
        });
        let locality = match (has_local_signal, has_remote_signal) {
            (true, true) => "hybrid".to_string(),
            (true, false) => "local".to_string(),
            (false, true) => "remote".to_string(),
            (false, false) => "remote".to_string(),
        };
        let mainline_stability = capabilities
            .iter()
            .find_map(|capability| {
                capability
                    .strip_prefix("mainline:")
                    .map(|value| value.to_string())
            })
            .unwrap_or_else(|| {
                if tool_contract_mode == "prompt_json_tools" {
                    "transitional".to_string()
                } else {
                    "stable".to_string()
                }
            });
        let continuation = ProviderContinuationCapability {
            tool_call_exact_replay: capabilities
                .iter()
                .any(|capability| capability == "continuation:tool_exact_replay"),
            protocol_live_continuation: capabilities
                .iter()
                .any(|capability| capability == "continuation:protocol_live"),
            thinking_final_split: capabilities.iter().any(|capability| {
                capability == "continuation:thinking_final_split"
                    || capability == "thinking_final_split"
            }),
            structured_context_errors: capabilities
                .iter()
                .any(|capability| capability == "continuation:structured_context_errors"),
        };

        ProviderCapabilityView {
            context_window_tokens,
            supports_vision,
            supports_tools,
            supports_streaming,
            locality,
            has_fallback,
            tool_contract_mode,
            mainline_stability,
            continuation,
        }
    }
}

fn parse_context_window_tokens(capability: &str) -> Option<usize> {
    if !capability.contains("context") {
        return None;
    }

    let bytes = capability.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx].is_ascii_digit() {
            let start = idx;
            while idx < bytes.len() && bytes[idx].is_ascii_digit() {
                idx += 1;
            }
            let number = capability[start..idx].parse::<usize>().ok()?;
            let multiplier = match bytes.get(idx).copied().map(char::from) {
                Some('k') | Some('K') => 1_000,
                Some('m') | Some('M') => 1_000_000,
                _ => 1,
            };
            return Some(number.saturating_mul(multiplier));
        }
        idx += 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_view_parses_context_and_locality() {
        let metadata = ProviderMetadata {
            id: "local".to_string(),
            name: "Local".to_string(),
            description: String::new(),
            icon: String::new(),
            fields: Vec::new(),
            capabilities: vec![
                "runtime:local".to_string(),
                "context:128k".to_string(),
                "vision".to_string(),
                "contract:prompt_json_tools".to_string(),
            ],
            preferred_models: Vec::new(),
        };

        let view = metadata.capability_view();
        assert_eq!(view.context_window_tokens, Some(128_000));
        assert_eq!(view.locality, "local");
        assert!(view.supports_vision);
        assert_eq!(view.tool_contract_mode, "prompt_json_tools");
    }

    #[test]
    fn chat_request_defaults_to_empty_payload() {
        let request = ChatRequest::default();
        assert!(request.model.is_empty());
        assert!(request.messages.is_empty());
        assert!(request.tools.is_empty());
        assert!(request.session_id.is_none());
        assert!(request.continuation_hint.is_none());
    }

    #[test]
    fn local_native_tool_contract_defaults_to_stable() {
        let metadata = ProviderMetadata {
            id: "local".to_string(),
            name: "Local".to_string(),
            description: String::new(),
            icon: String::new(),
            fields: Vec::new(),
            capabilities: vec!["runtime:local".to_string(), "tools".to_string()],
            preferred_models: Vec::new(),
        };

        let view = metadata.capability_view();
        assert_eq!(view.locality, "local");
        assert_eq!(view.tool_contract_mode, "native_tool_calling");
        assert_eq!(view.mainline_stability, "stable");
    }

    #[test]
    fn capability_view_parses_continuation_capabilities() {
        let metadata = ProviderMetadata {
            id: "local".to_string(),
            name: "Local".to_string(),
            description: String::new(),
            icon: String::new(),
            fields: Vec::new(),
            capabilities: vec![
                "runtime:local".to_string(),
                "context:256k".to_string(),
                "continuation:tool_exact_replay".to_string(),
                "continuation:structured_context_errors".to_string(),
            ],
            preferred_models: Vec::new(),
        };

        let view = metadata.capability_view();
        assert!(view.continuation.tool_call_exact_replay);
        assert!(view.continuation.structured_context_errors);
    }

    #[test]
    fn context_limit_error_roundtrips_through_provider_message() {
        let error =
            ContextLimitError::new(130_000, 128_000, 4_096).with_largest_section("dynamic_context");
        let message = error.to_provider_error_message();
        let parsed = ContextLimitError::from_provider_error_message(&message).expect("parsed");

        assert_eq!(parsed.prompt_tokens, 130_000);
        assert_eq!(parsed.configured_context_tokens, 128_000);
        assert_eq!(parsed.requested_output_tokens, 4_096);
        assert_eq!(parsed.overflow_tokens, 6_096);
        assert_eq!(parsed.largest_section.as_deref(), Some("dynamic_context"));
        assert!(ContextLimitError::looks_like_context_limit_message(
            "maximum context length exceeded"
        ));
        assert!(parsed
            .as_user_blocker(true)
            .contains("context_limit_exceeded"));
    }
}
