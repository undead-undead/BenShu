//! DeepSeek provider implementation

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// DeepSeek API client (OpenAI compatible)
pub struct DeepSeek {
    inner: OpenAI,
}

impl DeepSeek {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://api.deepseek.com/v1")?;
        Ok(Self { inner })
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .map_err(|_| Error::ProviderAuth("DEEPSEEK_API_KEY not set".to_string()))?;
        Self::new(api_key)
    }
}

#[async_trait]
impl Provider for DeepSeek {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "deepseek"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "deepseek".to_string(),
            name: "DeepSeek".to_string(),
            description: "Cost-effective, high-performance models from DeepSeek.".to_string(),
            icon: "🐳".to_string(),
            fields: vec![benshu_provider_core::ProviderField {
                key: "DEEPSEEK_API_KEY".to_string(),
                label: "API Key".to_string(),
                field_type: "password".to_string(),
                description: "Your DeepSeek API Key".to_string(),
                required: true,
                default: None,
            }],
            capabilities: vec!["tools".into(), "streaming".into()],
            preferred_models: vec!["deepseek-chat".into(), "deepseek-coder".into()],
        }
    }
}

/// Common model constants
/// DeepSeek Chat
pub const DEEPSEEK_CHAT: &str = "deepseek-chat";
/// DeepSeek Coder
pub const DEEPSEEK_CODER: &str = "deepseek-coder";
