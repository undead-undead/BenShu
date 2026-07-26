use crate::agent::context::ContextManager;
use crate::agent::message::{Message, Role};
use crate::agent::provider::{ChatRequest, Provider};

const LOCAL_CONTEXT_SAFETY_MARGIN_TOKENS: usize = 512;
const LOCAL_MIN_OUTPUT_TOKENS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalContextClampReport {
    pub effective_context_window_tokens: usize,
    pub estimated_prompt_tokens_before: usize,
    pub estimated_prompt_tokens_after: usize,
    pub requested_output_tokens_before: Option<u64>,
    pub requested_output_tokens_after: Option<u64>,
    pub dropped_message_count: usize,
    pub dropped_tool_count: usize,
}

pub(crate) fn effective_context_window_tokens<P: Provider + ?Sized>(
    provider: &P,
    model: &str,
    configured_context_tokens: Option<u64>,
) -> usize {
    let provider_window = provider.get_context_window(model).max(1);
    let configured_window = configured_context_tokens
        .and_then(|tokens| usize::try_from(tokens).ok())
        .filter(|tokens| *tokens > 0)
        .unwrap_or(provider_window);

    if provider.is_local() {
        configured_window.min(provider_window).max(1)
    } else {
        configured_window.max(1)
    }
}

pub(crate) fn clamp_local_chat_request_to_context<P: Provider + ?Sized>(
    provider: &P,
    configured_context_tokens: Option<u64>,
    default_output_tokens: usize,
    request: &mut ChatRequest,
) -> Option<LocalContextClampReport> {
    if !provider.is_local() {
        return None;
    }

    let context_window =
        effective_context_window_tokens(provider, &request.model, configured_context_tokens);
    let safety_margin = LOCAL_CONTEXT_SAFETY_MARGIN_TOKENS.min(context_window.saturating_div(8));
    let default_output = default_output_tokens
        .max(1)
        .min(context_window.saturating_sub(1).max(1));
    let requested_output_before = request.max_tokens;
    let requested_output = request
        .max_tokens
        .and_then(|tokens| usize::try_from(tokens).ok())
        .filter(|tokens| *tokens > 0)
        .unwrap_or(default_output)
        .min(context_window.saturating_sub(1).max(1));

    let prompt_before = estimate_chat_request_prompt_tokens(request);
    let mut target_output = requested_output;
    let mut dropped_message_count = 0usize;
    let mut dropped_tool_count = 0usize;

    if prompt_before
        .saturating_add(target_output)
        .saturating_add(safety_margin)
        > context_window
    {
        let available_output = context_window
            .saturating_sub(prompt_before)
            .saturating_sub(safety_margin);
        if available_output >= LOCAL_MIN_OUTPUT_TOKENS {
            target_output = target_output.min(available_output);
        } else {
            let minimum_output = LOCAL_MIN_OUTPUT_TOKENS
                .min(context_window.saturating_div(4).max(1))
                .max(1);
            target_output = target_output.min(minimum_output).max(1);
            let prompt_budget = context_window
                .saturating_sub(target_output)
                .saturating_sub(safety_margin)
                .max(1);

            let original_messages = request.messages.len();
            request.messages =
                fit_messages_to_prompt_budget(request, prompt_budget, provider.is_local());
            dropped_message_count = original_messages.saturating_sub(request.messages.len());

            while !request.tools.is_empty()
                && estimate_chat_request_prompt_tokens(request) > prompt_budget
            {
                request.tools.pop();
                dropped_tool_count = dropped_tool_count.saturating_add(1);
            }
            while request.messages.len() > 1
                && estimate_chat_request_prompt_tokens(request) > prompt_budget
            {
                request.messages.remove(0);
                dropped_message_count = dropped_message_count.saturating_add(1);
            }
            if estimate_chat_request_prompt_tokens(request) > prompt_budget {
                let per_message_chars = prompt_budget
                    .saturating_mul(3)
                    .checked_div(request.messages.len().max(1))
                    .unwrap_or(prompt_budget)
                    .max(128);
                for message in &mut request.messages {
                    message.soft_trim(per_message_chars);
                }
            }
            if estimate_chat_request_prompt_tokens(request) > prompt_budget {
                trim_system_prompt_to_budget(request, prompt_budget);
            }

            let mut prompt_after_trim = estimate_chat_request_prompt_tokens(request);
            let mut available_after_trim = context_window
                .saturating_sub(prompt_after_trim)
                .saturating_sub(safety_margin);
            if available_after_trim < minimum_output {
                let prompt_budget_for_minimum = context_window
                    .saturating_sub(minimum_output)
                    .saturating_sub(safety_margin)
                    .max(1);
                trim_messages_and_system_to_budget(request, prompt_budget_for_minimum);
                prompt_after_trim = estimate_chat_request_prompt_tokens(request);
                available_after_trim = context_window
                    .saturating_sub(prompt_after_trim)
                    .saturating_sub(safety_margin);
            }
            target_output = target_output
                .min(available_after_trim.max(1))
                .max(minimum_output.min(context_window.saturating_div(4).max(1)));
        }
    }

    request.max_tokens = Some(target_output as u64);
    let prompt_after = estimate_chat_request_prompt_tokens(request);

    annotate_context_budget(
        request,
        context_window,
        prompt_before,
        prompt_after,
        requested_output_before,
        Some(target_output as u64),
        dropped_message_count,
        dropped_tool_count,
    );

    Some(LocalContextClampReport {
        effective_context_window_tokens: context_window,
        estimated_prompt_tokens_before: prompt_before,
        estimated_prompt_tokens_after: prompt_after,
        requested_output_tokens_before: requested_output_before,
        requested_output_tokens_after: Some(target_output as u64),
        dropped_message_count,
        dropped_tool_count,
    })
}

pub(crate) fn estimate_chat_request_prompt_tokens(request: &ChatRequest) -> usize {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    if let Some(system_prompt) = request
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        messages.push(Message::system(system_prompt.to_string()));
    }
    messages.extend(request.messages.iter().cloned());

    let message_tokens = ContextManager::estimate_tokens(&messages);
    let tool_tokens = if request.tools.is_empty() {
        0
    } else {
        serde_json::to_string(&request.tools)
            .map(|json| json.chars().count().saturating_add(3) / 4)
            .unwrap_or_else(|_| {
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        tool.name.chars().count()
                            + tool.description.chars().count()
                            + tool
                                .usage_guidelines
                                .as_deref()
                                .map(|value| value.chars().count())
                                .unwrap_or(0)
                    })
                    .sum::<usize>()
                    .saturating_add(3)
                    / 4
            })
            .saturating_add(request.tools.len().saturating_mul(16))
    };

    message_tokens.saturating_add(tool_tokens)
}

fn fit_messages_to_prompt_budget(
    request: &ChatRequest,
    prompt_budget: usize,
    is_local: bool,
) -> Vec<Message> {
    let static_tokens = estimate_static_prompt_tokens(request);
    let message_budget = prompt_budget.saturating_sub(static_tokens).max(1);
    let mut kept = Vec::new();
    let mut used = 0usize;

    for message in request.messages.iter().rev() {
        let cost = ContextManager::estimate_tokens(std::slice::from_ref(message)).max(1);
        if used.saturating_add(cost) <= message_budget || kept.is_empty() {
            let mut candidate = message.clone();
            if used.saturating_add(cost) > message_budget {
                let char_limit = message_budget.saturating_mul(if is_local { 3 } else { 4 });
                candidate.soft_trim(char_limit.max(256));
            }
            used = used.saturating_add(
                ContextManager::estimate_tokens(std::slice::from_ref(&candidate)).max(1),
            );
            kept.push(candidate);
        }
    }

    kept.reverse();
    if kept.is_empty() {
        if let Some(last) = request.messages.last() {
            let mut candidate = last.clone();
            candidate.soft_trim(message_budget.saturating_mul(3).max(256));
            kept.push(candidate);
        }
    }

    if !kept
        .iter()
        .any(|message| matches!(message.role, Role::User))
    {
        if let Some(last_user) = request
            .messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, Role::User))
        {
            let mut candidate = last_user.clone();
            candidate.soft_trim(message_budget.saturating_mul(3).max(256));
            kept.push(candidate);
        }
    }

    kept
}

fn estimate_static_prompt_tokens(request: &ChatRequest) -> usize {
    let mut static_messages = Vec::new();
    if let Some(system_prompt) = request
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        static_messages.push(Message::system(system_prompt.to_string()));
    }
    let mut static_request = request.clone();
    static_request.messages = static_messages;
    estimate_chat_request_prompt_tokens(&static_request)
}

fn trim_system_prompt_to_budget(request: &mut ChatRequest, prompt_budget: usize) {
    if request.system_prompt.is_none() {
        return;
    }

    let prompt_budget = prompt_budget.max(1);
    let mut char_limit = prompt_budget.saturating_mul(3).max(128);
    while estimate_chat_request_prompt_tokens(request) > prompt_budget && char_limit > 128 {
        char_limit = char_limit.saturating_mul(3).saturating_div(4).max(128);
        if let Some(system_prompt) = request.system_prompt.as_mut() {
            trim_string_to_char_limit(system_prompt, char_limit);
        }
    }

    if estimate_chat_request_prompt_tokens(request) > prompt_budget {
        if let Some(system_prompt) = request.system_prompt.as_mut() {
            trim_string_to_char_limit(system_prompt, 128);
        }
    }
}

fn trim_messages_and_system_to_budget(request: &mut ChatRequest, prompt_budget: usize) {
    let prompt_budget = prompt_budget.max(1);
    let mut char_limit = prompt_budget.saturating_mul(3).max(128);
    while estimate_chat_request_prompt_tokens(request) > prompt_budget && char_limit > 128 {
        char_limit = char_limit.saturating_mul(3).saturating_div(4).max(128);
        for message in &mut request.messages {
            message.soft_trim(char_limit);
        }
        if let Some(system_prompt) = request.system_prompt.as_mut() {
            trim_string_to_char_limit(system_prompt, char_limit);
        }
    }

    if estimate_chat_request_prompt_tokens(request) > prompt_budget {
        for message in &mut request.messages {
            message.soft_trim(128);
        }
        if let Some(system_prompt) = request.system_prompt.as_mut() {
            trim_string_to_char_limit(system_prompt, 128);
        }
    }
}

fn trim_string_to_char_limit(value: &mut String, char_limit: usize) {
    if value.chars().count() <= char_limit {
        return;
    }

    let mut trimmed: String = value.chars().take(char_limit.saturating_sub(32)).collect();
    trimmed.push_str("\n\n[context-fit: truncated]");
    *value = trimmed;
}

fn annotate_context_budget(
    request: &mut ChatRequest,
    context_window: usize,
    prompt_before: usize,
    prompt_after: usize,
    output_before: Option<u64>,
    output_after: Option<u64>,
    dropped_messages: usize,
    dropped_tools: usize,
) {
    let mut extra = request
        .extra_params
        .take()
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    if !extra.is_object() {
        extra = serde_json::Value::Object(serde_json::Map::new());
    }
    if let Some(map) = extra.as_object_mut() {
        map.insert(
            "effective_context_window_tokens".to_string(),
            serde_json::json!(context_window),
        );
        map.insert(
            "estimated_prompt_tokens_before_context_fit".to_string(),
            serde_json::json!(prompt_before),
        );
        map.insert(
            "estimated_prompt_tokens_after_context_fit".to_string(),
            serde_json::json!(prompt_after),
        );
        if let Some(value) = output_before {
            map.insert(
                "requested_output_tokens_before_context_fit".to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = output_after {
            map.insert(
                "requested_output_tokens_after_context_fit".to_string(),
                serde_json::json!(value),
            );
        }
        if dropped_messages > 0 || dropped_tools > 0 || output_before != output_after {
            map.insert("context_fit_applied".to_string(), serde_json::json!(true));
            map.insert(
                "context_fit_dropped_messages".to_string(),
                serde_json::json!(dropped_messages),
            );
            map.insert(
                "context_fit_dropped_tools".to_string(),
                serde_json::json!(dropped_tools),
            );
        }
    }
    request.extra_params = Some(extra);
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use benshu_infra::traits::tool::ToolDefinition;
    use benshu_provider_core::{ProviderMetadata, StreamingResponse};

    #[derive(Debug)]
    struct LocalBudgetProvider {
        context_window: usize,
    }

    #[async_trait]
    impl Provider for LocalBudgetProvider {
        async fn stream_completion(
            &self,
            _request: ChatRequest,
        ) -> benshu_infra::error::Result<StreamingResponse> {
            unreachable!("not used")
        }

        fn name(&self) -> &str {
            "local-budget"
        }

        fn is_local(&self) -> bool {
            true
        }

        fn get_context_window(&self, _model: &str) -> usize {
            self.context_window
        }

        fn metadata() -> ProviderMetadata
        where
            Self: Sized,
        {
            ProviderMetadata {
                id: "local-budget".to_string(),
                name: "Local Budget".to_string(),
                description: "test".to_string(),
                icon: "x".to_string(),
                fields: vec![],
                capabilities: vec!["runtime:local".to_string()],
                preferred_models: vec![],
            }
        }
    }

    #[test]
    fn local_context_window_prefers_lower_configured_limit() {
        let provider = LocalBudgetProvider {
            context_window: 131_072,
        };

        assert_eq!(
            effective_context_window_tokens(&provider, "model", Some(32_768)),
            32_768
        );
    }

    #[test]
    fn local_context_window_never_exceeds_provider_limit() {
        let provider = LocalBudgetProvider {
            context_window: 8_192,
        };

        assert_eq!(
            effective_context_window_tokens(&provider, "model", Some(128_000)),
            8_192
        );
    }

    #[test]
    fn local_request_clamp_reduces_output_to_fit_context() {
        let provider = LocalBudgetProvider {
            context_window: 2_048,
        };
        let mut request = ChatRequest {
            model: "local".to_string(),
            system_prompt: Some("You are concise.".to_string()),
            messages: vec![Message::user("简短回答：你好")],
            max_tokens: Some(4_096),
            ..Default::default()
        };

        let report =
            clamp_local_chat_request_to_context(&provider, Some(2_048), 1_024, &mut request)
                .expect("local clamp report");

        let prompt = estimate_chat_request_prompt_tokens(&request);
        let output = request.max_tokens.unwrap() as usize;
        assert!(prompt + output + LOCAL_CONTEXT_SAFETY_MARGIN_TOKENS.min(256) <= 2_048);
        assert!(report.requested_output_tokens_after.unwrap() < 4_096);
    }

    #[test]
    fn local_request_clamp_can_drop_oversized_history() {
        let provider = LocalBudgetProvider {
            context_window: 2_048,
        };
        let long_text = "历史内容".repeat(4000);
        let mut request = ChatRequest {
            model: "local".to_string(),
            system_prompt: Some("You are concise.".to_string()),
            messages: vec![
                Message::assistant(long_text),
                Message::user("请继续回答当前问题。"),
            ],
            max_tokens: Some(512),
            tools: vec![ToolDefinition {
                name: "small_tool".to_string(),
                description: "Small test tool".to_string(),
                parameters: serde_json::json!({"type":"object","properties":{}}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: Default::default(),
                usage_guidelines: None,
            }],
            ..Default::default()
        };

        let report =
            clamp_local_chat_request_to_context(&provider, Some(2_048), 1_024, &mut request)
                .expect("local clamp report");

        assert!(report.dropped_message_count > 0);
        assert!(request
            .messages
            .iter()
            .any(|message| matches!(message.role, Role::User)));
        assert!(estimate_chat_request_prompt_tokens(&request) < 2_048);
    }

    #[test]
    fn local_request_clamp_preserves_minimum_output_for_oversized_single_prompt() {
        let provider = LocalBudgetProvider {
            context_window: 2_048,
        };
        let mut request = ChatRequest {
            model: "local".to_string(),
            system_prompt: Some("You write artifacts.".to_string()),
            messages: vec![Message::user("超长上下文".repeat(5000))],
            max_tokens: Some(4_096),
            ..Default::default()
        };

        let report =
            clamp_local_chat_request_to_context(&provider, Some(2_048), 1_024, &mut request)
                .expect("local clamp report");

        assert!(request.max_tokens.unwrap() >= LOCAL_MIN_OUTPUT_TOKENS as u64);
        assert!(
            estimate_chat_request_prompt_tokens(&request)
                + request.max_tokens.unwrap() as usize
                + LOCAL_CONTEXT_SAFETY_MARGIN_TOKENS.min(256)
                <= 2_048
        );
        assert!(report.estimated_prompt_tokens_after < report.estimated_prompt_tokens_before);
    }
}
