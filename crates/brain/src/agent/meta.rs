use crate::agent::message::Message;
use crate::agent::provider::{ChatRequest, Provider};
use crate::error::Result;
use async_trait::async_trait;
use benshu_hardness::sanitize_task_complexity;
pub use benshu_hardness::TaskComplexity;
use std::sync::Arc;
use tracing::{debug, warn};

/// Trait for task complexity estimation
#[async_trait]
pub trait ComplexityEstimator: Send + Sync {
    /// Estimate the complexity of a task before execution
    async fn estimate(
        &self,
        prompt: &str,
        available_tools: &[crate::skills::tool::ToolDefinition],
    ) -> Result<TaskComplexity>;
}

/// A default complexity estimator that uses a fast LLM call
pub struct LlmComplexityEstimator<P: Provider + ?Sized> {
    provider: Arc<P>,
    model: String,
    session_root: Option<String>,
}

impl<P: Provider + ?Sized> LlmComplexityEstimator<P> {
    pub fn new(provider: Arc<P>, model: String) -> Self {
        Self {
            provider,
            model,
            session_root: None,
        }
    }

    pub fn with_session_root(mut self, session_root: impl Into<String>) -> Self {
        self.session_root = Some(session_root.into());
        self
    }

    fn complexity_session_id(&self) -> Option<String> {
        self.session_root
            .as_ref()
            .map(|root| format!("{}::complexity", root))
    }

    /// Optimized JSON extraction that handles markdown blocks
    fn extract_json(text: &str) -> &str {
        // Try to find markdown json block first
        if let Some(block) = text.split("```json").nth(1) {
            if let Some(end) = block.find("```") {
                return block[..end].trim();
            }
        }

        // Fallback to finding braces
        let start = text.find('{').unwrap_or(0);
        let end = text.rfind('}').map(|e| e + 1).unwrap_or(text.len());
        text[start..end].trim()
    }

    /// Rule-based fallback if LLM fails
    fn fallback_estimate(
        prompt: &str,
        tools: &[crate::skills::tool::ToolDefinition],
    ) -> TaskComplexity {
        let mut risk_score = 0.1;
        let mut estimated_steps = 2;

        // Keyword heuristic
        let dangerous = [
            "delete", "remove", "kill", "format", "write", "update", "send", "pay", "trade",
        ];
        let prompt_lower = prompt.to_lowercase();

        if dangerous.iter().any(|&w| prompt_lower.contains(w)) {
            risk_score = 0.7;
            estimated_steps = 4;
        }

        // Tool count heuristic
        if tools.len() > 20 {
            estimated_steps += 2;
        }

        TaskComplexity {
            estimated_steps,
            risk_score,
            rationale: "LLM estimation failed or timed out. Using safety fallback heuristics."
                .to_string(),
            max_steps_override: Some(estimated_steps * 2),
            intent: "general_query".to_string(),
        }
    }
}

#[async_trait]
impl<P: Provider + ?Sized> ComplexityEstimator for LlmComplexityEstimator<P> {
    async fn estimate(
        &self,
        prompt: &str,
        available_tools: &[crate::skills::tool::ToolDefinition],
    ) -> Result<TaskComplexity> {
        let tools_desc = available_tools
            .iter()
            .map(|t| format!("- {}: {}", t.name, t.description))
            .collect::<Vec<_>>()
            .join("\n");

        let system_prompt = format!(
            "You are a meta-cognitive analyzer for AI agents. Analyze the given task and available tools.\n\n\
            Available Tools:\n{}\n\n\
            Rules for Estimation:\n\
            1. estimated_steps: Predicted count of tool calls (1-20).\n\
            2. risk_score: 0.0 (read-only) to 1.0 (dangerous system changes/deletions).\n\
            3. intent: 1-2 word category label (e.g., 'coding', 'market_research', 'system_config').\n\
            4. rationale: 1-sentence justification.\n\
            5. max_steps_override: suggest a hard limit if the task could spiral.\n\n\
            Respond ONLY with valid JSON:",
            tools_desc
        );

        let request = ChatRequest {
            model: self.model.clone(),
            system_prompt: Some(system_prompt),
            messages: vec![Message::user(prompt)],
            temperature: Some(0.0),
            session_id: self.complexity_session_id(),
            ..Default::default()
        };

        let response_res = self.provider.stream_completion(request).await;

        if let Err(e) = response_res {
            warn!(
                "Complexity estimation failed (LLM error): {}. Using fallback.",
                e
            );
            return Ok(Self::fallback_estimate(prompt, available_tools));
        }

        let response = response_res.unwrap();
        let full_text = response.collect_text().await?;
        let json_str = Self::extract_json(&full_text);

        let mut complexity: TaskComplexity =
            match serde_json::from_str::<serde_json::Value>(json_str) {
                Ok(v) => TaskComplexity {
                    estimated_steps: v["estimated_steps"].as_u64().unwrap_or(2) as usize,
                    risk_score: v["risk_score"].as_f64().unwrap_or(0.1) as f32,
                    intent: v["intent"].as_str().unwrap_or("general").to_string(),
                    rationale: v["rationale"].as_str().unwrap_or_default().to_string(),
                    max_steps_override: v["max_steps_override"].as_u64().map(|n| n as usize),
                },
                Err(e) => {
                    warn!(
                        "Complexity estimation failed (JSON error: {}). Body: {}. Using fallback.",
                        e, full_text
                    );
                    return Ok(Self::fallback_estimate(prompt, available_tools));
                }
            };

        let complexity = sanitize_task_complexity(complexity);

        debug!(score = %complexity.risk_score, steps = %complexity.estimated_steps, intent = %complexity.intent, "Task complexity estimated");
        Ok(complexity)
    }
}
