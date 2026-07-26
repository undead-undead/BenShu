//! Provider contracts for LLM integrations.

use async_trait::async_trait;
use std::sync::Arc;

pub use benshu_provider_core::{
    ChatRequest, Provider, ProviderCapabilityView, ProviderField, ProviderMetadata,
    ProviderRuntimePolicy,
};

mod resilient;

pub use resilient::{CircuitBreakerConfig, ResilientProvider};

pub struct MockProvider {
    pub responses: Arc<parking_lot::Mutex<std::collections::VecDeque<String>>>,
}

impl MockProvider {
    pub fn new(response: impl Into<String>) -> Self {
        let mut responses = std::collections::VecDeque::new();
        responses.push_back(response.into());
        Self {
            responses: Arc::new(parking_lot::Mutex::new(responses)),
        }
    }

    pub fn new_sequence(responses_in: Vec<impl Into<String>>) -> Self {
        let mut responses = std::collections::VecDeque::new();
        for r in responses_in {
            responses.push_back(r.into());
        }
        Self {
            responses: Arc::new(parking_lot::Mutex::new(responses)),
        }
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            id: "mock".to_string(),
            name: "Mock".to_string(),
            description: "Mock Provider".to_string(),
            icon: "".to_string(),
            fields: vec![],
            capabilities: vec![],
            preferred_models: vec![],
        }
    }

    async fn stream_completion(
        &self,
        _request: ChatRequest,
    ) -> benshu_infra::error::Result<benshu_provider_core::StreamingResponse> {
        let response = {
            let mut lock = self.responses.lock();
            if lock.len() > 1 {
                lock.pop_front().unwrap_or_default()
            } else {
                lock.front().cloned().unwrap_or_default()
            }
        };

        Ok(benshu_provider_core::MockStreamBuilder::new()
            .message(&response)
            .finish(benshu_provider_core::FinishReason::Stop)
            .telemetry(benshu_provider_core::ProviderTelemetry {
                provider_name: Some("mock".to_string()),
                model: None,
                latency_ms: Some(0),
                continuation: None,
                extra: std::collections::HashMap::new(),
            })
            .done()
            .build())
    }

    fn name(&self) -> &str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatRequest, MockProvider, Provider, ProviderCapabilityView, ProviderMetadata};
    use crate::agent::streaming::{FinishReason, StreamingChoice};
    use async_trait::async_trait;
    use futures::StreamExt;

    struct LocalDefaultProvider;

    #[async_trait]
    impl Provider for LocalDefaultProvider {
        fn metadata() -> ProviderMetadata {
            ProviderMetadata {
                id: "local-default".to_string(),
                name: "LocalDefault".to_string(),
                description: "test".to_string(),
                icon: "x".to_string(),
                fields: vec![],
                capabilities: vec!["runtime:local".to_string()],
                preferred_models: vec![],
            }
        }

        async fn stream_completion(
            &self,
            _request: ChatRequest,
        ) -> benshu_infra::error::Result<crate::agent::streaming::StreamingResponse> {
            unreachable!("not used in contract default test")
        }

        fn name(&self) -> &str {
            "local-default"
        }

        fn is_local(&self) -> bool {
            true
        }
    }

    #[test]
    fn capability_view_normalizes_common_runtime_signals() {
        let metadata = ProviderMetadata {
            id: "inference".to_string(),
            name: "Inference".to_string(),
            description: "test".to_string(),
            icon: "x".to_string(),
            fields: vec![],
            capabilities: vec![
                "vision".to_string(),
                "tools".to_string(),
                "streaming".to_string(),
                "runtime:local".to_string(),
                "runtime:fallback-enabled".to_string(),
                "runtime:context-window:128k".to_string(),
            ],
            preferred_models: vec![],
        };

        assert_eq!(
            metadata.capability_view(),
            ProviderCapabilityView {
                context_window_tokens: Some(128_000),
                supports_vision: true,
                supports_tools: true,
                supports_streaming: true,
                locality: "local".to_string(),
                has_fallback: true,
                tool_contract_mode: "native_tool_calling".to_string(),
                mainline_stability: "stable".to_string(),
                continuation: Default::default(),
            }
        );
    }

    #[test]
    fn capability_view_marks_hybrid_when_both_local_and_remote_signals_exist() {
        let metadata = ProviderMetadata {
            id: "resilient".to_string(),
            name: "Resilient".to_string(),
            description: "test".to_string(),
            icon: "x".to_string(),
            fields: vec![],
            capabilities: vec![
                "runtime:local".to_string(),
                "runtime:registry-bridge".to_string(),
            ],
            preferred_models: vec![],
        };

        assert_eq!(metadata.capability_view().locality, "hybrid");
    }

    #[test]
    fn capability_view_parses_contract_and_stability_markers() {
        let metadata = ProviderMetadata {
            id: "inference".to_string(),
            name: "Inference".to_string(),
            description: "test".to_string(),
            icon: "x".to_string(),
            fields: vec![],
            capabilities: vec![
                "tools".to_string(),
                "streaming".to_string(),
                "runtime:local".to_string(),
                "contract:prompt_json_tools".to_string(),
                "mainline:transitional".to_string(),
            ],
            preferred_models: vec![],
        };

        let capability_view = metadata.capability_view();
        assert_eq!(capability_view.tool_contract_mode, "prompt_json_tools");
        assert_eq!(capability_view.mainline_stability, "transitional");
    }

    #[test]
    fn runtime_policy_defaults_to_remote_stable_shape() {
        let provider = MockProvider::new("hello");
        let policy = provider.runtime_policy();
        assert_eq!(policy.locality, "remote");
        assert!(!policy.unlocks_full_context_window);
        assert_eq!(
            policy.session_token_quota,
            benshu_provider_core::API_SESSION_TOKEN_QUOTA
        );
    }

    #[test]
    fn local_provider_defaults_to_stable_native_contract_when_not_overridden() {
        let provider = LocalDefaultProvider;
        assert!(provider.is_local());
        assert_eq!(provider.tool_contract_mode(), "native_tool_calling");
        assert_eq!(provider.mainline_stability(), "stable");

        let policy = provider.runtime_policy();
        assert_eq!(policy.locality, "local");
        assert!(policy.unlocks_full_context_window);
    }

    #[tokio::test]
    async fn mock_provider_emits_tool_capable_response_contract() {
        let provider = MockProvider::new("hello");
        let stream = provider
            .stream_completion(ChatRequest::default())
            .await
            .expect("stream");
        let choices: Vec<_> = stream.collect().await;

        assert!(choices.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::Message(text)) if text == "hello"
        )));
        assert!(choices
            .iter()
            .any(|choice| matches!(choice, Ok(StreamingChoice::Finish(FinishReason::Stop)))));
        assert!(choices.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::Telemetry(telemetry))
                if telemetry.provider_name.as_deref() == Some("mock")
        )));
        assert!(choices
            .iter()
            .any(|choice| matches!(choice, Ok(StreamingChoice::Done))));
    }
}
