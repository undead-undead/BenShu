//! Doubao (ByteDance/Volcengine) provider implementation
//!
//! Doubao is OpenAI-compatible in its Ark API v3.
//! Base URL: https://ark.cn-beijing.volces.com/api/v3

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// Doubao API client (OpenAI compatible)
pub struct Doubao {
    inner: OpenAI,
}

impl Doubao {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://ark.cn-beijing.volces.com/api/v3")?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl Provider for Doubao {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "doubao"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "doubao".to_string(),
            name: "Doubao (豆包)".to_string(),
            description: "Advanced LLMs from ByteDance Volcengine Ark".to_string(),
            icon: "🌋".to_string(),
            fields: vec![benshu_provider_core::ProviderField {
                key: "doubao_api_key".to_string(),
                label: "API Key".to_string(),
                field_type: "password".to_string(),
                description: "Your Volcengine Ark API Key".to_string(),
                required: true,
                default: None,
            }],
            capabilities: vec!["tools".to_string(), "vision".to_string()],
            preferred_models: vec![
                "ep-20240523000001-f9b88".to_string(), // Typical endpoint ID
                "doubao-pro-32k".to_string(),
                "doubao-lite-32k".to_string(),
            ],
        }
    }
}
