//! Baidu ERNIE Bot provider implementation
//!
//! ERNIE Bot is OpenAI-compatible in its newer API versions.
//! Base URL: https://qianfan.baidubce.com/v2

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// Baidu ERNIE API client (OpenAI compatible)
pub struct Baidu {
    inner: OpenAI,
}

impl Baidu {
    /// Create from API key (Access Token or API Key)
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://qianfan.baidubce.com/v2")?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl Provider for Baidu {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "baidu"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "baidu".to_string(),
            name: "Baidu ERNIE (文心一言)".to_string(),
            description: "Baidu's ERNIE Bot via Qianfan Platform".to_string(),
            icon: "🐻".to_string(),
            fields: vec![
                benshu_provider_core::ProviderField {
                    key: "baidu_api_key".to_string(),
                    label: "API Key".to_string(),
                    field_type: "password".to_string(),
                    description: "Your Baidu Qianfan API Key".to_string(),
                    required: true,
                    default: None,
                },
                benshu_provider_core::ProviderField {
                    key: "baidu_secret_key".to_string(),
                    label: "Secret Key".to_string(),
                    field_type: "password".to_string(),
                    description: "Your Baidu Qianfan Secret Key".to_string(),
                    required: false,
                    default: None,
                },
            ],
            capabilities: vec!["tools".to_string()],
            preferred_models: vec![
                "ernie-4.0-turbo-8k".to_string(),
                "ernie-3.5-8k".to_string(),
                "ernie-speed-128k".to_string(),
            ],
        }
    }
}
