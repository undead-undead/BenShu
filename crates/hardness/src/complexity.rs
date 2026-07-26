use async_trait::async_trait;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::System;
use tracing::{debug, info, warn};

use crate::model::{ComplexityScore, MediaKind, MessageSnapshot};

#[async_trait]
pub trait SemanticComplexityAnalyzer: Send + Sync {
    async fn analyze_complexity(&self, prompt: &str) -> Result<String, String>;
}

pub struct ComplexityEstimator {
    system: Arc<RwLock<System>>,
    cpu_refresh_interval: Duration,
    last_cpu_refresh: Arc<RwLock<Instant>>,
}

impl Default for ComplexityEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplexityEstimator {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();

        Self {
            system: Arc::new(RwLock::new(sys)),
            cpu_refresh_interval: Duration::from_millis(500),
            last_cpu_refresh: Arc::new(RwLock::new(Instant::now())),
        }
    }

    pub async fn estimate(
        &self,
        messages: &[MessageSnapshot],
        analyzer: Option<&dyn SemanticComplexityAnalyzer>,
    ) -> ComplexityScore {
        if messages.is_empty() {
            return ComplexityScore {
                score: 0.0,
                reason: "EMPTY_INPUT".to_string(),
                predicted_output_tokens: 0,
                is_parallelizable: false,
                level: 1,
                metadata: serde_json::Value::default(),
            };
        }

        let l1_score = self.estimate_level_1(messages);
        if l1_score.score < 0.35 || (l1_score.score > 0.85 && l1_score.level == 1) {
            debug!(
                "Complexity Level 1 sufficient: {:.2} ({})",
                l1_score.score, l1_score.reason
            );
            return l1_score;
        }

        if let Some(analyzer) = analyzer {
            match self.estimate_level_2(messages, analyzer).await {
                Ok(l2_score) => {
                    info!(
                        "Complexity Level 2 Analysis: {:.2} ({})",
                        l2_score.score, l2_score.reason
                    );
                    return l2_score;
                }
                Err(error) => {
                    warn!(
                        "Complexity Level 2 analysis failed: {}. Falling back to Level 1.",
                        error
                    );
                }
            }
        }

        l1_score
    }

    pub fn current_usage(&self) -> f32 {
        let mut last_refresh = self.last_cpu_refresh.write();

        if last_refresh.elapsed() >= self.cpu_refresh_interval {
            let mut sys = self.system.write();
            sys.refresh_cpu_all();
            *last_refresh = Instant::now();
        }

        let sys = self.system.read();
        let cpus = sys.cpus();
        if cpus.is_empty() {
            return 0.0;
        }
        cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpus.len() as f32
    }

    fn estimate_level_1(&self, messages: &[MessageSnapshot]) -> ComplexityScore {
        let mut score = 0.0;
        let mut reasons: Vec<String> = Vec::new();
        let mut is_parallel = false;
        let mut metadata_map = serde_json::Map::new();

        let last_msg = messages.last().expect("non-empty checked above");
        let content = last_msg.text.to_lowercase();

        let parallel_keywords = [
            "同时",
            "并行",
            "分别",
            "批量",
            "parallel",
            "simultaneously",
            "batch",
            "separately",
        ];
        let complex_keywords = [
            "分析",
            "设计",
            "重构",
            "实现",
            "优化",
            "analyze",
            "design",
            "refactor",
            "implement",
            "optimize",
        ];

        let p_count = parallel_keywords
            .iter()
            .filter(|&&k| content.contains(k))
            .count();
        let c_count = complex_keywords
            .iter()
            .filter(|&&k| content.contains(k))
            .count();

        if p_count > 0 {
            score += 0.2 + (p_count as f32 * 0.05);
            reasons.push("PARALLEL_SIGNATURE".to_string());
            is_parallel = true;
        }
        if c_count >= 2 {
            score += 0.3;
            reasons.push("SEMANTIC_COMPLEXITY_BIAS".to_string());
        }

        let (media_count, media_tokens, media_types) = self.analyze_media_content(last_msg);
        if media_count > 0 {
            score += 0.5 + (media_count as f32 * 0.1);
            reasons.push(format!("MULTI_MODAL_INPUT({})", media_count));
            metadata_map.insert("media_types".to_string(), media_types.into());
        }

        let is_code = self.detect_code_content(&content);
        let input_text_tokens = self.estimate_tokens(&content, is_code);
        let input_tokens = input_text_tokens + media_tokens;

        let multiplier = if is_parallel {
            4.5
        } else if is_code {
            3.0
        } else {
            2.0
        };
        let predicted_output = (input_tokens as f32 * multiplier) as usize;

        if input_tokens > 2000 {
            score += 0.4;
            reasons.push("LARGE_CONTEXT_INPUT".to_string());
        }
        if predicted_output > 3000 {
            score += 0.3;
            reasons.push("LARGE_OUTPUT_PROJECTION".to_string());
        }

        let pressure = self.current_usage();
        if pressure > 75.0 {
            score += 0.15;
            reasons.push("HIGH_SYS_LOAD_BIAS".to_string());
        }

        ComplexityScore {
            score: score.clamp(0.0, 1.0),
            reason: reasons.join("|"),
            predicted_output_tokens: predicted_output,
            is_parallelizable: is_parallel,
            level: 1,
            metadata: serde_json::Value::Object(metadata_map),
        }
    }

    async fn estimate_level_2(
        &self,
        messages: &[MessageSnapshot],
        analyzer: &dyn SemanticComplexityAnalyzer,
    ) -> Result<ComplexityScore, String> {
        let last_msg = messages.last().expect("non-empty checked above");
        let prompt = format!(
            r#"Analyze the complexity of this task for an autonomous agent swarm.
Detect independent sub-tasks, parallelizability, and potential output length.

Task: "{}"

Output ONLY valid JSON:
{{
  "score": 0.0-1.0,
  "reason": "short explanation",
  "predicted_output_tokens": 1000,
  "is_parallelizable": true,
  "sub_tasks": ["task 1", "task 2"]
}}"#,
            last_msg.text
        );

        let response = analyzer.analyze_complexity(&prompt).await?;
        let json_str = self.extract_json_from_response(&response);

        match serde_json::from_str::<serde_json::Value>(&json_str) {
            Ok(parsed) => {
                let mut metadata = serde_json::Map::new();
                if let Some(st) = parsed["sub_tasks"].as_array() {
                    metadata.insert("sub_tasks".to_string(), st.clone().into());
                }

                Ok(ComplexityScore {
                    score: parsed["score"].as_f64().unwrap_or(0.5) as f32,
                    reason: parsed["reason"]
                        .as_str()
                        .unwrap_or("LLM_ANALYSIS")
                        .to_string(),
                    predicted_output_tokens: parsed["predicted_output_tokens"]
                        .as_u64()
                        .unwrap_or(1000) as usize,
                    is_parallelizable: parsed["is_parallelizable"].as_bool().unwrap_or(false),
                    level: 2,
                    metadata: serde_json::Value::Object(metadata),
                })
            }
            Err(error) => Err(format!(
                "Complexity L2 parse error: {}. Raw: {}",
                error, json_str
            )),
        }
    }

    fn analyze_media_content(&self, msg: &MessageSnapshot) -> (usize, usize, Vec<String>) {
        let mut count = 0;
        let mut tokens = 0;
        let mut types = Vec::new();

        for media in &msg.media {
            match media {
                MediaKind::Image => {
                    count += 1;
                    tokens += 1500;
                    types.push("image".to_string());
                }
                MediaKind::Audio => {
                    count += 1;
                    tokens += 1200;
                    types.push("audio".to_string());
                }
                MediaKind::Video => {
                    count += 1;
                    tokens += 5000;
                    types.push("video".to_string());
                }
            }
        }

        (count, tokens, types)
    }

    fn detect_code_content(&self, text: &str) -> bool {
        if text.contains("```") {
            return true;
        }
        let code_markers = [
            "fn ",
            "def ",
            "pub ",
            "class ",
            "let ",
            "const ",
            "impl ",
            "interface ",
        ];
        code_markers.iter().filter(|&&m| text.contains(m)).count() >= 2
    }

    fn estimate_tokens(&self, text: &str, is_code: bool) -> usize {
        if text.is_empty() {
            return 0;
        }

        let mut total: f32 = 0.0;
        for c in text.chars() {
            if (c >= '\u{4e00}' && c <= '\u{9fff}') || (c >= '\u{ac00}' && c <= '\u{d7af}') {
                total += 1.8;
            } else if is_code && c.is_ascii_punctuation() {
                total += 2.5;
            } else if c.is_whitespace() {
                total += 0.3;
            } else {
                total += 1.0;
            }
        }

        total.ceil() as usize
    }

    fn extract_json_from_response(&self, response: &str) -> String {
        if let Some(start) = response.find("```json") {
            let rest = &response[start + 7..];
            if let Some(end) = rest.find("```") {
                return rest[..end].trim().to_string();
            }
        }
        if let Some(start) = response.find("```") {
            let rest = &response[start + 3..];
            if let Some(end) = rest.find("```") {
                return rest[..end].trim().to_string();
            }
        }
        if let (Some(s), Some(e)) = (response.find('{'), response.rfind('}')) {
            return response[s..=e].to_string();
        }
        response.trim().to_string()
    }
}
