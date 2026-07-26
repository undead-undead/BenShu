//! Web Search Tool
//!
//! Provider-backed search with browser SERP fallback when configured.
//!
//! Feature-gated behind `http`.

pub(crate) mod orchestrator;
pub(crate) mod policy;
pub(crate) mod provider;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::debug;

use self::orchestrator::{
    EvidenceBundle, SearchCandidate, SearchOrchestrator, SearchOrchestratorConfig,
};
use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};
use benshu_routing::{
    build_search_result_followup_plan, build_verified_verification_result_envelope,
    route_reason_for_plan, QueryVerificationPlan, VerificationDomain, VerificationMode,
    VerificationRequirement, VerificationSource, WebVerificationOrchestrator,
};

/// Maximum cache entries to prevent unbounded growth.
const MAX_CACHE_ENTRIES: usize = 256;
/// Default cache TTL.
const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);
/// Cached search result with expiry.
struct CacheEntry {
    data: String,
    expires_at: Instant,
}

#[derive(Debug, Serialize, Deserialize)]
struct StructuredWebSearchPayload {
    kind: String,
    query: String,
    provider: String,
    queried_at: String,
    plan: serde_json::Value,
    diagnostics: serde_json::Value,
    evidence_bundle: serde_json::Value,
    verification_preview: serde_json::Value,
    verification_followup: serde_json::Value,
    results: Vec<SearchResult>,
}

/// Web search tool configuration.
#[derive(Debug, Clone)]
pub struct WebSearchConfig {
    /// Max results to return
    pub max_results: u8,
    /// Cache TTL
    pub cache_ttl: Duration,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            max_results: 5,
            cache_ttl: DEFAULT_CACHE_TTL,
        }
    }
}

/// Web search tool — lets the Agent search the internet.
pub struct WebSearchTool {
    config: WebSearchConfig,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl WebSearchTool {
    /// Create a new web search tool.
    pub fn new(config: WebSearchConfig) -> Result<Self, Error> {
        Ok(Self {
            config,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Create with default config.
    pub fn from_env() -> Result<Self, Error> {
        Self::new(WebSearchConfig::default())
    }

    /// Check and return cached result.
    fn cache_get(&self, key: &str) -> Option<String> {
        let cache = self.cache.lock().ok()?;
        if let Some(entry) = cache.get(key) {
            if Instant::now() < entry.expires_at {
                return Some(entry.data.clone());
            }
        }
        None
    }

    /// Store a result in cache (with eviction).
    fn cache_set(&self, key: String, data: String) {
        if let Ok(mut cache) = self.cache.lock() {
            // Evict expired entries when cache is full
            if cache.len() >= MAX_CACHE_ENTRIES {
                let now = Instant::now();
                cache.retain(|_, v| v.expires_at > now);
                // If still full, remove oldest
                if cache.len() >= MAX_CACHE_ENTRIES {
                    if let Some(oldest_key) = cache
                        .iter()
                        .min_by_key(|(_, v)| v.expires_at)
                        .map(|(k, _)| k.clone())
                    {
                        cache.remove(&oldest_key);
                    }
                }
            }
            cache.insert(
                key,
                CacheEntry {
                    data,
                    expires_at: Instant::now() + self.config.cache_ttl,
                },
            );
        }
    }

    async fn execute_search(&self, query: &str) -> anyhow::Result<EvidenceBundle> {
        let orchestrator = SearchOrchestrator::new(SearchOrchestratorConfig {
            max_results: self.config.max_results as usize,
            ..SearchOrchestratorConfig::default()
        })?;
        Ok(orchestrator.search(query).await)
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> String {
        "web_search".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web for information. Returns a list of results with titles, URLs, and snippets.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "structured": {
                        "type": "boolean",
                        "description": "When true, return a structured payload with verification preview instead of the legacy raw result array."
                    }
                },
                "required": ["query"]
            }),
            parameters_ts: Some("interface WebSearch {\n  query: string; // The search query\n  structured?: boolean; // Return verification-aware structured payload\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use to find current information, URLs, documentation, or facts from the internet through installed Microsoft Edge or Google Chrome. If neither browser is installed, tell the user to install one of them.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            #[serde(default)]
            structured: bool,
        }
        let args: Args = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Invalid arguments: {}", e))?;

        let query = args.query.trim();
        if query.is_empty() {
            anyhow::bail!("Search query cannot be empty");
        }

        // Check cache
        if let Some(cached) = self.cache_get(query) {
            debug!(query = query, "Cache hit for web search");
            let bundle: EvidenceBundle = serde_json::from_str(&cached)
                .map_err(|e| anyhow::anyhow!("Invalid cached search evidence bundle: {}", e))?;
            if args.structured {
                return render_structured_web_search_payload(query, "cache", &bundle);
            }
            return render_legacy_search_results(&bundle);
        }

        let bundle = self.execute_search(query).await?;
        let cached = serde_json::to_string(&bundle)?;

        // Avoid caching empty lookups: the next turn should be able to retry with
        // a refined query or different runtime conditions instead of reusing [].
        if !bundle.candidates.is_empty() {
            self.cache_set(query.to_string(), cached);
        }
        if args.structured {
            render_structured_web_search_payload(query, "orchestrator", &bundle)
        } else {
            render_legacy_search_results(&bundle)
        }
    }
}

/// A single search result.
#[derive(Debug, Serialize, Deserialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

impl From<&SearchCandidate> for SearchResult {
    fn from(candidate: &SearchCandidate) -> Self {
        Self {
            title: candidate.title.clone(),
            url: candidate.url.clone(),
            snippet: candidate.snippet.clone(),
        }
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn verification_source_from_search_result(result: &SearchResult) -> VerificationSource {
    VerificationSource {
        kind: "web_search_result".to_string(),
        title: result.title.clone(),
        uri: result.url.clone(),
        observed_at: Some(now_rfc3339()),
    }
}

fn render_structured_web_search_payload(
    query: &str,
    provider: &str,
    bundle: &EvidenceBundle,
) -> anyhow::Result<String> {
    let results: Vec<SearchResult> = bundle.candidates.iter().map(SearchResult::from).collect();
    let verification_plan = QueryVerificationPlan {
        domain: VerificationDomain::KnowledgeFact,
        requirement: VerificationRequirement::Required,
        mode: VerificationMode::WebSearchFetch,
        route_hint: None,
    };
    let verification_preview = build_verified_verification_result_envelope(
        VerificationDomain::KnowledgeFact,
        VerificationMode::WebSearchFetch,
        results
            .iter()
            .map(verification_source_from_search_result)
            .collect(),
        "web search completed; URLs discovered but source pages have not been fetched yet",
    );
    let verification_followup = build_search_result_followup_plan();
    let orchestration_decision = WebVerificationOrchestrator::new().decide(
        Some(&verification_plan),
        Some(&verification_preview),
        Some(&verification_followup),
    );
    let payload = StructuredWebSearchPayload {
        kind: "web_search".to_string(),
        query: query.to_string(),
        provider: provider.to_string(),
        queried_at: now_rfc3339(),
        plan: serde_json::to_value(&bundle.plan)?,
        diagnostics: serde_json::to_value(&bundle.diagnostics)?,
        evidence_bundle: serde_json::to_value(bundle)?,
        verification_preview: serde_json::to_value(verification_preview)?,
        verification_followup: serde_json::to_value(verification_followup)?,
        results,
    };
    let mut payload = serde_json::to_value(payload)?;
    payload["route_reason"] = serde_json::Value::String(
        route_reason_for_plan(Some(&verification_plan))
            .as_str()
            .to_string(),
    );
    payload["orchestration_decision"] = serde_json::to_value(orchestration_decision)?;
    Ok(serde_json::to_string_pretty(&payload)?)
}

fn render_legacy_search_results(bundle: &EvidenceBundle) -> anyhow::Result<String> {
    let results: Vec<SearchResult> = bundle.candidates.iter().map(SearchResult::from).collect();
    if results.is_empty() {
        let diagnostics = bundle
            .diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "- source={} status={} message={} retry_hint={}",
                    diagnostic.source, diagnostic.status, diagnostic.message, diagnostic.retry_hint
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(format!(
            "status: blocked\nexecuted_tool: web_search\nquery: {}\nresults: []\nblockers: no candidate search results survived source retrieval and relevance filtering\nsource_diagnostics:\n{}",
            bundle.query.trim(),
            diagnostics
        ));
    }
    Ok(serde_json::to_string_pretty(&results)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_web_search_payload_contains_verification_preview() {
        let bundle = EvidenceBundle {
            kind: "search_evidence_bundle".to_string(),
            query: "example".to_string(),
            plan: crate::tool::web_search::orchestrator::build_search_plan("example"),
            diagnostics: Vec::new(),
            candidates: vec![SearchCandidate {
                title: "Example".to_string(),
                url: "https://example.com".to_string(),
                snippet: "Example snippet".to_string(),
                source: "browser".to_string(),
                capability: crate::tool::web_search::orchestrator::SourceCapability::Browser,
                rank: 1,
                score: 1.0,
            }],
        };
        let rendered = render_structured_web_search_payload("example", "browser", &bundle).unwrap();
        let payload: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(payload["kind"], "web_search");
        assert_eq!(payload["provider"], "browser");
        assert_eq!(payload["plan"]["intent"], "general");
        assert_eq!(payload["evidence_bundle"]["kind"], "search_evidence_bundle");
        assert_eq!(
            payload["verification_preview"]["outcome"],
            "VerificationSucceeded"
        );
        assert_eq!(
            payload["verification_followup"]["answer_readiness"],
            "search_results_only"
        );
        assert_eq!(
            payload["verification_followup"]["next_tools"][0],
            "web_fetch"
        );
        assert_eq!(
            payload["route_reason"],
            "external_fact_requires_search_then_source_read"
        );
        assert_eq!(
            payload["orchestration_decision"]["continuation"],
            "ContinueFetchOrBrowse"
        );
        assert_eq!(payload["results"][0]["url"], "https://example.com");
    }

    #[test]
    fn legacy_search_results_remain_compatible() {
        let bundle = EvidenceBundle {
            kind: "search_evidence_bundle".to_string(),
            query: "example".to_string(),
            plan: crate::tool::web_search::orchestrator::build_search_plan("example"),
            diagnostics: Vec::new(),
            candidates: vec![SearchCandidate {
                title: "Example".to_string(),
                url: "https://example.com".to_string(),
                snippet: "Example snippet".to_string(),
                source: "browser".to_string(),
                capability: crate::tool::web_search::orchestrator::SourceCapability::Browser,
                rank: 1,
                score: 1.0,
            }],
        };
        let rendered = render_legacy_search_results(&bundle).unwrap();
        let results: Vec<SearchResult> = serde_json::from_str(&rendered).unwrap();
        assert_eq!(results[0].url, "https://example.com");
    }

    #[test]
    fn legacy_empty_search_result_returns_blocker_diagnostics() {
        let bundle = EvidenceBundle {
            kind: "search_evidence_bundle".to_string(),
            query: "example".to_string(),
            plan: crate::tool::web_search::orchestrator::build_search_plan("example"),
            diagnostics: vec![crate::tool::web_search::orchestrator::SourceDiagnostic {
                source: "bing".to_string(),
                capability: crate::tool::web_search::orchestrator::SourceCapability::Public,
                status: "ok".to_string(),
                message: "0 candidates".to_string(),
                retry_hint: "try a different query".to_string(),
            }],
            candidates: Vec::new(),
        };
        let rendered = render_legacy_search_results(&bundle).unwrap();
        assert!(rendered.contains("status: blocked"));
        assert!(rendered.contains("executed_tool: web_search"));
        assert!(rendered.contains("source=bing"));
    }
}
