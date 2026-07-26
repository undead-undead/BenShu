//! Xunfei Spark (讯飞星火) provider implementation
//!
//! Spark is OpenAI-compatible in its v3+ HTTP API.
//! Base URL: https://spark-api-open.xf-yun.com/v1

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// Xunfei Spark API client (OpenAI compatible)
pub struct Xunfei {
    inner: OpenAI,
}

impl Xunfei {
    /// Create from API key (format: APIPassword:APIKey or just APIKey depending on console)
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://spark-api-open.xf-yun.com/v1")?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl Provider for Xunfei {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "xunfei"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "xunfei".to_string(),
            name: "Xunfei Spark (讯飞星火)".to_string(),
            description: "iFLYTEK Spark cognitive intelligence large model".to_string(),
            icon: "🔥".to_string(),
            fields: vec![benshu_provider_core::ProviderField {
                key: "xunfei_api_key".to_string(),
                label: "API Key".to_string(),
                field_type: "password".to_string(),
                description: "Your Xunfei Spark API Key (HTTP API)".to_string(),
                required: true,
                default: None,
            }],
            capabilities: vec!["tools".to_string()],
            preferred_models: vec![
                "generalv3.5".to_string(),
                "generalv3".to_string(),
                "pro-128k".to_string(),
            ],
        }
    }
}
