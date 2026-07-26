//! Zhipu AI (GLM) provider implementation
//!
//! GLM is OpenAI-compatible in its v4 API.
//! Base URL: https://open.bigmodel.cn/api/paas/v4

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// Zhipu API client (OpenAI compatible)
pub struct Zhipu {
    inner: OpenAI,
}

impl Zhipu {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://open.bigmodel.cn/api/paas/v4")?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl Provider for Zhipu {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "zhipu"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "zhipu".to_string(),
            name: "Zhipu AI (GLM)".to_string(),
            description: "Advanced LLMs from Zhipu (Tsinghua related)".to_string(),
            icon: "🧬".to_string(), // Or a specific logo
            fields: vec![benshu_provider_core::ProviderField {
                key: "zhipu_api_key".to_string(),
                label: "API Key".to_string(),
                field_type: "password".to_string(),
                description: "Your Zhipu API Key".to_string(),
                required: true,
                default: None,
            }],
            capabilities: vec!["tools".to_string(), "vision".to_string()],
            preferred_models: vec![
                "glm-4-plus".to_string(),
                "glm-4-flash".to_string(),
                "glm-4v".to_string(),
            ],
        }
    }
}
