use benshu_brain::skills::tool::{capability_route_preferred_tool_names_for_query, ToolSet};
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};
use benshu_routing::{
    build_pending_verification_followup_plan, build_pending_verification_result_envelope,
    classify_query_verification_plan_with_request, preferred_capability_domain_for_route,
    query_requests_image_generation, route_reason_for_plan, CapabilityClarificationHint,
    CapabilityRouteHint, CapabilityRouteRequest, CapabilityRouter, WebVerificationOrchestrator,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Clone)]
pub struct ToolSearchTool {
    toolset: ToolSet,
}

impl ToolSearchTool {
    pub fn new(toolset: ToolSet) -> Self {
        Self { toolset }
    }
}

#[derive(Debug, Deserialize)]
struct ToolSearchArgs {
    #[serde(default)]
    query: String,
    #[serde(default)]
    route_request: CapabilityRouteRequest,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    8
}

#[async_trait::async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> String {
        "tool_search".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "tool_search".to_string(),
            description: "Search the current tool catalog and return a short list of the most relevant tools for a task. Use this before choosing among many tools.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language description of the capability or task you need."
                    },
                    "route_request": {
                        "type": "object",
                        "description": "Optional shared capability-routing context used to bias tool search the same way the frontstage router does.",
                        "properties": {
                            "approved_forge_request": {"type": "boolean"},
                            "has_media_input": {"type": "boolean"},
                            "force_document_understanding": {"type": "boolean"},
                            "runtime_surface_bias": {"type": "boolean"},
                            "suppress_document_understanding": {"type": "boolean"},
                            "suppress_realtime_lookup": {"type": "boolean"}
                        }
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of tool matches to return.",
                        "minimum": 1,
                        "maximum": 20,
                        "default": 8
                    }
                },
                "required": ["query"]
            }),
            parameters_ts: Some(
                "type ToolSearchArgs = { query: string; route_request?: { approved_forge_request?: boolean; has_media_input?: boolean; force_document_understanding?: boolean; runtime_surface_bias?: boolean; suppress_document_understanding?: boolean; suppress_realtime_lookup?: boolean }; limit?: number }".to_string(),
            ),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some(
                "Call this first when there are many tools or when you are unsure which tool best fits the task. After receiving the shortlist, choose a concrete tool and call it.".to_string(),
            ),
            safety_level: SafetyLevel::Green,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: ToolSearchArgs = serde_json::from_str(arguments)?;
        let limit = args.limit.clamp(1, 20);
        let router = CapabilityRouter::new(args.route_request);
        let route = router.classify_query_route(&args.query);
        let capability_route = route
            .map(|value| router.route_label(value))
            .map(str::to_string);
        let capability_debug_label = route
            .map(|value| router.route_debug_label(value))
            .map(str::to_string);
        let preferred_capability_domain = route
            .and_then(preferred_capability_domain_for_route)
            .map(str::to_string);
        let preferred_tools: Vec<String> = route
            .map(|value| {
                capability_route_preferred_tool_names_for_query(value, &args.query)
                    .iter()
                    .map(|name| (*name).to_string())
                    .collect()
            })
            .unwrap_or_default();
        let orchestration_hint = route.and_then(|value| {
            if args.query.trim().is_empty() {
                return None;
            }
            if !matches!(value, CapabilityRouteHint::RealtimeLookup(_)) {
                return None;
            }
            if !preferred_tools.iter().any(|name| name == "delegate") {
                return None;
            }
            Some("If the overall user request also includes a downstream action such as save/import/send/notify, preserve the full original request when choosing the next tool and prefer coordinator delegation over narrowing the task to lookup-only phrasing.".to_string())
        });
        let clarification_hint = router
            .clarification_hint(&args.query)
            .map(|value| match value {
                CapabilityClarificationHint::MissingPriceTarget => "missing_price_target",
                CapabilityClarificationHint::MissingFxPair => "missing_fx_pair",
                CapabilityClarificationHint::MissingWeatherLocation => "missing_weather_location",
            })
            .map(str::to_string);
        let requires_real_tool_call = route
            .map(|value| router.route_requires_real_tool_call(value))
            .unwrap_or(false);
        let requires_source_fetch = route
            .map(|value| router.route_requires_source_fetch(value))
            .unwrap_or(false);
        let verification_plan =
            classify_query_verification_plan_with_request(&args.query, args.route_request);
        let verification_preview = verification_plan.map(|plan| {
            build_pending_verification_result_envelope(
                plan,
                requires_source_fetch,
                "verification routing selected but no verification tool has executed yet",
            )
        });
        let verification_followup =
            verification_plan.map(|plan| build_pending_verification_followup_plan(plan.mode));
        let orchestration_decision = WebVerificationOrchestrator::new().decide(
            verification_plan.as_ref(),
            verification_preview.as_ref(),
            verification_followup.as_ref(),
        );
        let route_reason = route_reason_for_plan(verification_plan.as_ref()).as_str();
        let matches = self
            .toolset
            .search_catalog_with_request(&args.query, limit, args.route_request)
            .await;
        let availability_hint = if matches.is_empty()
            && query_requests_image_generation(&args.query)
        {
            Some("No image generation tool is currently available in this runtime. Reply plainly that image generation is unavailable until a real image backend is configured and exposed.".to_string())
        } else {
            None
        };

        Ok(serde_json::to_string_pretty(&json!({
            "query": args.query,
            "route_request": args.route_request,
            "capability_route": capability_route,
            "capability_debug_label": capability_debug_label,
            "preferred_capability_domain": preferred_capability_domain,
            "clarification_hint": clarification_hint,
            "requires_real_tool_call": requires_real_tool_call,
            "requires_source_fetch": requires_source_fetch,
            "route_reason": route_reason,
            "verification_plan": verification_plan,
            "verification_preview": verification_preview,
            "verification_followup": verification_followup,
            "orchestration_decision": orchestration_decision,
            "availability_hint": availability_hint,
            "preferred_tools": preferred_tools,
            "orchestration_hint": orchestration_hint,
            "count": matches.len(),
            "tools": matches,
        }))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSearch;
    struct FakePdf;
    struct FakeImage;

    #[async_trait::async_trait]
    impl Tool for FakeSearch {
        fn name(&self) -> String {
            "web_search".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "web_search".into(),
                description: "Search the web for current information.".into(),
                parameters: json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                usage_guidelines: Some("Use for latest prices or web search.".into()),
                safety_level: SafetyLevel::Green,
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for FakePdf {
        fn name(&self) -> String {
            "pdf_parse".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "pdf_parse".into(),
                description: "Read and parse PDF files.".into(),
                parameters: json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                usage_guidelines: Some("Use when the task mentions PDF documents.".into()),
                safety_level: SafetyLevel::Yellow,
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for FakeImage {
        fn name(&self) -> String {
            "generate_image".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "generate_image".into(),
                description: "Generate an image from a text prompt.".into(),
                parameters: json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                usage_guidelines: Some(
                    "Use this to create new images from text descriptions.".into(),
                ),
                safety_level: SafetyLevel::Green,
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[tokio::test]
    async fn tool_search_returns_ranked_matches() {
        let toolset = ToolSet::new();
        toolset.add(FakeSearch).add(FakePdf);
        let tool = ToolSearchTool::new(toolset);

        let result = tool
            .call(r#"{"query":"btc latest price search","limit":3}"#)
            .await
            .unwrap();
        assert!(result.contains("\"web_search\""));
        assert!(result.contains("\"capability_route\": \"realtime_lookup.price\""));
        assert!(result.contains("\"preferred_tools\""));
        assert!(result.contains("\"web_fetch\""));
        assert!(result.contains("\"verification_plan\""));
        assert!(result.contains("\"verification_preview\""));
        assert!(result.contains("\"verification_followup\""));
        assert!(result.contains("\"route_reason\": \"structured_lookup_can_answer_directly\""));
        assert!(result.contains("\"orchestration_decision\""));
    }

    #[tokio::test]
    async fn tool_search_respects_route_request_biases() {
        let toolset = ToolSet::new();
        toolset.add(FakeSearch).add(FakePdf);
        let tool = ToolSearchTool::new(toolset);

        let result = tool
            .call(
                r#"{"query":"帮我看看这个截图","route_request":{"has_media_input":true},"limit":2}"#,
            )
            .await
            .unwrap();
        assert!(result.contains("\"capability_route\": \"document_understanding\""));
        assert!(result.contains("\"preferred_capability_domain\": \"document_understanding\""));
        assert!(result.contains("\"requires_real_tool_call\": true"));
        assert!(result.contains("\"route_request\""));
        assert!(result.contains("\"pdf_parse\""));
        assert!(result.contains("\"route_reason\": \"tool_lookup_required_before_answering\""));
    }

    #[tokio::test]
    async fn tool_search_compound_realtime_prefers_delegate_first() {
        let toolset = ToolSet::new();
        toolset.add(FakeSearch);
        toolset.add(ToolSearchTool::new(toolset.clone()));

        let tool = ToolSearchTool::new(toolset);
        let result = tool
            .call(r#"{"query":"请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。"}"#)
            .await
            .unwrap();

        assert!(result.contains("\"preferred_tools\""));
        assert!(result.contains("\"delegate\""));
        assert!(result.contains("\"orchestration_hint\""));
    }

    #[tokio::test]
    async fn tool_search_surfaces_image_generation_unavailable_hint_when_missing() {
        let toolset = ToolSet::new();
        let tool = ToolSearchTool::new(toolset);

        let result = tool
            .call(r#"{"query":"请帮我生成一张图片","limit":4}"#)
            .await
            .unwrap();
        assert!(result.contains("\"availability_hint\""));
        assert!(result.contains("image generation is unavailable"));
    }

    #[tokio::test]
    async fn tool_search_finds_generate_image_when_present() {
        let toolset = ToolSet::new();
        toolset.add(FakeImage);
        let tool = ToolSearchTool::new(toolset);

        let result = tool
            .call(r#"{"query":"generate image of a silver logo","limit":4}"#)
            .await
            .unwrap();
        assert!(result.contains("\"generate_image\""));
    }
}
