use std::sync::Arc;

use benshu_brain::skills::tool::{Tool, ToolDefinition};
use benshu_compression::knowledge_snippet_text;
use benshu_engram::{HierarchicalRetriever, RetrievalReport};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

/// Tool that allows the Agent to perform deep recursive knowledge searches
pub struct KnowledgeSearchTool {
    retriever: Arc<HierarchicalRetriever>,
}

impl KnowledgeSearchTool {
    pub fn new(retriever: Arc<HierarchicalRetriever>) -> Self {
        Self { retriever }
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
            output.push_str(&format!(
                "Candidate Top-Up: {}/{}\n",
                report
                    .broadened_result_count
                    .unwrap_or(report.initial_result_count),
                broadened_limit
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
}

#[async_trait]
impl Tool for KnowledgeSearchTool {
    fn name(&self) -> String {
        "knowledge_search".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Deep recursive search in the local knowledge base. Use this for complex queries that require analyzing abstracts and full content to find precise answers.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The natural language query to search for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
            parameters_ts: Some("interface KnowledgeSearchArgs {\n  query: string;\n  limit?: number;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this when the user asks questions about project documentation, architecture, settings, or historical data stored in the knowledge base.".to_string()),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            limit: Option<usize>,
        }
        let args: Args = serde_json::from_str(arguments)?;
        let limit = args.limit.unwrap_or(5);

        // Perform recursive search
        let outcome = self
            .retriever
            .search_recursive_with_report(&args.query, limit)
            .await?;

        if outcome.results.is_empty() {
            return Ok("No relevant information found in the knowledge base.".to_string());
        }

        // Format results for LLM consumption
        let mut output = self.format_report(&outcome.report);
        output.push_str("### Knowledge Search Results\n\n");
        for (i, res) in outcome.results.iter().enumerate() {
            output.push_str(&format!(
                "{}. [{}] (Score: {:.2})\n",
                i + 1,
                res.document.path,
                res.rrf_score
            ));
            if let Some(content) = &res.document.body {
                output.push_str(&format!(
                    "   Content Snippet: {}\n",
                    knowledge_snippet_text(content, 1000)
                ));
            }
            output.push_str("\n");
        }

        Ok(output)
    }
}
