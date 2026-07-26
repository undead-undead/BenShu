//! Tongyi Qianwen (Qwen/Alibaba) provider implementation
//!
//! Qwen is OpenAI-compatible in its DashScope API.
//! Base URL: https://dashscope.aliyuncs.com/compatible-mode/v1

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// Qwen API client (OpenAI compatible)
pub struct Qwen {
    inner: OpenAI,
}

impl Qwen {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner =
            OpenAI::with_base_url(api_key, "https://dashscope.aliyuncs.com/compatible-mode/v1")?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl Provider for Qwen {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "qwen"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "qwen".to_string(),
            name: "Tongyi Qianwen (Qwen)".to_string(),
            description: "Powerful models from Alibaba Cloud DashScope".to_string(),
            icon: "☁️".to_string(),
            fields: vec![benshu_provider_core::ProviderField {
                key: "qwen_api_key".to_string(),
                label: "API Key".to_string(),
                field_type: "password".to_string(),
                description: "Your DashScope (Qwen) API Key".to_string(),
                required: true,
                default: None,
            }],
            capabilities: vec!["tools".to_string(), "vision".to_string()],
            preferred_models: vec![
                "qwen-max".to_string(),
                "qwen-plus".to_string(),
                "qwen-turbo".to_string(),
                "qwen-long".to_string(),
            ],
        }
    }
}
