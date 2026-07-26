//! Test provider implementations (without needing API keys)
//!
//! This tests that all providers can be instantiated correctly.
//!
//! Run with: cargo test --package providers --all-features

#[cfg(test)]
mod provider_tests {
    use crate::Provider;

    #[test]
    #[cfg(feature = "openai")]
    fn test_openai_creation() {
        use crate::openai::OpenAI;
        let provider = OpenAI::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "openai");
    }

    #[test]
    #[cfg(feature = "anthropic")]
    fn test_anthropic_creation() {
        use crate::anthropic::Anthropic;
        let provider = Anthropic::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "anthropic");
    }

    #[test]
    #[cfg(feature = "gemini")]
    fn test_gemini_creation() {
        use crate::gemini::Gemini;
        let provider = Gemini::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "gemini");
    }

    #[test]
    #[cfg(feature = "deepseek")]
    fn test_deepseek_creation() {
        use crate::deepseek::DeepSeek;
        let provider = DeepSeek::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "deepseek");
    }

    #[test]
    #[cfg(feature = "moonshot")]
    fn test_moonshot_creation() {
        use crate::moonshot::Moonshot;
        let provider = Moonshot::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "moonshot");
    }

    #[test]
    #[cfg(feature = "zhipu")]
    fn test_zhipu_creation() {
        use crate::zhipu::Zhipu;
        let provider = Zhipu::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "zhipu");
    }

    #[test]
    #[cfg(feature = "qwen")]
    fn test_qwen_creation() {
        use crate::qwen::Qwen;
        let provider = Qwen::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "qwen");
    }

    #[test]
    #[cfg(feature = "doubao")]
    fn test_doubao_creation() {
        use crate::doubao::Doubao;
        let provider = Doubao::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "doubao");
    }

    #[test]
    #[cfg(feature = "openrouter")]
    fn test_openrouter_creation() {
        use crate::openrouter::OpenRouter;
        let provider = OpenRouter::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "openrouter");
    }

    #[test]
    #[cfg(feature = "groq")]
    fn test_groq_creation() {
        use crate::groq::Groq;
        let provider = Groq::new("test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.name(), "groq");
    }

    #[test]
    #[cfg(all(
        feature = "openai",
        feature = "groq",
        feature = "zhipu",
        feature = "qwen",
        feature = "doubao"
    ))]
    fn test_all_providers_unique_names() {
        use crate::doubao::Doubao;
        use crate::groq::Groq;
        use crate::openai::OpenAI;
        use crate::qwen::Qwen;
        use crate::zhipu::Zhipu;

        let openai = OpenAI::new("test").unwrap();
        let groq = Groq::new("test").unwrap();
        let zhipu = Zhipu::new("test").unwrap();
        let qwen = Qwen::new("test").unwrap();
        let doubao = Doubao::new("test").unwrap();

        // Ensure all providers have unique names
        assert_ne!(openai.name(), groq.name());
        assert_ne!(openai.name(), zhipu.name());
        assert_ne!(openai.name(), qwen.name());
        assert_ne!(openai.name(), doubao.name());
        assert_ne!(zhipu.name(), qwen.name());
    }
}
