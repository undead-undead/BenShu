use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::{HierarchicalRetriever, RetrievalReport};
use benshu_compression::knowledge_snippet_text;
use benshu_infra::traits::security::{
    QueryProtectionAction, QueryProtectionDecision, QueryProtectionRequest, SecurityHandler,
};
use benshu_infra::{Tool, ToolDefinition};

/// Configuration for the KnowledgeSearchTool
#[derive(Debug, Clone)]
pub struct KnowledgeSearchConfig {
    pub default_limit: usize,
    pub max_limit: usize,
    pub snippet_len: usize,
    pub include_metadata: bool,
}

impl Default for KnowledgeSearchConfig {
    fn default() -> Self {
        Self {
            default_limit: 5,
            max_limit: 20,
            snippet_len: 800,
            include_metadata: true,
        }
    }
}

/// A Tool for Agents to perform deep hierarchical searches in the local knowledge base.
/// Optimized for the new Engram storage architecture (CAS + Unix Timestamps).
pub struct KnowledgeSearchTool {
    retriever: Arc<HierarchicalRetriever>,
    config: KnowledgeSearchConfig,
    security_handler: Option<Arc<dyn SecurityHandler>>,
}

impl KnowledgeSearchTool {
    pub fn new(retriever: Arc<HierarchicalRetriever>) -> Self {
        Self {
            retriever,
            config: KnowledgeSearchConfig::default(),
            security_handler: None,
        }
    }

    pub fn with_security_handler(mut self, security_handler: Arc<dyn SecurityHandler>) -> Self {
        self.security_handler = Some(security_handler);
        self
    }

    /// Format results for LLM consumption, injecting tiered context (Abstract/Overview/Content)
    fn format_results(&self, results: &[crate::hybrid_search::HybridSearchResult]) -> String {
        if results.is_empty() {
            return "No relevant information found in knowledge base.".to_string();
        }

        let mut output = String::from("### Knowledge Search Results\n\n");
        let kv = self.retriever.engine().engram_store().kv();

        for (i, res) in results.iter().enumerate() {
            output.push_str(&format!(
                "{}. **{}** [Score: {:.2}]\n",
                i + 1,
                res.document.path,
                res.rrf_score
            ));

            // Priority 1: Abstract (Teaser)
            if let Some(abs) = &res.document.abstract_content {
                output.push_str(&format!("   *Abstract*: {}\n", abs));
            }

            // Priority 2: Overview (Summary)
            if let Some(ov) = &res.document.overview_content {
                output.push_str(&format!("   *Overview*: {}\n", ov));
            }

            // Priority 3: Deep Content (CAS-backed Body)
            // Retrieve body using content_hash as Document struct no longer holds raw bytes
            match kv.get_content(&res.document.content_hash) {
                Ok(Some(raw_bytes)) => {
                    let content = String::from_utf8_lossy(raw_bytes.as_ref());
                    output.push_str(&format!(
                        "   *Snippet*: {}\n",
                        knowledge_snippet_text(&content, self.config.snippet_len)
                    ));
                }
                _ => {
                    output.push_str(
                        "   *Content*: [Full content pending higher-level verification]\n",
                    );
                }
            }

            if self.config.include_metadata && !res.document.metadata.is_empty() {
                let meta_str: String = res
                    .document
                    .metadata
                    .iter()
                    .take(3)
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(", ");
                output.push_str(&format!("   *Metadata*: {}\n", meta_str));
            }
            output.push_str("\n");
        }
        output
    }

    fn format_report(&self, report: &RetrievalReport) -> String {
        let mut output = String::from("### Retrieval Route\n\n");
        if !report.query_signature.is_empty() {
            output.push_str(&format!("Query Signature: {}\n", report.query_signature));
        }
        output.push_str(&format!("Requested Limit: {}\n", report.requested_limit));
        output.push_str(&format!("Estimated Token Cost: {}\n", report.token_cost));
        output.push_str(&format!(
            "Initial Candidates: {}/{}\n",
            report.initial_result_count, report.initial_limit
        ));
        if let Some(broadened_limit) = report.broadened_limit {
            let broadened_count = report
                .broadened_result_count
                .unwrap_or(report.initial_result_count);
            output.push_str(&format!(
                "Candidate Top-Up: {}/{}\n",
                broadened_count, broadened_limit
            ));
        }
        output.push_str(&format!("Landmarks: {}\n", report.landmark_count));
        output.push_str(&format!(
            "Drill-Down: {}/{}\n",
            report.drill_down_successes, report.drill_down_attempts
        ));
        output.push_str(&format!(
            "Safety Net: {}\n",
            if report.safety_net_triggered {
                "applied"
            } else {
                "not_needed"
            }
        ));
        if let Some(summary) = report.degradation_summary() {
            output.push_str(&format!("Retrieval Degradation: {}\n", summary));
        }
        if report.dos_hardening_triggered {
            let guard = if report.throttled {
                "throttled"
            } else if report.negative_cache_hit {
                "negative_cache_hit"
            } else if report.query_truncated {
                "query_truncated"
            } else {
                "active"
            };
            output.push_str(&format!("DoS Guard: {}\n", guard));
        }
        output.push_str(&format!("Latency Ms: {}\n\n", report.latency_ms));
        output
    }

    fn protect_query(&self, query: &str, limit: usize) -> Option<QueryProtectionDecision> {
        self.security_handler.as_ref().map(|security| {
            security.protect_query(&QueryProtectionRequest {
                surface: "knowledge_search_tool".to_string(),
                query: query.to_string(),
                requested_limit: limit,
                estimated_cost: None,
                prefers_deep_retrieval: true,
            })
        })
    }

    fn format_protection_decision(&self, decision: &QueryProtectionDecision) -> String {
        let mut output = String::from("### Query Protection\n\n");
        output.push_str(&format!("Action: {:?}\n", decision.action));
        output.push_str(&format!("Query Signature: {}\n", decision.query_signature));
        output.push_str(&format!("Estimated Cost: {}\n", decision.estimated_cost));
        if let Some(retry_after_ms) = decision.retry_after_ms {
            output.push_str(&format!("Retry After Ms: {}\n", retry_after_ms));
        }
        if !decision.reasons.is_empty() {
            output.push_str(&format!("Reasons: {}\n", decision.reasons.join(", ")));
        }
        output.push_str(&format!("Guidance: {}\n", decision.user_message));
        output.push_str("Path: lightweight retrieval fallback\n\n");
        output
    }

    fn lightweight_search(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<crate::hybrid_search::HybridSearchResult>> {
        self.retriever
            .engine()
            .search(query, limit)
            .map_err(|e| anyhow::anyhow!("Retrieval failed: {}", e))
    }
}

#[async_trait]
impl Tool for KnowledgeSearchTool {
    fn name(&self) -> String {
        "knowledge_search".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Search in internal knowledge base using hierarchical retrieval. Use this when the user asks for project-specific details, summaries, or deep technical data.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "limit": { "type": "integer", "description": "Result limit (1-20)", "default": 5 }
                },
                "required": ["query"]
            }),
            parameters_ts: Some("interface KnowledgeSearchArgs { query: string; limit?: number; }".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            limit: Option<usize>,
        }

        let args: Args = serde_json::from_str(arguments)?;
        let limit = args
            .limit
            .unwrap_or(self.config.default_limit)
            .clamp(1, self.config.max_limit);

        let protection = self.protect_query(&args.query, limit);
        if let Some(decision) = protection.as_ref() {
            if matches!(
                decision.action,
                QueryProtectionAction::Degrade | QueryProtectionAction::PauseCurrentPath
            ) {
                let results = self.lightweight_search(&args.query, limit)?;
                let mut output = self.format_protection_decision(decision);
                output.push_str(&self.format_results(&results));
                return Ok(output);
            }
        }

        let outcome = self
            .retriever
            .search_recursive_with_report(&args.query, limit)
            .await
            .map_err(|e| anyhow::anyhow!("Retrieval failed: {}", e))?;
        let mut output = self.format_report(&outcome.report);
        output.push_str(&self.format_results(&outcome.results));
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HybridSearchConfig;
    use crate::HybridSearchEngine;
    use benshu_infra::traits::security::{
        AuditLogRecord, DynamicPolicy, LeakDetection, QueryProtectionAction, SanitizedOutput,
    };
    use std::collections::HashMap;
    use tempfile::tempdir;

    struct StubSecurityHandler {
        action: QueryProtectionAction,
    }

    #[async_trait]
    impl SecurityHandler for StubSecurityHandler {
        fn check_input(&self, text: &str) -> SanitizedOutput {
            SanitizedOutput {
                content: text.to_string(),
                warnings: Vec::new(),
                was_modified: false,
            }
        }

        fn check_output(&self, text: &str) -> (String, Vec<LeakDetection>) {
            (text.to_string(), Vec::new())
        }

        fn log_action(
            &self,
            _session_key: Option<&str>,
            _tool_name: &str,
            _arguments: &str,
            _success: bool,
            _output_preview: &str,
            _backup: Option<benshu_infra::skill::BackupInfo>,
        ) {
        }

        async fn retrieve_audit_logs(&self, _limit: usize) -> anyhow::Result<Vec<AuditLogRecord>> {
            Ok(Vec::new())
        }

        fn get_dynamic_policy(&self) -> DynamicPolicy {
            DynamicPolicy::default()
        }

        fn protect_query(&self, request: &QueryProtectionRequest) -> QueryProtectionDecision {
            QueryProtectionDecision {
                action: self.action,
                surface: request.surface.clone(),
                query_signature: request.query.clone(),
                estimated_cost: request.estimated_cost.unwrap_or(8),
                retry_after_ms: Some(1500),
                protect_user: true,
                protect_system: true,
                reasons: vec!["test_query_protection".to_string()],
                user_message: "lightweight fallback engaged".to_string(),
            }
        }
    }

    #[tokio::test]
    async fn knowledge_search_tool_uses_lightweight_fallback_when_query_is_paused() {
        let temp = tempdir().expect("tempdir");
        let config = HybridSearchConfig {
            db_path: temp.path().join("engram.redb"),
            use_vector: false,
            use_reranker: false,
            ..Default::default()
        };
        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let store = engine.engram_store();
        store
            .store_document(
                "docs",
                "safety.md",
                "Safety",
                "query protection fallback keeps knowledge search available",
                false,
                HashMap::new(),
            )
            .expect("store document");
        let retriever = Arc::new(HierarchicalRetriever::new(engine));
        let tool = KnowledgeSearchTool::new(retriever).with_security_handler(Arc::new(
            StubSecurityHandler {
                action: QueryProtectionAction::PauseCurrentPath,
            },
        ));

        let output = tool
            .call(r#"{"query":"query protection fallback","limit":5}"#)
            .await
            .expect("tool call");

        assert!(output.contains("### Query Protection"));
        assert!(output.contains("Path: lightweight retrieval fallback"));
        assert!(output.contains("safety.md"));
    }
}
