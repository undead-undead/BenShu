//! OpenRouter provider implementation

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// OpenRouter API client (OpenAI compatible with model routing)
pub struct OpenRouter {
    inner: OpenAI,
}

impl OpenRouter {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://openrouter.ai/api/v1")?;
        Ok(Self { inner })
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .map_err(|_| Error::ProviderAuth("OPENROUTER_API_KEY not set".to_string()))?;
        Self::new(api_key)
    }
}

#[async_trait]
impl Provider for OpenRouter {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "openrouter"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "openrouter".to_string(),
            name: "OpenRouter".to_string(),
            description: "A unified API for every AI model. Access GPT-4, Claude 3, and more."
                .to_string(),
            icon: "🌐".to_string(),
            fields: vec![benshu_provider_core::ProviderField {
                key: "OPENROUTER_API_KEY".to_string(),
                label: "API Key".to_string(),
                field_type: "password".to_string(),
                description: "Your OpenRouter API Key".to_string(),
                required: true,
                default: None,
            }],
            capabilities: vec!["streaming".into(), "model_routing".into()],
            preferred_models: vec![
                "openai/gpt-5.4-thinking".into(),
                "google/gemini-3.1-flash".into(),
                "anthropic/claude-3.5-sonnet".into(),
                "meta-llama/llama-3.3-70b-instruct".into(),
            ],
        }
    }
}

/// Popular models on OpenRouter
/// Claude 3.5 Sonnet via OpenRouter
pub const CLAUDE_3_5_SONNET: &str = "anthropic/claude-3.5-sonnet";
/// Gemini 2.0 Flash via OpenRouter
pub const GEMINI_FLASH: &str = "google/gemini-2.0-flash-exp";
/// Llama 3.3 70B via OpenRouter
pub const LLAMA_70B: &str = "meta-llama/llama-3.3-70b-instruct";
