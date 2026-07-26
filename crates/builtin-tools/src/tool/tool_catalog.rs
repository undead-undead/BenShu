use benshu_brain::skills::tool::ToolSet;
use benshu_infra::{SafetyLevel, Tool, ToolCatalogEntry, ToolDefinition};
use benshu_routing::{
    preferred_capability_domain_for_route, CapabilityClarificationHint, CapabilityRouteRequest,
    CapabilityRouter,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct ToolCatalogTool {
    toolset: ToolSet,
}

impl ToolCatalogTool {
    pub fn new(toolset: ToolSet) -> Self {
        Self { toolset }
    }
}

#[derive(Debug, Deserialize)]
struct ToolCatalogArgs {
    #[serde(default)]
    query: String,
    #[serde(default)]
    route_request: CapabilityRouteRequest,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    capability_domain: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

fn matches_filter(entry: &ToolCatalogEntry, args: &ToolCatalogArgs) -> bool {
    if let Some(source) = &args.source {
        if entry.source != source.trim().to_lowercase() {
            return false;
        }
    }

    if let Some(scope) = &args.scope {
        if entry.scope != scope.trim().to_lowercase() {
            return false;
        }
    }

    if let Some(capability_domain) = &args.capability_domain {
        let wanted = capability_domain.trim().to_lowercase();
        if !(entry.capability_domain == wanted || entry.capability_domain.starts_with(&wanted)) {
            return false;
        }
    }

    true
}

fn summarize(entries: &[ToolCatalogEntry]) -> serde_json::Value {
    let mut by_source: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_scope: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_capability: BTreeMap<String, usize> = BTreeMap::new();

    for entry in entries {
        *by_source.entry(entry.source.clone()).or_default() += 1;
        *by_scope.entry(entry.scope.clone()).or_default() += 1;
        *by_capability
            .entry(entry.capability_domain.clone())
            .or_default() += 1;
    }

    json!({
        "sources": by_source,
        "scopes": by_scope,
        "capability_domains": by_capability,
    })
}

#[async_trait::async_trait]
impl Tool for ToolCatalogTool {
    fn name(&self) -> String {
        "tool_catalog".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "tool_catalog".to_string(),
            description: "Inspect the current unified tool catalog with optional filters for source, capability domain, scope, or query.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Optional natural language query used to rank matching tools before returning them."
                    },
                    "route_request": {
                        "type": "object",
                        "description": "Optional shared capability-routing context used to bias catalog ranking the same way the frontstage router does.",
                        "properties": {
                            "approved_forge_request": {"type": "boolean"},
                            "has_media_input": {"type": "boolean"},
                            "force_document_understanding": {"type": "boolean"},
                            "runtime_surface_bias": {"type": "boolean"},
                            "suppress_document_understanding": {"type": "boolean"},
                            "suppress_realtime_lookup": {"type": "boolean"}
                        }
                    },
                    "source": {
                        "type": "string",
                        "description": "Optional source filter such as builtin, mcp, forge, or skill."
                    },
                    "capability_domain": {
                        "type": "string",
                        "description": "Optional capability-domain filter such as realtime_lookup, document_understanding, runtime_surface, or external_cli_tools."
                    },
                    "scope": {
                        "type": "string",
                        "description": "Optional scope filter such as agent, session, or external."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 20,
                        "description": "Maximum number of catalog entries to return."
                    }
                }
            }),
            parameters_ts: Some("type ToolCatalogArgs = { query?: string; route_request?: { approved_forge_request?: boolean; has_media_input?: boolean; force_document_understanding?: boolean; runtime_surface_bias?: boolean; suppress_document_understanding?: boolean; suppress_realtime_lookup?: boolean }; source?: string; capability_domain?: string; scope?: string; limit?: number }".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some(
                "Use this when you need to inspect the current unified tool registry, debug why a tool is or is not visible, or confirm which capability domains are currently available.".to_string(),
            ),
            safety_level: SafetyLevel::Green,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: ToolCatalogArgs = if arguments.trim().is_empty() {
            ToolCatalogArgs {
                query: String::new(),
                route_request: CapabilityRouteRequest::default(),
                source: None,
                capability_domain: None,
                scope: None,
                limit: default_limit(),
            }
        } else {
            serde_json::from_str(arguments)?
        };

        let router = CapabilityRouter::new(args.route_request);
        let route = router.classify_query_route(&args.query);
        let limit = args.limit.clamp(1, 100);
        let mut entries = if args.query.trim().is_empty() {
            self.toolset.catalog().await
        } else {
            self.toolset
                .search_catalog_with_request(&args.query, limit, args.route_request)
                .await
        };

        entries.retain(|entry| matches_filter(entry, &args));
        if args.query.trim().is_empty() && entries.len() > limit {
            entries.truncate(limit);
        }

        Ok(serde_json::to_string_pretty(&json!({
            "query": args.query,
            "route_request": args.route_request,
            "capability_route": route.map(|value| router.route_label(value)).map(str::to_string),
            "preferred_capability_domain": route.and_then(preferred_capability_domain_for_route).map(str::to_string),
            "clarification_hint": router.clarification_hint(&args.query).map(|value| match value {
                CapabilityClarificationHint::MissingPriceTarget => "missing_price_target",
                CapabilityClarificationHint::MissingFxPair => "missing_fx_pair",
                CapabilityClarificationHint::MissingWeatherLocation => "missing_weather_location",
            }),
            "filters": {
                "source": args.source,
                "capability_domain": args.capability_domain,
                "scope": args.scope,
                "limit": limit,
            },
            "count": entries.len(),
            "summary": summarize(&entries),
            "tools": entries,
        }))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeBuiltin;
    struct FakeMcp;
    struct FakeForge;

    #[async_trait::async_trait]
    impl Tool for FakeBuiltin {
        fn name(&self) -> String {
            "web_search".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "web_search".into(),
                description: "Search the web for fresh public information.".into(),
                parameters: json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                usage_guidelines: Some("Use for latest info and current events.".into()),
                safety_level: SafetyLevel::Green,
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for FakeMcp {
        fn name(&self) -> String {
            "mcp:browser.open".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "mcp:browser.open".into(),
                description: "Open a browser page through MCP.".into(),
                parameters: json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                usage_guidelines: Some("Use for external browser automation.".into()),
                safety_level: SafetyLevel::Yellow,
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for FakeForge {
        fn name(&self) -> String {
            "forge_skill".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "forge_skill".into(),
                description: "Forge a new skill at runtime.".into(),
                parameters: json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                usage_guidelines: Some("Use when no current capability fits the task.".into()),
                safety_level: SafetyLevel::Yellow,
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[tokio::test]
    async fn tool_catalog_lists_summary_and_filters() {
        let toolset = ToolSet::new();
        toolset.add(FakeBuiltin).add(FakeMcp).add(FakeForge);
        let tool = ToolCatalogTool::new(toolset);

        let output = tool
            .call(r#"{"source":"mcp","limit":10}"#)
            .await
            .expect("tool catalog output");

        assert!(output.contains("\"mcp:browser.open\""));
        assert!(output.contains("\"sources\""));
        assert!(!output.contains("\"web_search\""));
    }

    #[tokio::test]
    async fn tool_catalog_query_uses_ranked_search() {
        let toolset = ToolSet::new();
        toolset.add(FakeBuiltin).add(FakeMcp).add(FakeForge);
        let tool = ToolCatalogTool::new(toolset);

        let output = tool
            .call(r#"{"query":"latest web info","limit":5}"#)
            .await
            .expect("tool catalog output");

        assert!(output.contains("\"web_search\""));
    }
}
