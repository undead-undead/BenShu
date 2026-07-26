use crate::intent::{IntentAnalysis, RetrievalIntent};
use async_trait::async_trait;
use benshu_infra::error::Result;
use tokio::time::{timeout, Duration};
use tracing::{info, instrument, warn};

#[async_trait]
pub trait IntentAnalysisAgent: Send + Sync {
    async fn process(&self, prompt: &str) -> Result<String>;
}

/// Routes user queries to specific virtual paths based on semantic intent.
/// Hardened with timeout control, sanitation, and confidence thresholds.
pub struct IntentRouter {
    pub confidence_threshold: f32,
    pub llm_timeout_ms: u64,
}

// Internal metadata to keep the module cohesive
const INTENT_META: [(RetrievalIntent, &str, &str, &str); 5] = [
    (
        RetrievalIntent::Skill,
        "skill",
        "Use a tool or check capabilities",
        "benshu://skills/",
    ),
    (
        RetrievalIntent::Memory,
        "memory",
        "Past interactions, facts, or long-term memory",
        "benshu://memory/",
    ),
    (
        RetrievalIntent::Code,
        "code",
        "Source code, implementation, or technical details",
        "benshu://codebase/",
    ),
    (
        RetrievalIntent::System,
        "system",
        "Configuration, status, or system health",
        "benshu://system/",
    ),
    (
        RetrievalIntent::Chat,
        "chat",
        "Casual conversation, no retrieval needed",
        "",
    ),
];

impl Default for IntentRouter {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.7,
            llm_timeout_ms: 5000,
        }
    }
}

impl IntentRouter {
    pub fn new() -> Self {
        Self::default()
    }

    #[instrument(skip(self, agent), fields(query = %query))]
    pub async fn analyze(
        &self,
        agent: &dyn IntentAnalysisAgent,
        query: &str,
    ) -> Result<IntentAnalysis> {
        // 1. Construct prompt using cohesive metadata
        let intent_descriptions = INTENT_META
            .iter()
            .map(|(_, name, desc, path)| format!("- \"{}\": {}. Target: {}", name, desc, path))
            .collect::<Vec<_>>()
            .join("\n");

        // Protect against injection
        let sanitized_query = query
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', " ");

        let prompt = format!(
            r#"You are the Intent Classifier for the BenShu Knowledge Base.
Route the user's query to the correct virtual directory.

Query: "{}"

Available Intents:
{}

Output JSON Requirement:
Return ONLY valid JSON with keys: "primary_intent", "target_paths", "keywords", "confidence", "sentiment", "urgency".
Return ONLY JSON."#,
            sanitized_query.trim(),
            intent_descriptions
        );

        // 2. Transact with LLM providing safe timeout
        let llm_response = match timeout(
            Duration::from_millis(self.llm_timeout_ms),
            agent.process(&prompt),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                warn!("LLM intent analysis error: {}", e);
                return Ok(IntentAnalysis::global(query));
            }
            Err(_) => {
                warn!("LLM intent analysis timed out ({}ms)", self.llm_timeout_ms);
                return Ok(IntentAnalysis::global(query));
            }
        };

        // 3. Robust JSON Extraction (Byte-safe implementation)
        let cleaned_json = match extract_json_braces(&llm_response) {
            Some(json) => json,
            None => {
                warn!("No valid JSON found in response: {}", llm_response);
                return Ok(IntentAnalysis::global(query));
            }
        };

        // 4. Parse and Validate
        match serde_json::from_str::<IntentAnalysis>(cleaned_json) {
            Ok(mut analysis) => {
                // Confidence Gating
                if analysis.confidence < self.confidence_threshold {
                    info!(
                        "Intent confidence too low: {}. Falling back.",
                        analysis.confidence
                    );
                    return Ok(IntentAnalysis::global(query));
                }

                // Metadata Sync/Correction (Ensures target_paths always match intent)
                if let Some((_, _, _, target_path)) = INTENT_META
                    .iter()
                    .find(|(intent, _, _, _)| *intent == analysis.primary_intent)
                {
                    if target_path.is_empty() {
                        analysis.target_paths.clear();
                    } else {
                        analysis.target_paths = vec![target_path.to_string()];
                    }
                }

                analysis.confidence = analysis.confidence.clamp(0.0, 1.0);
                analysis.urgency = analysis.urgency.clamp(0.0, 1.0);

                Ok(analysis)
            }
            Err(e) => {
                warn!("Intent JSON parse error: {}. data: {}", e, cleaned_json);
                Ok(IntentAnalysis::global(query))
            }
        }
    }
}

/// Extracts a JSON object from text using byte-safe brace matching.
/// Handles multi-line and nested JSON blobs accurately.
fn extract_json_braces(input: &str) -> Option<&str> {
    let mut start_idx = None;
    let mut brace_count = 0;

    for (i, c) in input.char_indices() {
        if c == '{' {
            if start_idx.is_none() {
                start_idx = Some(i);
            }
            brace_count += 1;
        } else if c == '}' && start_idx.is_some() {
            brace_count -= 1;
            if brace_count == 0 {
                return Some(&input[start_idx?..=i]);
            }
        }
    }
    None
}
