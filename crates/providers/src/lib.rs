//! # BenShu Providers
//!
//! LLM Provider implementations for BenShu (AI Agent Trade).
//!
//! Includes support for OpenAI, Anthropic, Gemini, etc.

#![warn(missing_docs)]

// Re-export core types for convenience
pub use benshu_infra::error::{Error, Result};
pub use benshu_infra::traits::tool::ToolDefinition;
pub use benshu_protocol_core::Message;
pub use benshu_provider_core::Provider;
pub use benshu_provider_core::{StreamingChoice, StreamingResponse};
use parking_lot::RwLock;
pub use std::sync::Arc;

pub mod mock;
pub use mock::MockProvider;
pub mod utils;

#[cfg(feature = "openai")]
pub mod openai;

#[cfg(feature = "anthropic")]
pub mod anthropic;

#[cfg(feature = "gemini")]
pub mod gemini;

#[cfg(feature = "deepseek")]
pub mod deepseek;

#[cfg(feature = "openrouter")]
pub mod openrouter;

#[cfg(feature = "moonshot")]
pub mod moonshot;

#[cfg(feature = "zhipu")]
pub mod zhipu;

#[cfg(feature = "qwen")]
pub mod qwen;

#[cfg(feature = "doubao")]
pub mod doubao;

#[cfg(feature = "siliconflow")]
pub mod siliconflow;

#[cfg(feature = "baidu")]
pub mod baidu;

#[cfg(feature = "xunfei")]
pub mod xunfei;

#[cfg(feature = "groq")]
pub mod groq;

#[cfg(feature = "minimax")]
pub mod minimax;

#[cfg(feature = "llama_cpp")]
pub mod llama_cpp;

pub mod native;

#[cfg(test)]
mod provider_tests;

/// HTTP client configuration
#[derive(Clone)]
pub struct HttpConfig {
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Connection pool idle timeout
    pub pool_idle_timeout_secs: u64,
    /// Max idle connections per host
    pub pool_max_idle_per_host: usize,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 60,
            pool_idle_timeout_secs: 90,
            pool_max_idle_per_host: 32,
        }
    }
}

impl HttpConfig {
    /// Build a reqwest client
    pub fn build_client(&self) -> Result<reqwest::Client> {
        use std::time::Duration;

        reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_secs))
            .pool_idle_timeout(Duration::from_secs(self.pool_idle_timeout_secs))
            .pool_max_idle_per_host(self.pool_max_idle_per_host)
            .build()
            .map_err(|e| Error::Internal(e.to_string()))
    }
}

/// Factory for creating providers by name (Phase 21.4 Evolution)
pub async fn create_provider(
    name: &str,
    base_url: Option<String>,
    api_key: Option<String>,
    kv_engine: Option<Arc<RwLock<benshu_inference::KvEngine>>>,
) -> Result<Arc<dyn Provider>> {
    match name.to_lowercase().as_str() {
        #[cfg(feature = "openai")]
        "openai" => {
            let key = api_key.ok_or_else(|| Error::Internal("OpenAI API key missing".into()))?;
            let is_custom_compat = base_url
                .as_deref()
                .map(|url| !url.trim().is_empty())
                .unwrap_or(false);
            if !is_custom_compat {
                utils::validate_api_key(&key, "openai")?;
            } else if key.trim().is_empty() {
                return Err(Error::ProviderAuth(
                    "Custom OpenAI-compatible provider requires a non-empty API key".into(),
                ));
            }
            tracing::info!(
                "Initializing OpenAI provider with key: {}",
                utils::mask_api_key(&key)
            );

            let provider = if let Some(url) = base_url {
                openai::OpenAI::with_base_url(key, url)?
            } else {
                openai::OpenAI::new(key)?
            };
            Ok(Arc::new(provider))
        }
        #[cfg(feature = "anthropic")]
        "anthropic" => {
            let key = api_key.ok_or_else(|| Error::Internal("Anthropic API key missing".into()))?;
            utils::validate_api_key(&key, "anthropic")?;
            tracing::info!(
                "Initializing Anthropic provider with key: {}",
                utils::mask_api_key(&key)
            );
            Ok(Arc::new(anthropic::Anthropic::new(key)?))
        }
        #[cfg(feature = "gemini")]
        "gemini" => {
            let key = api_key.ok_or_else(|| Error::Internal("Gemini API key missing".into()))?;
            utils::validate_api_key(&key, "gemini")?;
            tracing::info!(
                "Initializing Gemini provider with key: {}",
                utils::mask_api_key(&key)
            );
            Ok(Arc::new(gemini::Gemini::new(key)?))
        }
        #[cfg(feature = "deepseek")]
        "deepseek" => {
            let key = api_key.ok_or_else(|| Error::Internal("DeepSeek API key missing".into()))?;
            Ok(Arc::new(deepseek::DeepSeek::new(key)?))
        }
        #[cfg(feature = "groq")]
        "groq" => {
            let key = api_key.ok_or_else(|| Error::Internal("Groq API key missing".into()))?;
            Ok(Arc::new(groq::Groq::new(key)?))
        }
        #[cfg(feature = "openrouter")]
        "openrouter" | "or" => {
            let key =
                api_key.ok_or_else(|| Error::Internal("OpenRouter API key missing".into()))?;
            Ok(Arc::new(openai::OpenAI::with_base_url(
                key,
                "https://openrouter.ai/api/v1",
            )?))
        }
        // Universal Native Provider (Bridges InferenceFactory to Provider)
        "native" | "local" | "internal" | "candle" | "gguf" => {
            let engine = kv_engine
                .ok_or_else(|| Error::Internal("KvEngine required for native provider".into()))?;
            let model_path = api_key.unwrap_or_else(|| "none".to_string());

            // Phase 21.6: Use InferenceFactory with direct path support
            let path = std::path::PathBuf::from(&model_path);
            let backend = benshu_inference::backend::InferenceFactory::create_backend(&path, None)
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;

            Ok(Arc::new(native::NativeProvider::new(backend, engine)))
        }
        "mock" => Ok(Arc::new(mock::MockProvider::new(
            "I am a mock provider".to_string(),
        ))),
        _ => Err(Error::Internal(format!(
            "Unknown or disabled provider: {}",
            name
        ))),
    }
}

/// Bridge factory for Providers (Phase 22)
pub struct GenericProviderFactory {
    /// Provider ID (e.g. openai)
    pub name: String,
    /// Target capability
    pub capability: benshu_inference::backend::BackendCapability,
}

#[async_trait::async_trait]
impl benshu_inference::backend::BackendFactory for GenericProviderFactory {
    fn capability(&self) -> benshu_inference::backend::BackendCapability {
        self.capability
    }

    fn can_handle(&self, path: &std::path::Path) -> bool {
        let p = path.to_string_lossy();
        p.starts_with(&format!("api:{}", self.name))
    }

    fn estimate_usage(&self, _path: &std::path::Path) -> (u64, u64) {
        (0, 0)
    }

    async fn create(
        &self,
        path: &std::path::Path,
        _: Option<&std::path::Path>,
    ) -> benshu_inference::backend::Result<Box<dyn std::any::Any + Send + Sync>> {
        let path_str = path.to_string_lossy();

        // For OpenAI and compatible APIs, we use our specialized provider impl
        // supporting multiple capabilities beyond pure text.
        if self.name == "openai" {
            let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
                benshu_inference::backend::InferenceError::LoadFailed(
                    "OPENAI_API_KEY not set".into(),
                )
            })?;
            let provider = if path_str.contains("api.openai.com") || !path_str.contains("http") {
                openai::OpenAI::new(key)
            } else {
                openai::OpenAI::with_base_url(key, path_str.clone())
            }
            .map_err(|e| benshu_inference::backend::InferenceError::LoadFailed(e.to_string()))?;
            let provider_arc = Arc::new(provider);

            match self.capability {
                benshu_inference::backend::BackendCapability::LLM => {
                    let b: Arc<dyn benshu_inference::backend::ModelBackend> = provider_arc;
                    return Ok(Box::new(b));
                }
                benshu_inference::backend::BackendCapability::Embedding => {
                    let b: Arc<dyn benshu_inference::backend::EmbeddingBackend> = provider_arc;
                    return Ok(Box::new(b));
                }
                benshu_inference::backend::BackendCapability::OCR => {
                    let b: Arc<dyn benshu_inference::backend::OcrBackend> = provider_arc;
                    return Ok(Box::new(b));
                }
                benshu_inference::backend::BackendCapability::ImageGeneration => {
                    let b: Arc<dyn benshu_inference::backend::ImageGenBackend> = provider_arc;
                    return Ok(Box::new(b));
                }
                _ => {} // Fallback to generic below if capability not matched
            }
        }

        // Default generic cloud backend for other providers
        let backend: Arc<dyn benshu_inference::backend::ModelBackend> = Arc::new(
            benshu_inference::backend::cloud::CloudBackend::from_path(&path_str)?,
        );
        Ok(Box::new(backend))
    }
}

/// Initialize and register all providers into the inference engine (Phase 21.4 Evolution)
/// This bridges the gap without circular compile-time dependencies.
pub fn init_backends() {
    use benshu_inference::backend::BackendCapability::*;
    use benshu_inference::backend::REGISTRY;

    // 0. Initialize standard local factories (Candle, Whisper, Piper)
    REGISTRY.init_standard();

    // 1. Register specialized Cloud Provider Factories for major capabilities
    let providers = ["openai", "gemini", "anthropic", "deepseek"];
    let capabilities = [LLM, Vision, OCR, Embedding, STT, TTS, ImageGeneration];

    for p in providers {
        for &cap in &capabilities {
            REGISTRY.register(
                &format!("{}-{:?}", p, cap),
                Arc::new(GenericProviderFactory {
                    name: p.to_string(),
                    capability: cap,
                }),
            );
        }
    }

    tracing::info!("🔌 [Providers Bridge] Switched to Phase 22 Trait-based Factories for all Cloud AI Capabilities.");
}
