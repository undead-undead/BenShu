use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub confidence: f32,
    pub detected_hallucinations: Vec<String>,
    pub source_verification: Vec<SourceMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMatch {
    pub source_snippet: String,
    pub alignment_score: f32,
}

#[async_trait]
pub trait FactChecker: Send + Sync {
    /// Verify if the output text is factually consistent with provided context
    async fn verify(&self, text: &str, context: &str) -> ValidationResult;

    /// Check for internal consistency (no contradictions)
    async fn check_consistency(&self, text: &str) -> f32;
}
