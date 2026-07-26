//! Moonshot (Kimi) provider implementation
//!
//! Kimi AI is OpenAI-compatible.
//! Base URL: https://api.moonshot.cn/v1

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// Moonshot API client (OpenAI compatible)
pub struct Moonshot {
    inner: OpenAI,
}

impl Moonshot {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://api.moonshot.cn/v1")?;
        Ok(Self { inner })
    }

    /// Create from environment variable (MOONSHOT_API_KEY)
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("MOONSHOT_API_KEY")
            .map_err(|_| Error::ProviderAuth("MOONSHOT_API_KEY not set".to_string()))?;
        Self::new(api_key)
    }
}

#[async_trait]
impl Provider for Moonshot {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "moonshot"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "moonshot".to_string(),
            name: "Moonshot (Kimi)".to_string(),
            description: "High-performance LLMs from Moonshot AI".to_string(),
            icon: "🌙".to_string(),
            fields: vec![benshu_provider_core::ProviderField {
                key: "moonshot_api_key".to_string(),
                label: "API Key".to_string(),
                field_type: "password".to_string(),
                description: "Your Moonshot API Key".to_string(),
                required: true,
                default: None,
            }],
            capabilities: vec!["tools".to_string()],
            preferred_models: vec![
                "moonshot-v1-8k".to_string(),
                "moonshot-v1-32k".to_string(),
                "moonshot-v1-128k".to_string(),
            ],
        }
    }
}

/// Common model constants
pub const MOONSHOT_V1_8K: &str = "moonshot-v1-8k";
/// Moonshot v1 32k
pub const MOONSHOT_V1_32K: &str = "moonshot-v1-32k";
/// Moonshot v1 128k
pub const MOONSHOT_V1_128K: &str = "moonshot-v1-128k";
