//! MiniMax provider implementation
//!
//! MiniMax provides high-performance LLMs with OpenAI-compatible API.

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// MiniMax API client (OpenAI compatible)
pub struct MiniMax {
    inner: OpenAI,
}

impl MiniMax {
    /// Create from API key
    ///
    /// Default MiniMax API URL for international users is `https://api.minimax.io/v1`
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://api.minimax.io/v1")?;
        Ok(Self { inner })
    }

    /// Create with custom base URL
    ///
    /// For users in China, use `https://api.minimaxi.com/v1`
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, base_url)?;
        Ok(Self { inner })
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("MINIMAX_API_KEY")
            .map_err(|_| Error::ProviderAuth("MINIMAX_API_KEY not set".to_string()))?;
        Self::new(api_key)
    }
}

#[async_trait]
impl Provider for MiniMax {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "minimax"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "minimax".to_string(),
            name: "MiniMax".to_string(),
            description: "High-performance Chinese and international LLM provider.".to_string(),
            icon: "🚀".to_string(),
            fields: vec![benshu_provider_core::ProviderField {
                key: "MINIMAX_API_KEY".to_string(),
                label: "API Key".to_string(),
                field_type: "password".to_string(),
                description: "Your MiniMax API Key".to_string(),
                required: true,
                default: None,
            }],
            capabilities: vec!["streaming".into()],
            preferred_models: vec!["MiniMax-M2.5".into(), "MiniMax-M2.1".into()],
        }
    }
}

/// Common model constants
/// MiniMax M2.5
pub const MINIMAX_M2_5: &str = "MiniMax-M2.5";
/// MiniMax M2.5 HighSpeed
pub const MINIMAX_M2_5_HIGHSPEED: &str = "MiniMax-M2.5-highspeed";
/// MiniMax M2.1
pub const MINIMAX_M2_1: &str = "MiniMax-M2.1";
