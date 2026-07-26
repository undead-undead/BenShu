use crate::agent::context::ContextInjector;
use crate::agent::message::{Message, Role};
use crate::agent::protocol::{ChatOutcome, MetabolicStats};
use crate::agent::provider::{ChatRequest, Provider};
use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::error;

/// Phase 15: Metabolic Signature for Performance Arbitrage
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetabolicSignature {
    pub avg_latency_ms: u64,
    pub max_vram_pressure: f32,
    pub max_cpu_pressure: f32,
    pub token_usage: Option<crate::agent::protocol::TokenUsage>,
}

/// A structured record of a successful task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceEntry {
    pub problem_description: String,
    pub successful_path: Vec<String>,
    pub key_parameters: Vec<String>,
    pub anti_patterns: Vec<String>, // Explicitly track anti-patterns
    pub lessons_learned: Vec<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Phase 15: The resource cost of this experience
    #[serde(default)]
    pub metabolic_signature: Option<MetabolicSignature>,
}

pub struct MinerOutcome {
    pub experience: Option<ExperienceEntry>,
    pub anti_patterns: Vec<AntiPatternUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiPatternUpdate {
    pub fingerprint: String,
    pub cause: String,
    pub fix: String,
}

pub struct ExperienceMiner {
    provider: Arc<dyn Provider>,
    model: String,
}

impl ExperienceMiner {
    pub fn new(provider: Arc<dyn Provider>, model: String) -> Self {
        Self { provider, model }
    }

    fn auxiliary_session_id(outcome: &ChatOutcome, scope: &str) -> Option<String> {
        outcome
            .runtime_task
            .as_ref()
            .and_then(|task| task.session_id.clone())
            .or_else(|| outcome.ownership.session_id.clone())
            .map(|session_id| format!("{}::{}", session_id, scope.trim()))
    }

    /// Distill a successful interaction history into a structured ExperienceEntry.
    pub async fn distill(
        &self,
        history: &[Message],
        outcome: &ChatOutcome,
    ) -> Result<ExperienceEntry> {
        let mut transcript = String::new();
        for m in history.iter().take(20) {
            // Take a good slice of history
            transcript.push_str(&format!("{:?}: {}\n", m.role, m.content.as_text()));
        }

        let prompt = format!(
            "### EXPERIENCE EXTRACTION ENGINE\n\
             Review the following successful task execution and extract the 'Core Logic'.\n\n\
             INTERACTION TRANSCRIPT:\n{}\n\n\
             FINAL OUTCOME: {}\n\n\
             INSTRUCTIONS:\n\
             1. Identify the 'Problem Pattern' (What was the user's specific pain point?).\n\
             2. Extract the 'Golden Path' (The sequence of tool calls/actions that led to success).\n\
             3. List 'Crucial Parameters' (Paths, Flags, or Config values that were mission-critical).\n\
             4. Extract 'Anti-Patterns' (What specifically failed or should be avoided in similar cases?).\n\n\
             OUTPUT FORMAT: Respond with a JSON object matching this schema:\n\
             {{
               \"problem_description\": \"...\",
               \"successful_path\": [\"step 1\", \"step 2\"],
               \"key_parameters\": [\"param=val\"],
               \"lessons_learned\": [\"...\"],
               \"anti_patterns\": [\"... error X happens if you do Y ...\"]
             }}",
            transcript, outcome.response
        );

        let request = ChatRequest {
            model: self.model.clone(),
            system_prompt: Some("You are the BenShu Experience Miner. You condense complex successes into reusable logic.".to_string()),
            messages: vec![Message::user(prompt)],
            temperature: Some(0.2),
            session_id: Self::auxiliary_session_id(outcome, "experience_miner"),
            ..Default::default()
        };

        let stream = self.provider.stream_completion(request).await?;
        let full_text = stream.collect_text().await?;

        // Extract JSON with robust boundaries
        let json_start = full_text.find('{').ok_or_else(|| {
            crate::error::Error::Internal(
                "No JSON object found in experience miner response".to_string(),
            )
        })?;
        let json_end = full_text.rfind('}').ok_or_else(|| {
            crate::error::Error::Internal(
                "Unclosed JSON object in experience miner response".to_string(),
            )
        })?;
        let json_str = &full_text[json_start..=json_end];

        // Phase 14: Parse with fallback to avoid breaking the evolution loop
        let mut entry: ExperienceEntry = match serde_json::from_str(json_str) {
            Ok(e) => e,
            Err(e) => {
                error!("Failed to parse experience JSON: {} (Raw: {})", e, json_str);
                ExperienceEntry {
                    problem_description: "Failed to extract - fallback".to_string(),
                    successful_path: Vec::new(),
                    key_parameters: Vec::new(),
                    anti_patterns: Vec::new(),
                    lessons_learned: Vec::new(),
                    timestamp: chrono::Utc::now(),
                    metabolic_signature: None,
                }
            }
        };

        entry.timestamp = chrono::Utc::now();

        // Phase 15: Populate Metabolic Signature from Outcome
        if let Some(stats) = &outcome.metabolic_stats {
            let mut total_duration = 0;
            let mut max_vram = stats.vram_pressure;
            let mut max_cpu = stats.cpu_usage;

            for call in &outcome.tool_calls {
                total_duration += call.duration_ms;
                if let Some(v) = call.vram_pressure {
                    if v > max_vram {
                        max_vram = v;
                    }
                }
                if let Some(c) = call.cpu_pressure {
                    if c > max_cpu {
                        max_cpu = c;
                    }
                }
            }

            let avg_latency = if !outcome.tool_calls.is_empty() {
                total_duration / outcome.tool_calls.len() as u64
            } else {
                0
            };

            entry.metabolic_signature = Some(MetabolicSignature {
                avg_latency_ms: avg_latency,
                max_vram_pressure: max_vram,
                max_cpu_pressure: max_cpu,
                token_usage: stats.token_usage.clone(),
            });
        }

        Ok(entry)
    }
}

/// Phase 14: Cognitive Guidance Injector (Experience RAG)
/// Injects relevant "Golden Paths" and "Anti-Patterns" into the context.
pub struct CognitiveGuidanceInjector {
    memory: Arc<dyn crate::agent::memory::Memory>,
    limit: usize,
}

impl CognitiveGuidanceInjector {
    pub fn new(memory: Arc<dyn crate::agent::memory::Memory>) -> Self {
        Self { memory, limit: 3 }
    }
}

#[async_trait]
impl ContextInjector for CognitiveGuidanceInjector {
    async fn inject(&self, history: &[Message]) -> Result<Vec<Message>> {
        // Only run if there's a user query to search against
        let last_user_query = history
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.as_text())
            .unwrap_or_default();

        if last_user_query.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Search for intuitive guidance (Phase 14: Safety-First)
        let mut experiences = self
            .memory
            .search_experiences(&last_user_query, 10)
            .await
            .unwrap_or_default();
        let mut anti_patterns = self
            .memory
            .search_anti_patterns(&last_user_query, 10)
            .await
            .unwrap_or_default();

        // Phase 14.3: Conflict Resolution & Prioritization Logic
        // 1. Sort by Utility Score (Primary) and Recency (Secondary)
        let sort_by_quality = |a: &serde_json::Value, b: &serde_json::Value| {
            let utility_a = a
                .get("utility_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let utility_b = b
                .get("utility_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            if (utility_a - utility_b).abs() > 0.001 {
                utility_b
                    .partial_cmp(&utility_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            } else {
                // Secondary: Recency
                let ts_a = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
                let ts_b = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
                ts_b.cmp(ts_a)
            }
        };

        experiences.sort_by(sort_by_quality);
        anti_patterns.sort_by(sort_by_quality);

        // 2. Semantic Deduplication (Keep only the best version of similar problems)
        let mut final_experiences = Vec::new();
        let mut seen_descriptions = HashSet::new();

        for exp in experiences {
            let desc = exp
                .get("problem_description")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let id = exp.get("id").and_then(|v| v.as_str()).unwrap_or_default();

            // Basic deduplication: if we have a very similar description, skip it if it's lower quality (already sorted)
            // For now, exact match on description or ID. In future, use embedding similarity.
            if !desc.is_empty() && !seen_descriptions.contains(desc) {
                seen_descriptions.insert(desc.to_string());
                final_experiences.push(exp);
            } else if desc.is_empty() && !id.is_empty() && !seen_descriptions.contains(id) {
                seen_descriptions.insert(id.to_string());
                final_experiences.push(exp);
            }

            if final_experiences.len() >= self.limit {
                break;
            }
        }

        let mut final_anti_patterns = Vec::new();
        let mut seen_aps = HashSet::new();
        for ap in anti_patterns {
            let cause = ap
                .get("cause")
                .and_then(|v| v.as_str())
                .or_else(|| ap.get("root_cause").and_then(|v| v.as_str()))
                .unwrap_or_default();

            if !cause.is_empty() && !seen_aps.contains(cause) {
                seen_aps.insert(cause.to_string());
                final_anti_patterns.push(ap);
            }
            if final_anti_patterns.len() >= self.limit {
                break;
            }
        }

        if final_experiences.is_empty() && final_anti_patterns.is_empty() {
            return Ok(Vec::new());
        }

        let mut guidance = String::from("### COGNITIVE GUIDANCE (INTUITION)\n");
        guidance.push_str(
            "Based on past high-utility experiences, here are relevant patterns for this task:\n\n",
        );

        if !final_experiences.is_empty() {
            guidance.push_str("#### PROVEN GOLDEN PATHS:\n");
            for exp in &final_experiences {
                if let (Some(desc), Some(path)) = (
                    exp.get("problem_description").and_then(|v| v.as_str()),
                    exp.get("successful_path").and_then(|v| v.as_array()),
                ) {
                    let steps: Vec<String> = path
                        .iter()
                        .filter_map(|s| s.as_str().map(|v| v.to_string()))
                        .collect();
                    let utility = exp
                        .get("utility_score")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let mut meta_str = format!("(Utility: {:.1})", utility);

                    if let Some(sig) = exp.get("metabolic_signature") {
                        let latency = sig
                            .get("avg_latency_ms")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let vram = sig
                            .get("max_vram_pressure")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        if latency > 0 || vram > 0.0 {
                            meta_str = format!(
                                "(Utility: {:.1}, Latency: {}ms, VRAM: {:.0}%)",
                                utility, latency, vram
                            );
                        }
                    }

                    guidance.push_str(&format!(
                        "- Task: {} {}\n  Steps: {}\n",
                        desc,
                        meta_str,
                        steps.join(" -> ")
                    ));
                }
            }
            guidance.push_str("\n");
        }

        if !final_anti_patterns.is_empty() {
            guidance.push_str("#### CRITICAL FAILURES TO AVOID:\n");
            for ap in &final_anti_patterns {
                let cause = ap
                    .get("cause")
                    .and_then(|v| v.as_str())
                    .or_else(|| ap.get("root_cause").and_then(|v| v.as_str()))
                    .unwrap_or("Unknown cause");
                let fix = ap
                    .get("fix")
                    .and_then(|v| v.as_str())
                    .or_else(|| ap.get("correction").and_then(|v| v.as_str()))
                    .unwrap_or("No correction available");

                guidance.push_str(&format!("- ISSUE: {}\n  FIX: {}\n", cause, fix));
            }
        }

        // Phase 14: Context Guard - Truncate if too long
        const MAX_GUIDANCE_LENGTH: usize = 3000;
        if guidance.len() > MAX_GUIDANCE_LENGTH {
            guidance.truncate(MAX_GUIDANCE_LENGTH);
            guidance.push_str("\n\n[Truncated: Guidance too long]");
        }

        let mut msg = Message::system(guidance);

        // Track used IDs for utility scoring in EvolutionManager
        msg.used_experience_ids = final_experiences
            .iter()
            .filter_map(|e| e.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();

        msg.used_anti_pattern_ids = final_anti_patterns
            .iter()
            .filter_map(|a| a.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();

        Ok(vec![msg])
    }
}
