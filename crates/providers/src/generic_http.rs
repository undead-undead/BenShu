use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use crate::{Error, Result, StreamingResponse, Provider, HttpConfig};
use benshu_provider_core::ChatRequest;
use futures::StreamExt;
use bytes::Bytes;

/// A generic HTTP provider that can be configured for various APIs.
/// It supports OpenAI-compatible endpoints by default but can be extended.
pub struct GenericHttpProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    name: String,
    /// Whether this is a local endpoint (skips some safety delays/audits)
    is_local: bool,
}

impl GenericHttpProvider {
    pub fn new(name: impl Into<String>, api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        let name_str = name.into();
        let url = base_url.into();
        let url_lower = url.to_lowercase();
        let is_local = url_lower.contains("localhost") || 
                       url_lower.contains("127.0.0.1") || 
                       url_lower.contains(".local") ||
                       url_lower.contains("ollama") ||
                       url_lower.contains("llama") ||
                       url_lower.contains("candle") ||
                       url.starts_with('/') || // Linux Path
                       url.contains(":\\");   // Windows Path
        
        let config = HttpConfig::default();
        let client = config.build_client()?;

        Ok(Self {
            client,
            api_key: api_key.into(),
            base_url: url,
            name: name_str,
            is_local,
        })
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        
        if !self.api_key.is_empty() && self.api_key != "none" {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                    .map_err(|e| Error::Internal(format!("Invalid API Key header: {}", e)))?,
            );
        }
        Ok(headers)
    }

    /// Map internal ChatRequest to OpenAI-compatible payload
    fn map_to_openai_payload(&self, request: ChatRequest) -> serde_json::Value {
        let mut messages = Vec::new();
        
        if let Some(system) = request.system_prompt {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system
            }));
        }

        for msg in request.messages {
            messages.push(serde_json::json!({
                "role": format!("{:?}", msg.role).to_lowercase(),
                "content": msg.text()
            }));
        }

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
        });

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(tokens) = request.max_tokens {
            // Local fallback: if tokens is extremely high, assume unlimited
            if tokens < 1_000_000 {
                body["max_tokens"] = serde_json::json!(tokens);
            }
        }

        // Merge extra params
        if let Some(extra) = request.extra_params {
            if let serde_json::Value::Object(extra_map) = extra {
                if let serde_json::Value::Object(ref mut body_map) = body {
                    for (k, v) in extra_map {
                        body_map.insert(k, v);
                    }
                }
            }
        }

        body
    }
}

#[async_trait]
impl Provider for GenericHttpProvider {
    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> Result<StreamingResponse> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let headers = self.build_headers()?;
        let body = self.map_to_openai_payload(request);

        let response = self.client.post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::ProviderError(format!("HTTP Request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(Error::ProviderError(format!("API Error ({}): {}", url, error_text)));
        }

        // Processing SSE stream
        let stream = response.bytes_stream();
        
        // Simple SSE parser for OpenAI-compatible streams
        let mapped_stream = stream.map(|chunk_res| {
            match chunk_res {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let mut content = String::new();
                    
                    for line in text.lines() {
                        if line.starts_with("data: ") {
                            let data = line.trim_start_matches("data: ").trim();
                            if data == "[DONE]" {
                                break;
                            }
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                if let Some(delta) = json["choices"][0]["delta"]["content"].as_str() {
                                    content.push_str(delta);
                                }
                            }
                        }
                    }
                    Ok(benshu_provider_core::StreamingChoice::Message(content))
                }
                Err(e) => Err(benshu_infra::error::Error::Internal(e.to_string())),
            }
        });

        Ok(StreamingResponse::new(Box::pin(mapped_stream)))
    }

    fn name(&self) -> &str {
        // Since &str must be static, we use a trick or return a generic name.
        // For production, we'd leak name_str once, but let's keep it safe.
        if self.is_local { "universal-local" } else { "universal-http" }
    }

    fn is_local(&self) -> bool {
        self.is_local
    }

    fn tool_contract_mode(&self) -> &'static str {
        if self.is_local {
            "prompt_json_tools"
        } else {
            "native_tool_calling"
        }
    }

    fn mainline_stability(&self) -> &'static str {
        if self.is_local {
            "transitional"
        } else {
            "stable"
        }
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata where Self: Sized {
        benshu_provider_core::ProviderMetadata {
            id: "universal".to_string(),
            name: "Universal HTTP".to_string(),
            description: "Connect to any standard OpenAI-compatible REST endpoint.".to_string(),
            icon: "🌐".to_string(),
            fields: vec![
                benshu_provider_core::ProviderField {
                    key: "base_url".to_string(),
                    label: "Base URL".to_string(),
                    field_type: "text".to_string(),
                    description: "Terminal URL (e.g. http://localhost:11434/v1)".to_string(),
                    required: true,
                    default: None,
                }
            ],
            capabilities: vec!["tools".to_string(), "streaming".to_string()],
            preferred_models: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GenericHttpProvider;
    use crate::Provider;

    #[test]
    fn local_generic_http_provider_explicitly_surfaces_transitional_contract() {
        let provider = GenericHttpProvider::new("local", "none", "http://localhost:11434/v1")
            .expect("provider should construct");

        assert!(provider.is_local());
        assert_eq!(provider.tool_contract_mode(), "prompt_json_tools");
        assert_eq!(provider.mainline_stability(), "transitional");
    }
}
