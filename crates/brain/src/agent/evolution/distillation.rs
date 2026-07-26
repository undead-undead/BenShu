use crate::agent::memory::{Fact, Memory};
use crate::agent::message::Message;
use crate::agent::provider::{ChatRequest, Provider};
use crate::agent::session::AgentSession;
use crate::error::Result;
use std::sync::Arc;

/// Automated memory distillation: Extracts facts from conversation logs.
pub struct MemoryDistiller {
    memory: Arc<dyn Memory>,
    provider: Arc<dyn Provider>,
    model: String,
}

impl MemoryDistiller {
    pub fn new(memory: Arc<dyn Memory>, provider: Arc<dyn Provider>, model: String) -> Self {
        Self {
            memory,
            provider,
            model,
        }
    }

    fn session_scoped_child_id(session_id: &str, scope: &str) -> String {
        format!("{}::{}", session_id, scope.trim())
    }

    fn jit_summary_session_id(session_root: Option<&str>) -> Option<String> {
        session_root
            .map(str::trim)
            .filter(|session_id| !session_id.is_empty())
            .map(|session_id| Self::session_scoped_child_id(session_id, "jit_distill"))
    }

    /// Run distillation on multiple sessions
    pub async fn run(&self) -> Result<usize> {
        let sessions = self.memory.list_sessions().await?;
        let mut count = 0;

        for mut session in sessions {
            // Only distill inactive, non-distilled sessions with history
            if !session.is_distilled && session.messages.len() >= 4 {
                tracing::info!("Distilling session: {}", session.id);

                match self.distill_session(&session).await {
                    Ok(facts) => {
                        for fact in facts {
                            let importance = fact.importance;
                            let content = fact.content.clone();
                            let category = fact.category.clone();

                            // 1. Store as Fact (Mid-term)
                            let _ = self.memory.store_fact("default", None, fact).await;

                            // 2. Phase 15: Automatic Promotion Rule
                            // If vitality (importance) >= 0.8, promote to permanent LTM Knowledge
                            if importance >= 0.8 {
                                tracing::info!(
                                    "Promoting high-importance fact to LTM: {}",
                                    content
                                );
                                let _ = self
                                    .memory
                                    .store_knowledge(
                                        "default",
                                        None,
                                        &format!("Promoted: {}", category),
                                        &content,
                                        &category,
                                        false, // verified = true (not unverified)
                                    )
                                    .await;
                            }
                        }

                        // Mark as distilled to prevent re-distillation
                        session.is_distilled = true;
                        let _ = self.memory.store_session(session).await;
                        count += 1;
                    }
                    Err(e) => {
                        tracing::error!("Failed to distill session {}: {}", session.id, e);
                    }
                }
            }
        }

        Ok(count)
    }

    /// Extract facts from a single session using LLM
    async fn distill_session(&self, session: &AgentSession) -> Result<Vec<Fact>> {
        let mut history = String::new();
        for msg in &session.messages {
            history.push_str(&format!("{:?}: {}\n", msg.role, msg.text()));
        }

        let system_prompt = concat!(
            "### MEMORY DISTILLATION MISSION (Knowledge Graph Extraction) / 记忆洞察与提炼使命\n\n",
            "You are a Senior Insight Analyst for BenShu. Analyze chat logs to extract 'Atomic Facts' and 'Relations'.\n",
            "你正在执行记忆提炼任务。分析对话记录，提取“原子事实”及其“关联关系”。\n\n",
            "### EXTRACTION RULES / 提取规则:\n",
            "1. Atomic Facts: Extract concise, standalone truths (Preferences, Identity, Knowledge).\n",
            "   原子事实：提取简练、独立的真相（偏好、身份、知识）。\n",
            "2. Relations: Identify how facts relate (e.g., 'User' -[prefers]-> 'Dark Mode').\n",
            "   关联关系：识别事实间的逻辑链条（如：“用户”-[偏好]->“深色模式”）。\n",
            "3. Vitality: Score importance from 0.0 to 1.0 (High vitality = long-term persistence).\n",
            "   生命力评分：从0.0到1.0为重要性打分（高分将进入长期记忆）。\n\n",
            "RESPONSE FORMAT: Respond ONLY with a JSON array of Fact objects:\n",
            "必须仅以 JSON 数组形式响应：\n",
            "[{\n",
            "  \"content\": \"...\",\n",
            "  \"category\": \"preference|identity|knowledge|constraint\",\n",
            "  \"importance\": 0.8,\n",
            "  \"confidence\": 0.9,\n",
            "  \"relations\": [{\"predicate\": \"likes|works_at|member_of\", \"target_content\": \"...\"}]\n",
            "}]"
        );

        let request = ChatRequest {
            model: self.model.clone(),
            system_prompt: Some(system_prompt.to_string()),
            messages: vec![Message::user(format!(
                "### CONVERSATION LOG ###\n\n{}\n\n### EXTRACT KNOWLEDGE GRAPH ###",
                history
            ))],
            max_tokens: Some(1500),
            temperature: Some(0.1),
            session_id: Some(Self::session_scoped_child_id(&session.id, "memory_distill")),
            ..Default::default()
        };

        let stream = self.provider.stream_completion(request).await?;
        let full_text = stream
            .collect_text()
            .await
            .map_err(|e| crate::error::Error::Internal(e.to_string()))?;

        // Extract JSON block
        let json_start = full_text.find('[');
        let json_end = full_text.rfind(']');

        if let (Some(start), Some(end)) = (json_start, json_end) {
            let json_str = &full_text[start..=end];
            let raw_facts: Vec<RawFact> = serde_json::from_str(json_str).map_err(|e| {
                crate::error::Error::Internal(format!("Malformed distillation JSON: {}", e))
            })?;

            let facts = raw_facts
                .into_iter()
                .map(|rf| {
                    let mut f = Fact::new(rf.content, rf.category);
                    f.importance = rf.importance;
                    f.confidence = rf.confidence;
                    f.source = Some(session.id.clone());

                    // Note: Real relation mapping would need target_id resolving,
                    // but for initial distillation we store them as pending relations in metadata or similar.
                    // For now, we store them in the relations vector as placeholder targets.
                    for rel in rf.relations {
                        f.relations.push(crate::agent::memory::Relation {
                            predicate: rel.predicate,
                            target_id: rel.target_content, // Temporary: Store content as ID for later resolution
                            strength: 0.8,
                        });
                    }
                    f
                })
                .collect();

            Ok(facts)
        } else {
            Ok(Vec::new())
        }
    }

    /// JIT Micro-Distillation: Summarize a conversation segment when a topic shift is detected.
    /// Implements the "Dual-Core" fallback: SLM -> Primary Provider -> Rules.
    pub async fn jit_summarize(
        &self,
        messages: &[Message],
        session_root: Option<&str>,
    ) -> Result<String> {
        if messages.is_empty() {
            return Ok(String::new());
        }

        let history_text: String = messages
            .iter()
            .map(|m| format!("{}: {}", m.role.as_str(), m.text()))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "### JIT EPISODE SUMMARY / 话题快速摘要\n\n\
            Summarize the following conversation segment concisely (max 50 words).\n\
            简要总结以下对话片段（不超过50字）。\n\n\
            CONVERSATION:\n{}\n\n\
            SUMMARY:",
            history_text
        );

        // Level 1: Try Local SLM (if configured via jit_distillation_model)
        // Note: For now we try the primary provider but with the specific small model if it differs.
        let request = ChatRequest {
            model: self.model.clone(), // This is our currently configured 'Brains' model or SLM
            messages: vec![Message::user(prompt)],
            max_tokens: Some(200),
            temperature: Some(0.3),
            session_id: Self::jit_summary_session_id(session_root),
            ..Default::default()
        };

        match self.provider.stream_completion(request).await {
            Ok(stream) => match stream.collect_text().await {
                Ok(summary) if !summary.trim().is_empty() => {
                    tracing::debug!("JIT Distillation successful via LLM: {}", summary);
                    Ok(summary.trim().to_string())
                }
                _ => {
                    tracing::warn!("JIT Distillation returned empty text, falling back to rules.");
                    Ok(self.rule_based_summary(messages))
                }
            },
            Err(e) => {
                tracing::warn!(
                    "JIT Distillation LLM call failed ({}). Falling back to rules.",
                    e
                );
                Ok(self.rule_based_summary(messages)) // Level 3 Fallback
            }
        }
    }

    /// Level 3 Implementation: Deterministic Rule-Based Summary (Polished for keywords + Entities)
    fn rule_based_summary(&self, messages: &[Message]) -> String {
        let first_msg = messages
            .first()
            .map(|m| m.text())
            .unwrap_or_else(|| "No start".to_string());
        let last_msg = messages
            .last()
            .map(|m| m.text())
            .unwrap_or_else(|| "No end".to_string());

        let mut summary = format!(
            "Topic segment: '{}...' to '{}...'.",
            first_msg.chars().take(30).collect::<String>(),
            last_msg.chars().take(30).collect::<String>()
        );

        // Dimension 1: Keywords (Nouns/Technical terms based on length and frequency)
        let mut word_counts = std::collections::HashMap::new();
        for msg in messages {
            for word in msg.text().split_whitespace() {
                let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
                if clean.len() > 4 {
                    *word_counts.entry(clean.to_lowercase()).or_insert(0) += 1;
                }
            }
        }
        let mut keywords: Vec<_> = word_counts.into_iter().collect();
        keywords.sort_by(|a, b| b.1.cmp(&a.1));
        let top_keywords: Vec<_> = keywords.into_iter().take(4).map(|(k, _)| k).collect();

        // Dimension 2: Entities (Proper nouns/Capitalized terms)
        let entities: Vec<String> = messages
            .iter()
            .flat_map(|m| {
                m.text()
                    .split_whitespace()
                    .map(|w| w.to_string())
                    .collect::<Vec<_>>()
            })
            .filter(|w| w.len() > 3 && w.chars().next().map_or(false, |c| c.is_uppercase()))
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .take(4)
            .collect();

        if !top_keywords.is_empty() {
            summary.push_str(&format!(" Keywords: {}.", top_keywords.join(", ")));
        }
        if !entities.is_empty() {
            summary.push_str(&format!(" Entities: {}.", entities.join(", ")));
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryDistiller;
    use crate::agent::memory::InMemoryMemory;
    use crate::agent::message::Message;
    use crate::agent::provider::{ChatRequest, Provider, ProviderMetadata};
    use crate::agent::streaming::{
        FinishReason, MockStreamBuilder, ProviderTelemetry, StreamingResponse,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    struct CaptureProvider {
        last_request: Arc<Mutex<Option<ChatRequest>>>,
    }

    impl CaptureProvider {
        fn new() -> Self {
            Self {
                last_request: Arc::new(Mutex::new(None)),
            }
        }
    }

    #[async_trait]
    impl Provider for CaptureProvider {
        fn metadata() -> ProviderMetadata {
            ProviderMetadata {
                id: "capture".to_string(),
                name: "Capture".to_string(),
                description: "capture".to_string(),
                icon: String::new(),
                fields: vec![],
                capabilities: vec![],
                preferred_models: vec![],
            }
        }

        async fn stream_completion(
            &self,
            request: ChatRequest,
        ) -> benshu_infra::error::Result<StreamingResponse> {
            *self.last_request.lock().await = Some(request);
            Ok(MockStreamBuilder::new()
                .message("summary ok")
                .finish(FinishReason::Stop)
                .telemetry(ProviderTelemetry {
                    provider_name: Some("capture".to_string()),
                    model: None,
                    latency_ms: Some(0),
                    continuation: None,
                    extra: std::collections::HashMap::new(),
                })
                .done()
                .build())
        }

        fn name(&self) -> &str {
            "capture"
        }
    }

    #[tokio::test]
    async fn jit_summarize_uses_stable_child_session_when_root_exists() {
        let provider = Arc::new(CaptureProvider::new());
        let distiller = MemoryDistiller::new(
            Arc::new(InMemoryMemory::new()),
            provider.clone(),
            "test-model".to_string(),
        );

        let _ = distiller
            .jit_summarize(&[Message::user("继续这个主线")], Some("session-123"))
            .await
            .unwrap();

        let request = provider.last_request.lock().await.clone().unwrap();
        assert_eq!(
            request.session_id.as_deref(),
            Some("session-123::jit_distill")
        );
    }

    #[tokio::test]
    async fn jit_summarize_stays_detached_without_root_session() {
        let provider = Arc::new(CaptureProvider::new());
        let distiller = MemoryDistiller::new(
            Arc::new(InMemoryMemory::new()),
            provider.clone(),
            "test-model".to_string(),
        );

        let _ = distiller
            .jit_summarize(&[Message::user("继续这个主线")], None)
            .await
            .unwrap();

        let request = provider.last_request.lock().await.clone().unwrap();
        assert_eq!(request.session_id, None);
    }
}

#[derive(serde::Deserialize)]
struct RawFact {
    content: String,
    category: String,
    importance: f32,
    confidence: f32,
    #[serde(default)]
    relations: Vec<RawRelation>,
}

#[derive(serde::Deserialize)]
struct RawRelation {
    predicate: String,
    target_content: String,
}
