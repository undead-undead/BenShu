//! SiliconFlow provider implementation
//!
//! SiliconFlow is a high-performance LLM aggregator, OpenAI-compatible.
//! Base URL: https://api.siliconflow.cn/v1

use async_trait::async_trait;

use crate::openai::OpenAI;
use crate::{Error, Provider, Result, StreamingResponse};

/// SiliconFlow API client (OpenAI compatible)
pub struct SiliconFlow {
    inner: OpenAI,
}

impl SiliconFlow {
    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let inner = OpenAI::with_base_url(api_key, "https://api.siliconflow.cn/v1")?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl Provider for SiliconFlow {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "siliconflow"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "siliconflow".to_string(),
            name: "SiliconFlow (硅基流动)".to_string(),
            description: "High-performance aggregator for various Chinese/Global models"
                .to_string(),
            icon: "⚡".to_string(),
            fields: vec![benshu_provider_core::ProviderField {
                key: "siliconflow_api_key".to_string(),
                label: "API Key".to_string(),
                field_type: "password".to_string(),
                description: "Your SiliconFlow API Key".to_string(),
                required: true,
                default: None,
            }],
            capabilities: vec!["tools".to_string()],
            preferred_models: vec![
                "deepseek-ai/DeepSeek-V3".to_string(),
                "deepseek-ai/DeepSeek-R1".to_string(),
                "THUDM/glm-4-9b-chat".to_string(),
                "Qwen/Qwen2.5-72B-Instruct".to_string(),
            ],
        }
    }
}
