use super::*;
use crate::agent::builder::AgentBuilder;
use crate::agent::memory::{
    BackgroundEnvelope, BackgroundQualitySignal, RelationshipBackgroundLayer,
};
use crate::agent::message::Content;
use crate::agent::provider::{ChatRequest, MockProvider, ProviderMetadata};
use crate::agent::runtime_support::RuntimeExecutionSeed;
use crate::agent::runtime_support::RuntimeStageSignal;
use crate::agent::session::SessionStatus;
use crate::agent::streaming::StreamingResponse;
use crate::agent::streaming::{FinishReason, MockStreamBuilder, ProviderTelemetry};
use crate::error::Result;
use crate::skills::tool::ToolCatalogOverride;
use crate::testing::MockSecurityHandler;
use async_trait::async_trait;
use benshu_infra::traits::tool::{Tool, ToolDefinition};
use benshu_telemetry::{RuntimeStage, TraceStatus};
use chrono::Utc;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone, Default)]
struct CaptureProvider {
    last_request: Arc<tokio::sync::Mutex<Option<ChatRequest>>>,
}

#[async_trait]
impl Provider for CaptureProvider {
    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        *self.last_request.lock().await = Some(request);
        Ok(MockStreamBuilder::new()
            .message("ok")
            .finish(FinishReason::Stop)
            .telemetry(ProviderTelemetry {
                provider_name: Some("capture".to_string()),
                model: Some("capture-model".to_string()),
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

    fn metadata() -> ProviderMetadata
    where
        Self: Sized,
    {
        ProviderMetadata {
            id: "capture".to_string(),
            name: "Capture".to_string(),
            description: "Captures the last request for tests".to_string(),
            icon: "".to_string(),
            fields: vec![],
            capabilities: vec![],
            preferred_models: vec![],
        }
    }
}

#[derive(Clone)]
struct TestTool(&'static str);

#[async_trait]
impl Tool for TestTool {
    fn name(&self) -> String {
        self.0.to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.0.to_string(),
            description: format!("test tool {}", self.0),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
            usage_guidelines: None,
            is_binary: false,
            is_verified: false,
            parameters_ts: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
        Ok("ok".to_string())
    }
}

#[test]
fn test_agent_config_default() {
    let config = AgentConfig::default();
    assert_eq!(config.model, "benshu-unconfigured-model");
    assert_eq!(config.max_tokens, Some(128000));
}

#[test]
fn runtime_thread_id_prefers_session_id() {
    let thread_id = Agent::<crate::agent::provider::MockProvider>::derive_runtime_thread_id(
        Some("session-123"),
        Uuid::nil(),
    );
    assert_eq!(thread_id, "session-123");
}

#[test]
fn runtime_thread_id_falls_back_to_task_scope() {
    let task_id = Uuid::nil();
    let thread_id =
        Agent::<crate::agent::provider::MockProvider>::derive_runtime_thread_id(None, task_id);
    assert_eq!(thread_id, format!("thread:{task_id}"));
}

#[tokio::test]
async fn stream_chat_applies_prompt_visible_tool_filter() {
    let provider = CaptureProvider::default();
    let mut builder = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test");

    builder = builder.with_tool(TestTool("tool_search"));
    for index in 0..8 {
        builder = builder.with_tool_catalog(
            TestTool(Box::leak(format!("mcp_tool_{index}").into_boxed_str())),
            ToolCatalogOverride {
                source: Some("mcp".to_string()),
                scope: Some("agent".to_string()),
                capability_domain: Some("general".to_string()),
                tags: vec!["mcp".to_string()],
            },
        );
    }

    let agent = builder.build().unwrap();
    agent
        .stream_chat(vec![Message::user("hello".to_string())])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: HashSet<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert!(tool_names.contains("tool_search"));
    assert!(tool_names.len() < 9);
    assert!(!tool_names.iter().any(|name| name.starts_with("mcp_tool_")));
}

#[tokio::test]
async fn stream_chat_file_ops_route_uses_minimal_tool_surface() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("read_file"))
        .with_tool(TestTool("list_dir"))
        .with_tool(TestTool("edit_file"))
        .with_tool(TestTool("write_file"))
        .with_tool(TestTool("tool_search"))
        .with_tool(TestTool("shell"))
        .with_tool(TestTool("runtime_surface"))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![Message::user(
            "请读取 /home/biubiuboy/BenShu/data/agents/benshu/AGENT.md 的前三行".to_string(),
        )])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: HashSet<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert_eq!(
        tool_names,
        HashSet::from([
            "read_file".to_string(),
            "list_dir".to_string(),
            "edit_file".to_string(),
            "write_file".to_string(),
            "tool_search".to_string(),
        ])
    );
}

#[tokio::test]
async fn stream_chat_requires_read_skill_manual_before_other_tools() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_extra_params(json!({
            "matched_skill_manual": "python_tooling"
        }))
        .with_tool(TestTool("read_skill_manual"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "read_skill_manual".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![Message::user(
            "use the python tooling skill".to_string(),
        )])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert_eq!(tool_names, vec!["read_skill_manual".to_string()]);
}

#[tokio::test]
async fn stream_chat_detects_matched_skill_manual_from_system_message() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("read_skill_manual"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "read_skill_manual".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![
            Message::system(
                "### RUNTIME_SURFACE_HARD_ROUTE\n\
                 This request matches the skill `python_tooling`. Call `read_skill_manual` for that skill before executing runtime steps, unless you already loaded that manual in this turn.\n"
                    .to_string(),
            ),
            Message::user("use the python tooling skill".to_string()),
        ])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert_eq!(tool_names, vec!["read_skill_manual".to_string()]);
}

#[tokio::test]
async fn stream_chat_requires_read_skill_asset_when_user_explicitly_mentions_asset_path() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("read_skill_manual"))
        .with_tool(TestTool("read_skill_asset"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "read_skill_manual".to_string(),
            "read_skill_asset".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![
            Message::system(
                "### RUNTIME_SURFACE_HARD_ROUTE\n\
                 This request matches the skill `python_tooling`. Call `read_skill_manual` for that skill before executing runtime steps, unless you already loaded that manual in this turn.\n"
                    .to_string(),
            ),
            Message::tool_result("call_1", "# Skill: python_tooling\n\nmanual")
                .with_tool_name("read_skill_manual"),
            Message::user("use references/setup.md from python_tooling".to_string()),
        ])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert_eq!(tool_names, vec!["read_skill_asset".to_string()]);
}

#[tokio::test]
async fn stream_chat_requires_read_skill_asset_when_user_mentions_reference_kind_after_manual() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("read_skill_manual"))
        .with_tool(TestTool("read_skill_asset"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "read_skill_manual".to_string(),
            "read_skill_asset".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![
            Message::system(
                "### RUNTIME_SURFACE_HARD_ROUTE\n\
                 This request matches the skill `python_tooling`. Call `read_skill_manual` for that skill before executing runtime steps, unless you already loaded that manual in this turn.\n"
                    .to_string(),
            ),
            Message::tool_result(
                "call_1",
                "# Skill: python_tooling\n\n## Available Skill Assets\n\n- `references/setup.md` (references)\n- `scripts/run.py` (scripts)\n\nUse `read_skill_asset` with the relative asset path when you need one of these supporting files.\n",
            )
            .with_tool_name("read_skill_manual"),
            Message::user("先看这个 skill 的参考资料".to_string()),
        ])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert_eq!(tool_names, vec!["read_skill_asset".to_string()]);
}

#[tokio::test]
async fn stream_chat_prioritizes_forged_session_tool_after_approved_forge() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("forge_skill"))
        .with_tool(TestTool("pdf_builder"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "forge_skill".to_string(),
            "pdf_builder".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![
            Message::system("### FORGE_APPROVED\nforge request approved".to_string()),
            Message::tool_result(
                "call_1",
                serde_json::json!({
                    "status": "success",
                    "tool_name": "pdf_builder",
                    "source": "forge",
                    "scope": "session",
                    "execution_surface": "runtime",
                    "smoke_test": {
                        "status": "passed",
                        "latency_ms": 42,
                        "execution_surface": "runtime",
                        "output_preview": "ok"
                    }
                })
                .to_string(),
            )
            .with_tool_name("forge_skill"),
            Message::user("继续执行这个刚 forge 的工具".to_string()),
        ])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert_eq!(tool_names, vec!["pdf_builder".to_string()]);
}

#[tokio::test]
async fn stream_chat_injects_media_followup_guidance_into_system_prompt() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("document_understand"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "document_understand".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![
            Message::tool_result(
                "call_1",
                json!({
                    "status": "needs_followup",
                    "media_preprocess_route": "normalize_audio",
                    "media_pipeline_outcome": "model_failed_after_preprocess"
                })
                .to_string(),
            )
            .with_tool_name("document_understand"),
            Message::user("继续处理这个音频".to_string()),
        ])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let system_prompt = request.system_prompt.expect("system prompt present");

    assert!(system_prompt.contains("MEDIA FOLLOW-UP STRATEGY"));
    assert!(system_prompt.contains("alternate_model_fallback"));
}

#[tokio::test]
async fn stream_chat_injects_truth_verification_guidance_into_system_prompt() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .build()
        .unwrap();

    agent
        .stream_chat(vec![Message::user(
            "当前 OpenAI API 定价政策是什么".to_string(),
        )])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let system_prompt = request.system_prompt.expect("system prompt present");

    assert!(system_prompt.contains("TRUTH AND VERIFICATION CONTRACT"));
    assert!(system_prompt.contains("Never present unverified claims as confirmed facts."));
}

#[tokio::test]
async fn stream_chat_prefers_document_understanding_tools_for_alternate_media_fallback() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("document_understand"))
        .with_tool(TestTool("pdf_parse"))
        .with_tool(TestTool("text_extract"))
        .with_tool(TestTool("tool_search"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "document_understand".to_string(),
            "pdf_parse".to_string(),
            "text_extract".to_string(),
            "tool_search".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![
            Message::tool_result(
                "call_1",
                json!({
                    "status": "needs_followup",
                    "media_preprocess_route": "normalize_audio",
                    "media_pipeline_outcome": "model_failed_after_preprocess"
                })
                .to_string(),
            )
            .with_tool_name("document_understand"),
            Message::user("继续处理这个音频".to_string()),
        ])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: HashSet<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert!(tool_names.contains("document_understand"));
    assert!(tool_names.contains("pdf_parse"));
    assert!(tool_names.contains("text_extract"));
    assert!(tool_names.contains("tool_search"));
    assert!(!tool_names.contains("runtime_surface"));
}

#[tokio::test]
async fn stream_chat_prefers_document_understanding_tools_for_attachment_media_fallback() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("document_understand"))
        .with_tool(TestTool("pdf_parse"))
        .with_tool(TestTool("text_extract"))
        .with_tool(TestTool("tool_search"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "document_understand".to_string(),
            "pdf_parse".to_string(),
            "text_extract".to_string(),
            "tool_search".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![
            Message::tool_result(
                "call_1",
                json!({
                    "status": "needs_followup",
                    "media_preprocess_route": "extract_video_frames",
                    "media_pipeline_outcome": "preprocess_failed"
                })
                .to_string(),
            )
            .with_tool_name("document_understand"),
            Message::user("继续处理这个视频".to_string()),
        ])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: HashSet<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert!(tool_names.contains("document_understand"));
    assert!(tool_names.contains("pdf_parse"));
    assert!(tool_names.contains("text_extract"));
    assert!(tool_names.contains("tool_search"));
    assert!(!tool_names.contains("runtime_surface"));
    let extra = request.extra_params.expect("extra params");
    assert_eq!(
        extra.get("capability_route").and_then(|v| v.as_str()),
        Some("document_understanding")
    );
    assert_eq!(
        extra
            .get("preferred_capability_domain")
            .and_then(|v| v.as_str()),
        Some("document_understanding")
    );
    assert_eq!(
        extra
            .get("media_followup_execution_surface")
            .and_then(|v| v.as_str()),
        Some("document_understanding_attachment_fallback")
    );
}

#[tokio::test]
async fn stream_chat_prefers_document_understanding_tools_for_text_extract_media_fallback() {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("document_understand"))
        .with_tool(TestTool("pdf_parse"))
        .with_tool(TestTool("text_extract"))
        .with_tool(TestTool("tool_search"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "document_understand".to_string(),
            "pdf_parse".to_string(),
            "text_extract".to_string(),
            "tool_search".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    agent
        .stream_chat(vec![
            Message::tool_result(
                "call_1",
                json!({
                    "status": "error",
                    "media_preprocess_route": "image_page_raster",
                    "media_pipeline_outcome": "model_failed_after_preprocess"
                })
                .to_string(),
            )
            .with_tool_name("text_extract"),
            Message::user("继续处理这个截图".to_string()),
        ])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: HashSet<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert!(tool_names.contains("document_understand"));
    assert!(tool_names.contains("pdf_parse"));
    assert!(tool_names.contains("text_extract"));
    assert!(tool_names.contains("tool_search"));
    assert!(!tool_names.contains("runtime_surface"));
}

#[tokio::test]
async fn stream_chat_prefers_document_understanding_tools_for_provider_media_fallback_from_assistant_metadata(
) {
    let provider = CaptureProvider::default();
    let agent = AgentBuilder::new(provider.clone())
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_tool(TestTool("document_understand"))
        .with_tool(TestTool("pdf_parse"))
        .with_tool(TestTool("text_extract"))
        .with_tool(TestTool("tool_search"))
        .with_tool(TestTool("runtime_surface"))
        .with_enabled_tools(Arc::new(RwLock::new(HashSet::from([
            "document_understand".to_string(),
            "pdf_parse".to_string(),
            "text_extract".to_string(),
            "tool_search".to_string(),
            "runtime_surface".to_string(),
        ]))))
        .build()
        .unwrap();

    let mut prior_assistant = Message::assistant("上一轮本地多模态已给出后续策略".to_string());
    prior_assistant.metadata.insert(
        "provider_media_preprocess_followup_strategies".to_string(),
        "extract_video_frames:alternate_model_fallback".to_string(),
    );

    agent
        .stream_chat(vec![
            prior_assistant,
            Message::user("继续处理这个视频".to_string()),
        ])
        .await
        .unwrap();

    let request = provider
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    let tool_names: HashSet<_> = request.tools.into_iter().map(|tool| tool.name).collect();

    assert!(tool_names.contains("document_understand"));
    assert!(tool_names.contains("pdf_parse"));
    assert!(tool_names.contains("text_extract"));
    assert!(tool_names.contains("tool_search"));
    assert!(!tool_names.contains("runtime_surface"));

    let extra = request.extra_params.expect("extra params");
    assert_eq!(
        extra.get("capability_route").and_then(|v| v.as_str()),
        Some("document_understanding")
    );
    assert_eq!(
        extra
            .get("preferred_capability_domain")
            .and_then(|v| v.as_str()),
        Some("document_understanding")
    );
    assert_eq!(
        extra
            .get("media_followup_execution_surface")
            .and_then(|v| v.as_str()),
        Some("document_understanding_alternate_model_fallback")
    );
}

#[test]
fn runtime_stage_traces_reflect_real_emitted_signals() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .build()
        .unwrap();

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-123".to_string()),
        thread_id: "session-123".to_string(),
    };

    agent.reset_runtime_hook_state(&seed);
    agent.emit_runtime_stage(
        &seed,
        RuntimeStage::Ingress,
        TraceStatus::Succeeded,
        Some("input accepted".to_string()),
    );
    agent.emit_runtime_stage(
        &seed,
        RuntimeStage::Reasoning,
        TraceStatus::Started,
        Some("strategy=react".to_string()),
    );
    agent.emit_runtime_stage(
        &seed,
        RuntimeStage::Reasoning,
        TraceStatus::Succeeded,
        Some("thoughts=1".to_string()),
    );

    let traces = agent.build_runtime_stage_traces(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec!["thought".to_string()],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        Utc::now(),
        &std::collections::HashMap::new(),
    );

    assert_eq!(traces.len(), 2);
    assert_eq!(traces[0].stage, RuntimeStage::Ingress);
    assert_eq!(traces[0].status, TraceStatus::Succeeded);
    assert_eq!(traces[1].stage, RuntimeStage::Reasoning);
    assert_eq!(traces[1].status, TraceStatus::Succeeded);
    assert_eq!(
        traces[1].metadata.get("signal_count"),
        Some(&"2".to_string())
    );
}

#[test]
fn runtime_stage_traces_include_stage_specific_runtime_metadata() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_extra_params(serde_json::json!({
            "tactical_slm_present": true,
            "tactical_slm_model_id": "api:openai/test-small-model",
            "tactical_slm_factory_id": "cloud_llm",
            "tactical_slm_source": "cloud",
            "tactical_slm_roles": "llm,slm"
        }))
        .build()
        .unwrap();

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-123".to_string()),
        thread_id: "session-123".to_string(),
    };

    agent.reset_runtime_hook_state(&seed);
    agent.emit_runtime_stage(
        &seed,
        RuntimeStage::Reasoning,
        TraceStatus::Succeeded,
        Some("thoughts=1".to_string()),
    );
    agent.emit_runtime_stage(
        &seed,
        RuntimeStage::ToolPlanningFiltering,
        TraceStatus::Succeeded,
        Some("tool shortlist built".to_string()),
    );
    agent.emit_runtime_stage(
        &seed,
        RuntimeStage::Execution,
        TraceStatus::Succeeded,
        Some("tool finished".to_string()),
    );
    agent.emit_runtime_stage(
        &seed,
        RuntimeStage::Egress,
        TraceStatus::Succeeded,
        Some("response finalized".to_string()),
    );
    agent.emit_runtime_stage(
        &seed,
        RuntimeStage::TraceAudit,
        TraceStatus::Succeeded,
        Some("trace envelope prepared".to_string()),
    );

    let mut metadata = std::collections::HashMap::new();
    metadata.insert("tactical_slm_present".to_string(), "true".to_string());
    metadata.insert(
        "tactical_slm_model_id".to_string(),
        "api:openai/test-small-model".to_string(),
    );
    metadata.insert(
        "tactical_slm_factory_id".to_string(),
        "cloud_llm".to_string(),
    );
    metadata.insert("tactical_slm_source".to_string(), "cloud".to_string());
    metadata.insert("tactical_slm_roles".to_string(), "llm,slm".to_string());
    metadata.insert(
        "tactical_slm_contract_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert("provider_name".to_string(), "capture".to_string());
    metadata.insert("provider_model".to_string(), "gpt-4.1-mini".to_string());
    metadata.insert("provider_latency_ms".to_string(), "37".to_string());
    metadata.insert("provider_prompt_tokens".to_string(), "120".to_string());
    metadata.insert("provider_completion_tokens".to_string(), "45".to_string());
    metadata.insert("provider_total_tokens".to_string(), "165".to_string());
    metadata.insert(
        "provider_finish_reason".to_string(),
        "tool_calls".to_string(),
    );
    metadata.insert("provider_tool_call_count".to_string(), "1".to_string());
    metadata.insert(
        "provider_tool_contract_mode".to_string(),
        "tagged_json_tool_calls".to_string(),
    );
    metadata.insert(
        "provider_mainline_stability".to_string(),
        "stable".to_string(),
    );
    metadata.insert(
        "provider_surface_note_core_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "provider_surface_note_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "deferred_tool_filter_active".to_string(),
        "true".to_string(),
    );
    metadata.insert("deferred_tool_deferred_count".to_string(), "8".to_string());
    metadata.insert(
        "deferred_tool_surface_note_present".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "deferred_tool_surface_note_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "matched_skill_manuals".to_string(),
        "python_tooling".to_string(),
    );
    metadata.insert(
        "matched_skill_assets".to_string(),
        "references/setup.md".to_string(),
    );
    metadata.insert(
        "read_skill_manuals".to_string(),
        "python_tooling".to_string(),
    );
    metadata.insert(
        "skill_surface_classifications".to_string(),
        "python_tooling:executable".to_string(),
    );
    metadata.insert(
        "skill_surface_executions".to_string(),
        "python_tooling:runtime".to_string(),
    );
    metadata.insert(
        "skill_surface_runtimes".to_string(),
        "python_tooling:uv".to_string(),
    );
    metadata.insert(
        "skill_surface_kinds".to_string(),
        "python_tooling:tool".to_string(),
    );
    metadata.insert(
        "skill_surface_contract_happened".to_string(),
        "true".to_string(),
    );
    metadata.insert("skill_manual_gate_active".to_string(), "true".to_string());
    metadata.insert(
        "skill_loading_surface_note_core_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "skill_surface_contract_core_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert("tool_error_tools".to_string(), "web_search".to_string());
    metadata.insert(
        "tool_error_surface_tools".to_string(),
        "web_search".to_string(),
    );
    metadata.insert("tool_error_surface_present".to_string(), "true".to_string());
    metadata.insert(
        "tool_error_contract_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "forge_registered_tools".to_string(),
        "python_helper".to_string(),
    );
    metadata.insert("forge_source".to_string(), "forge".to_string());
    metadata.insert("forge_scope".to_string(), "session".to_string());
    metadata.insert(
        "forge_execution_surfaces".to_string(),
        "python_helper:runtime".to_string(),
    );
    metadata.insert(
        "forge_capability_domains".to_string(),
        "python_helper:runtime_surface".to_string(),
    );
    metadata.insert(
        "forge_smoke_statuses".to_string(),
        "python_helper:passed".to_string(),
    );
    metadata.insert(
        "forge_smoke_latency_ms".to_string(),
        "python_helper:42".to_string(),
    );
    metadata.insert(
        "forge_cleanup_recorded".to_string(),
        "python_helper:true".to_string(),
    );
    metadata.insert("forge_surface_present".to_string(), "true".to_string());
    metadata.insert("forge_contract_complete".to_string(), "true".to_string());
    metadata.insert(
        "forge_followup_candidates".to_string(),
        "python_helper".to_string(),
    );
    metadata.insert("forge_followup_gate_active".to_string(), "true".to_string());
    metadata.insert(
        "forge_followup_tools".to_string(),
        "python_helper".to_string(),
    );
    metadata.insert(
        "forge_followup_execution_happened".to_string(),
        "true".to_string(),
    );
    metadata.insert("forge_closed_loop_complete".to_string(), "true".to_string());
    metadata.insert(
        "skill_asset_followups".to_string(),
        "python_tooling:references/setup.md:shell".to_string(),
    );
    metadata.insert(
        "skill_asset_execution_surfaces".to_string(),
        "python_tooling:references/setup.md:runtime:shell".to_string(),
    );
    metadata.insert(
        "read_skill_assets".to_string(),
        "python_tooling:references/setup.md".to_string(),
    );
    metadata.insert("skill_asset_gate_active".to_string(), "true".to_string());
    metadata.insert("skill_asset_read_happened".to_string(), "true".to_string());
    metadata.insert(
        "skill_asset_followup_happened".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "skill_asset_execution_surface_happened".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "skill_loading_surface_note_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "skill_surface_contract_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert("visible_owner".to_string(), "benshu".to_string());
    metadata.insert("memory_owner".to_string(), "engram".to_string());
    metadata.insert("approval_owner".to_string(), "benshu".to_string());
    metadata.insert(
        "session_title".to_string(),
        "Python Tooling Session".to_string(),
    );
    metadata.insert(
        "session_title_source".to_string(),
        "extra_params.session_title".to_string(),
    );
    metadata.insert("session_title_present".to_string(), "true".to_string());
    metadata.insert(
        "memory_session_contract_core_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "memory_session_contract_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "memory_session_surface_core_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "memory_session_surface_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "memory_session_surface_note_present".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "memory_session_surface_note_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "runtime_evidence_contract_core_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert(
        "runtime_evidence_contract_complete".to_string(),
        "true".to_string(),
    );
    metadata.insert("runtime_finish_reason".to_string(), "stop".to_string());

    let traces = agent.build_runtime_stage_traces(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec!["thought".to_string()],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        Utc::now(),
        &metadata,
    );

    let reasoning = traces
        .iter()
        .find(|trace| trace.stage == RuntimeStage::Reasoning)
        .unwrap();
    assert_eq!(
        reasoning.metadata.get("provider_name"),
        Some(&"capture".to_string())
    );
    assert_eq!(
        reasoning.metadata.get("tactical_slm_factory_id"),
        Some(&"cloud_llm".to_string())
    );
    assert_eq!(
        reasoning.metadata.get("tactical_slm_source"),
        Some(&"cloud".to_string())
    );
    assert_eq!(
        reasoning.metadata.get("tactical_slm_roles"),
        Some(&"llm,slm".to_string())
    );
    assert_eq!(
        reasoning.metadata.get("tactical_slm_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        reasoning.metadata.get("provider_finish_reason"),
        Some(&"tool_calls".to_string())
    );
    assert_eq!(
        reasoning.metadata.get("provider_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        reasoning.metadata.get("provider_usage_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        reasoning.metadata.get("provider_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        reasoning
            .metadata
            .get("provider_surface_note_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        reasoning.metadata.get("provider_surface_note_complete"),
        Some(&"true".to_string())
    );

    let tool_planning = traces
        .iter()
        .find(|trace| trace.stage == RuntimeStage::ToolPlanningFiltering)
        .unwrap();
    assert_eq!(
        tool_planning.metadata.get("deferred_tool_filter_active"),
        Some(&"true".to_string())
    );
    assert_eq!(
        tool_planning.metadata.get("deferred_tool_deferred_count"),
        Some(&"8".to_string())
    );
    assert_eq!(
        tool_planning
            .metadata
            .get("deferred_tool_surface_note_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        tool_planning
            .metadata
            .get("deferred_tool_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        tool_planning
            .metadata
            .get("skill_loading_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        tool_planning
            .metadata
            .get("skill_surface_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        tool_planning
            .metadata
            .get("skill_loading_surface_note_core_complete"),
        Some(&"true".to_string())
    );

    let execution = traces
        .iter()
        .find(|trace| trace.stage == RuntimeStage::Execution)
        .unwrap();
    assert_eq!(
        execution.metadata.get("skill_asset_followups"),
        Some(&"python_tooling:references/setup.md:shell".to_string())
    );
    assert_eq!(
        execution.metadata.get("skill_surface_runtimes"),
        Some(&"python_tooling:uv".to_string())
    );
    assert_eq!(
        execution.metadata.get("skill_surface_contract_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution.metadata.get("skill_asset_execution_surfaces"),
        Some(&"python_tooling:references/setup.md:runtime:shell".to_string())
    );
    assert_eq!(
        execution.metadata.get("tool_error_surface_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution.metadata.get("tool_error_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution.metadata.get("forge_registered_tools"),
        Some(&"python_helper".to_string())
    );
    assert_eq!(
        execution.metadata.get("forge_source"),
        Some(&"forge".to_string())
    );
    assert_eq!(
        execution.metadata.get("forge_scope"),
        Some(&"session".to_string())
    );
    assert_eq!(
        execution.metadata.get("forge_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution.metadata.get("forge_followup_tools"),
        Some(&"python_helper".to_string())
    );
    assert_eq!(
        execution.metadata.get("forge_followup_execution_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution.metadata.get("forge_closed_loop_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution.metadata.get("skill_asset_followup_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution
            .metadata
            .get("skill_asset_execution_surface_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution.metadata.get("skill_loading_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution.metadata.get("skill_surface_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        execution
            .metadata
            .get("skill_loading_surface_note_complete"),
        Some(&"true".to_string())
    );

    let egress = traces
        .iter()
        .find(|trace| trace.stage == RuntimeStage::Egress)
        .unwrap();
    assert_eq!(
        egress.metadata.get("runtime_finish_reason"),
        Some(&"stop".to_string())
    );

    let trace_audit = traces
        .iter()
        .find(|trace| trace.stage == RuntimeStage::TraceAudit)
        .unwrap();
    assert_eq!(
        trace_audit
            .metadata
            .get("runtime_evidence_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace_audit
            .metadata
            .get("runtime_evidence_contract_complete"),
        Some(&"true".to_string())
    );

    if let Some(persistence) = traces
        .iter()
        .find(|trace| trace.stage == RuntimeStage::PersistenceMemory)
    {
        assert_eq!(
            persistence
                .metadata
                .get("memory_session_contract_core_complete"),
            Some(&"true".to_string())
        );
        assert_eq!(
            persistence.metadata.get("memory_session_contract_complete"),
            Some(&"true".to_string())
        );
        assert_eq!(
            persistence
                .metadata
                .get("memory_session_surface_core_complete"),
            Some(&"true".to_string())
        );
        assert_eq!(
            persistence.metadata.get("memory_session_surface_complete"),
            Some(&"true".to_string())
        );
        assert_eq!(
            persistence
                .metadata
                .get("memory_session_surface_note_present"),
            Some(&"true".to_string())
        );
        assert_eq!(
            persistence
                .metadata
                .get("memory_session_surface_note_complete"),
            Some(&"true".to_string())
        );
        assert_eq!(
            persistence
                .metadata
                .get("memory_session_orchestration_contract_core_complete"),
            Some(&"true".to_string())
        );
        assert_eq!(
            persistence
                .metadata
                .get("memory_session_orchestration_contract_complete"),
            Some(&"true".to_string())
        );
    }
}

#[test]
fn collect_dangling_tool_call_ids_finds_unmatched_calls() {
    let messages = vec![
        Message::assistant(Content::Parts(vec![
            crate::agent::message::ContentPart::Text {
                text: "planning".to_string(),
            },
            crate::agent::message::ContentPart::ToolCall {
                id: "call_1".to_string(),
                name: "web_search".to_string(),
                arguments: serde_json::json!({ "query": "btc" }),
            },
            crate::agent::message::ContentPart::ToolCall {
                id: "call_2".to_string(),
                name: "web_fetch".to_string(),
                arguments: serde_json::json!({ "url": "https://example.com" }),
            },
        ])),
        Message::tool_result("call_1", "ok").with_tool_name("web_search"),
    ];

    let dangling =
        Agent::<crate::agent::provider::MockProvider>::collect_dangling_tool_call_ids(&messages);
    assert_eq!(dangling, vec!["call_2".to_string()]);
}

#[test]
fn build_run_trace_surfaces_skill_loading_metadata() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_extra_params(serde_json::json!({
            "session_title": "Python Tooling Session",
            "tactical_slm_present": true,
            "tactical_slm_model_id": "api:openai/test-small-model",
            "tactical_slm_factory_id": "cloud_llm",
            "tactical_slm_source": "cloud",
            "tactical_slm_roles": "llm,slm"
        }))
        .build()
        .unwrap();

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-123".to_string()),
        thread_id: "session-123".to_string(),
    };

    {
        let mut capture = agent.runtime_hook_capture.write();
        capture.skill_manual_read_count = 1;
        capture.skill_asset_read_count = 1;
        capture.memory_surface_count = 1;
        capture.subagent_surface_count = 1;
        capture.title_surface_count = 1;
        capture.summarization_surface_count = 1;
        capture
            .notes
            .push("before_llm:skill_manual:python_tooling".to_string());
        capture
            .notes
            .push("before_llm:skill_manual_gate_active".to_string());
        capture
            .notes
            .push("skill_manual_read:python_tooling".to_string());
        capture
            .notes
            .push("skill_surface_classification:python_tooling:executable".to_string());
        capture
            .notes
            .push("skill_surface_execution:python_tooling:runtime".to_string());
        capture
            .notes
            .push("skill_surface_runtime:python_tooling:uv".to_string());
        capture
            .notes
            .push("skill_surface_kind:python_tooling:tool".to_string());
        capture
            .notes
            .push("before_llm:skill_asset:references/setup.md".to_string());
        capture
            .notes
            .push("before_llm:skill_asset_gate_active".to_string());
        capture
            .notes
            .push("skill_asset_read:python_tooling:references/setup.md".to_string());
        capture
            .notes
            .push("skill_asset_followup:python_tooling:references/setup.md:shell".to_string());
        capture.notes.push(
            "skill_asset_execution_surface:python_tooling:references/setup.md:runtime:shell"
                .to_string(),
        );
    }

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec!["thought".to_string()],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[],
    );

    assert_eq!(
        trace.metadata.get("matched_skill_manuals"),
        Some(&"python_tooling".to_string())
    );
    assert_eq!(
        trace.metadata.get("read_skill_manuals"),
        Some(&"python_tooling".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_surface_classifications"),
        Some(&"python_tooling:executable".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_surface_executions"),
        Some(&"python_tooling:runtime".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_surface_runtimes"),
        Some(&"python_tooling:uv".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_surface_kinds"),
        Some(&"python_tooling:tool".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_surface_contract_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_manual_gate_active"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_manual_read_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("read_skill_assets"),
        Some(&"python_tooling:references/setup.md".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_asset_read_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_asset_followups"),
        Some(&"python_tooling:references/setup.md:shell".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_asset_execution_surfaces"),
        Some(&"python_tooling:references/setup.md:runtime:shell".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_asset_followup_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_asset_execution_surface_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_loading_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_surface_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_loading_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_surface_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("skill_loading_surface_note_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_loading_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("session_title"),
        Some(&"Python Tooling Session".to_string())
    );
    assert_eq!(
        trace.metadata.get("session_title_source"),
        Some(&"extra_params.session_title".to_string())
    );
    assert_eq!(
        trace.metadata.get("session_title_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("tactical_slm_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("tactical_slm_model_id"),
        Some(&"api:openai/test-small-model".to_string())
    );
    assert_eq!(
        trace.metadata.get("tactical_slm_factory_id"),
        Some(&"cloud_llm".to_string())
    );
    assert_eq!(
        trace.metadata.get("tactical_slm_source"),
        Some(&"cloud".to_string())
    );
    assert_eq!(
        trace.metadata.get("tactical_slm_roles"),
        Some(&"llm,slm".to_string())
    );
    assert_eq!(
        trace.metadata.get("tactical_slm_contract_complete"),
        Some(&"true".to_string())
    );
    {
        let mut capture = agent.runtime_hook_capture.write();
        capture
            .notes
            .push("before_llm:deferred_tool_filter:6/14:deferred=8".to_string());
        capture
            .notes
            .push("before_llm:ownership:visible=benshu:memory=engram:approval=benshu".to_string());
        capture
            .notes
            .push("tool_error:web_search:network timeout".to_string());
        capture
            .notes
            .push("tool_error_surface:web_search".to_string());
        capture
            .notes
            .push("tool_degradation:web_fetch:tool_output_truncated".to_string());
        capture
            .notes
            .push("loop_guard:shell:repeated invocation".to_string());
        capture.notes.push(
            "before_response:subagent_budget:delegation=false:handover=false:parallel_tools=4"
                .to_string(),
        );
        capture
            .notes
            .push("after_llm:finish:tool_calls".to_string());
        capture
            .notes
            .push("after_llm:provider:capture:gpt-4.1-mini".to_string());
        capture
            .notes
            .push("after_llm:provider_latency_ms:37".to_string());
        capture
            .notes
            .push("after_llm:provider_prompt_tokens:120".to_string());
        capture
            .notes
            .push("after_llm:provider_completion_tokens:45".to_string());
        capture
            .notes
            .push("after_llm:provider_total_tokens:165".to_string());
        capture
            .notes
            .push("after_llm:provider_finish_reason:tool_calls".to_string());
        capture
            .notes
            .push("after_llm:provider_tool_call_count:1".to_string());
        capture
            .notes
            .push("after_llm:provider_tool_contract_mode:tagged_json_tool_calls".to_string());
        capture
            .notes
            .push("after_llm:provider_mainline_stability:stable".to_string());
        capture
            .notes
            .push("post_run_eval:thoughts=1,tool_calls=0".to_string());
        capture.notes.push(
            "before_response:title:present=true:source=extra_params.session_title:value=Runtime audit"
                .to_string(),
        );
        capture.notes.push(
            "before_response:memory_session_surface:visible=benshu:memory=engram:approval=benshu:title_present=true:title_source=extra_params.session_title:summary_present=true"
                .to_string(),
        );
    }

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec!["thought".to_string()],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[],
    );

    assert_eq!(
        trace.metadata.get("deferred_tool_filter_active"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("deferred_tool_visible_count"),
        Some(&"6".to_string())
    );
    assert_eq!(
        trace.metadata.get("deferred_tool_total_count"),
        Some(&"14".to_string())
    );
    assert_eq!(
        trace.metadata.get("deferred_tool_deferred_count"),
        Some(&"8".to_string())
    );
    assert_eq!(
        trace.metadata.get("deferred_tool_surface_note_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("deferred_tool_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("tool_error_tools"),
        Some(&"web_search".to_string())
    );
    assert_eq!(
        trace.metadata.get("tool_error_surface_tools"),
        Some(&"web_search".to_string())
    );
    assert_eq!(
        trace.metadata.get("tool_error_surface_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("tool_error_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("degraded_tool_names"),
        Some(&"web_fetch".to_string())
    );
    assert_eq!(
        trace.metadata.get("loop_guard_tools"),
        Some(&"shell".to_string())
    );
    assert_eq!(
        trace.metadata.get("runtime_finish_reason"),
        Some(&"tool_calls".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_name"),
        Some(&"capture".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_model"),
        Some(&"gpt-4.1-mini".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_latency_ms"),
        Some(&"37".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_prompt_tokens"),
        Some(&"120".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_completion_tokens"),
        Some(&"45".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_total_tokens"),
        Some(&"165".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_finish_reason"),
        Some(&"tool_calls".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_tool_call_count"),
        Some(&"1".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_tool_contract_mode"),
        Some(&"tagged_json_tool_calls".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_mainline_stability"),
        Some(&"stable".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_usage_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_surface_note_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("memory_session_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("memory_session_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("memory_session_surface_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("memory_session_surface_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("memory_session_surface_note_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("memory_session_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("subagent_budget_surface_note_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("subagent_budget_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("title_surface_note_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("title_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("summarization_surface_note_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("summarization_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("memory_session_orchestration_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("memory_session_orchestration_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("runtime_evidence_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("runtime_evidence_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("post_run_summary"),
        Some(&"thoughts=1,tool_calls=0".to_string())
    );
    assert_eq!(
        trace.metadata.get("max_parallel_tools"),
        Some(&"4".to_string())
    );
    assert_eq!(
        trace.metadata.get("memory_owner"),
        Some(&"engram".to_string())
    );
}

#[test]
fn build_run_trace_surfaces_clarification_metadata_from_history_messages() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .name("clarification-trace-agent")
        .session_id("session-clarify")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-clarify".to_string()),
        thread_id: "session-clarify".to_string(),
    };

    {
        let mut capture = agent.runtime_hook_capture.write();
        capture.clarification_surface_count = 1;
        capture.notes.push(
            "before_response:clarification_surface:status=awaiting_clarification:event=status_surface:prompt_present=true:original_present=true:json_valid=true"
                .to_string(),
        );
    }

    let mut clarification_record =
        Message::system("Session is waiting for clarification before continuing");
    clarification_record.metadata.insert(
        "session_status".to_string(),
        "awaiting_clarification".to_string(),
    );
    clarification_record.metadata.insert(
        "session_status_json".to_string(),
        serde_json::to_string(&SessionStatus::AwaitingClarification {
            clarification: "你想查哪个城市的天气？".to_string(),
            original_request: "帮我查一下天气".to_string(),
        })
        .expect("serialize session status"),
    );
    clarification_record.metadata.insert(
        "clarification_prompt".to_string(),
        "你想查哪个城市的天气？".to_string(),
    );
    clarification_record.metadata.insert(
        "clarification_original_request".to_string(),
        "帮我查一下天气".to_string(),
    );
    clarification_record.metadata.insert(
        "clarification_status_kind".to_string(),
        "awaiting_clarification".to_string(),
    );
    clarification_record.metadata.insert(
        "clarification_status_surface".to_string(),
        "true".to_string(),
    );

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[clarification_record],
    );

    assert_eq!(
        trace.metadata.get("session_status"),
        Some(&"awaiting_clarification".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_prompt"),
        Some(&"你想查哪个城市的天气？".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_original_request"),
        Some(&"帮我查一下天气".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_status_kind"),
        Some(&"awaiting_clarification".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_status_surface"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_event"),
        Some(&"status_surface".to_string())
    );
    assert!(trace.metadata.contains_key("session_status_json"));
    assert_eq!(
        trace
            .metadata
            .get("clarification_session_status_json_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("clarification_session_status_json_valid"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_awaiting_seen"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_terminal_event_seen"),
        None
    );
    assert_eq!(trace.metadata.get("clarification_roundtrip_complete"), None);
    assert_eq!(
        trace.metadata.get("hook_clarification_surface_count"),
        Some(&"1".to_string())
    );
}

#[test]
fn build_run_trace_surfaces_clarification_roundtrip_metadata() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .name("clarification-trace-agent")
        .session_id("session-clarify-roundtrip")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-clarify-roundtrip".to_string()),
        thread_id: "session-clarify-roundtrip".to_string(),
    };

    {
        let mut capture = agent.runtime_hook_capture.write();
        capture.clarification_surface_count = 1;
        capture.notes.push(
            "before_response:clarification_surface:status=thinking:event=resolved:prompt_present=true:original_present=true:json_valid=true"
                .to_string(),
        );
    }

    let mut waiting = Message::system("Session is waiting for clarification before continuing");
    waiting.metadata.insert(
        "session_status".to_string(),
        "awaiting_clarification".to_string(),
    );
    waiting.metadata.insert(
        "clarification_prompt".to_string(),
        "你想查哪个城市的天气？".to_string(),
    );
    waiting.metadata.insert(
        "clarification_original_request".to_string(),
        "帮我查一下天气".to_string(),
    );
    waiting.metadata.insert(
        "clarification_status_kind".to_string(),
        "awaiting_clarification".to_string(),
    );

    let mut resolved = Message::system("Clarification resolved; resuming request");
    resolved
        .metadata
        .insert("session_status".to_string(), "thinking".to_string());
    resolved
        .metadata
        .insert("clarification_resolved".to_string(), "true".to_string());
    resolved.metadata.insert(
        "clarification_prompt".to_string(),
        "你想查哪个城市的天气？".to_string(),
    );
    resolved.metadata.insert(
        "clarification_original_request".to_string(),
        "帮我查一下天气".to_string(),
    );
    resolved.metadata.insert(
        "clarification_status_kind".to_string(),
        "thinking".to_string(),
    );
    resolved.metadata.insert(
        "session_status_json".to_string(),
        serde_json::json!({
            "kind": "thinking"
        })
        .to_string(),
    );

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[waiting, resolved],
    );

    assert_eq!(
        trace.metadata.get("clarification_awaiting_seen"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_terminal_event_seen"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_roundtrip_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("hook_clarification_surface_count"),
        Some(&"1".to_string())
    );
}

#[test]
fn build_run_trace_surfaces_media_preprocess_metadata() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .name("media-preprocess-trace-agent")
        .session_id("session-media-preprocess")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-media-preprocess".to_string()),
        thread_id: "session-media-preprocess".to_string(),
    };

    {
        let mut capture = agent.runtime_hook_capture.write();
        capture.media_surface_count = 1;
        capture
            .notes
            .push("media_preprocess_tool:normalize_audio".to_string());
        capture
            .notes
            .push("media_preprocess_status:normalize_audio:ok".to_string());
        capture
            .notes
            .push("media_preprocess_kind:normalize_audio:audio".to_string());
        capture
            .notes
            .push("media_preprocess_input:normalize_audio:/tmp/input.wav".to_string());
        capture
            .notes
            .push("media_preprocess_output:normalize_audio:file:/tmp/output.wav".to_string());
        capture
            .notes
            .push("media_preprocess_source_kind:image_page_raster:direct_image".to_string());
        capture
            .notes
            .push("media_preprocess_source_ref:image_page_raster:/tmp/screenshot.png".to_string());
        capture.notes.push(
            "media_preprocess_source_kind:pdf_parse_tool:page_image_ocr:pdf_page_image".to_string(),
        );
        capture.notes.push(
            "media_preprocess_source_ref:pdf_parse_tool:page_image_ocr:pdf_page:3".to_string(),
        );
        capture
            .notes
            .push("media_preprocess_engine:normalize_audio:ffmpeg".to_string());
        capture
            .notes
            .push("media_preprocess_cleanup:normalize_audio:false".to_string());
        capture
            .notes
            .push("media_preprocess_artifact_registered:normalize_audio:true".to_string());
        capture.notes.push(
            "media_preprocess_artifact_source_kind:normalize_audio:builtin_tool_output".to_string(),
        );
        capture.notes.push(
            "media_preprocess_artifact_kind:normalize_audio:normalized_audio_output".to_string(),
        );
        capture
            .notes
            .push("media_preprocess_artifact_uri:normalize_audio:/tmp/output.wav".to_string());
        capture
            .notes
            .push("media_preprocess_consumed_by:normalize_audio:stt".to_string());
        capture
            .notes
            .push("media_preprocess_consumed_by:image_page_raster:ocr".to_string());
        capture.notes.push(
            "media_preprocess_consumption_route:normalize_audio:media_runtime_audio_stt"
                .to_string(),
        );
        capture
            .notes
            .push("media_preprocess_consumption_route:image_page_raster:ocr_backend".to_string());
        capture
            .notes
            .push("media_preprocess_outcome:extract_video_frames:preprocess_failed".to_string());
        capture
            .notes
            .push("media_preprocess_preprocess_failed:extract_video_frames".to_string());
        capture.notes.push(
            "media_preprocess_outcome:image_page_raster:model_result_insufficient".to_string(),
        );
        capture
            .notes
            .push("media_preprocess_result_insufficient:image_page_raster".to_string());
        capture.notes.push(
            "media_preprocess_followup_strategy:extract_video_frames:attachment_fallback"
                .to_string(),
        );
        capture.notes.push(
            "before_llm:media_followup_strategies:extract_video_frames:attachment_fallback,image_page_raster:clarification_or_manual_review,normalize_audio:alternate_model_fallback"
                .to_string(),
        );
        capture
            .notes
            .push("before_llm:media_followup_capability_route:document_understanding".to_string());
        capture.notes.push(
            "before_llm:media_followup_execution_surface:document_understanding_alternate_model_fallback"
                .to_string(),
        );
        capture
            .notes
            .push("before_llm:media_followup_guidance_active".to_string());
        capture
            .notes
            .push("media_preprocess_strategy_attachment_fallback:extract_video_frames".to_string());
        capture.notes.push(
            "media_preprocess_followup_strategy:image_page_raster:clarification_or_manual_review"
                .to_string(),
        );
        capture
            .notes
            .push("media_preprocess_strategy_clarification:image_page_raster".to_string());
        capture.notes.push(
            "media_preprocess_outcome:normalize_audio:model_failed_after_preprocess".to_string(),
        );
        capture
            .notes
            .push("media_preprocess_model_failed:normalize_audio".to_string());
        capture.notes.push(
            "media_preprocess_followup_strategy:normalize_audio:alternate_model_fallback"
                .to_string(),
        );
        capture
            .notes
            .push("media_preprocess_strategy_alternate_model_fallback:normalize_audio".to_string());
        capture.notes.push(
            "after_llm:provider_media_preprocess_consumed_by:normalize_audio:stt,extract_video_frames:vlm".to_string(),
        );
        capture.notes.push(
            "after_llm:provider_media_preprocess_consumption_routes:normalize_audio:native_local_stt,extract_video_frames:native_provider_vision".to_string(),
        );
        capture.notes.push(
            "after_llm:provider_media_preprocess_outcomes:extract_video_frames:model_failed_after_preprocess,normalize_audio:model_result_insufficient".to_string(),
        );
        capture.notes.push(
            "after_llm:provider_media_preprocess_model_failed_routes:extract_video_frames"
                .to_string(),
        );
        capture.notes.push(
            "after_llm:provider_media_preprocess_result_insufficient_routes:normalize_audio"
                .to_string(),
        );
        capture.notes.push(
            "after_llm:provider_media_preprocess_followup_strategies:extract_video_frames:alternate_model_fallback,normalize_audio:clarification_or_manual_review".to_string(),
        );
        capture.notes.push(
            "after_llm:provider_media_preprocess_alternate_model_fallback_routes:extract_video_frames".to_string(),
        );
        capture.notes.push(
            "after_llm:provider_media_preprocess_clarification_routes:normalize_audio".to_string(),
        );
    }

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[],
    );

    assert_eq!(
        trace.metadata.get("hook_media_surface_count"),
        Some(&"1".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_tools"),
        Some(&"normalize_audio".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_statuses"),
        Some(&"normalize_audio:ok".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_outputs"),
        Some(&"normalize_audio:file:/tmp/output.wav".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_source_kinds"),
        Some(
            &"image_page_raster:direct_image,pdf_parse_tool:page_image_ocr:pdf_page_image"
                .to_string()
        )
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_source_refs"),
        Some(
            &"image_page_raster:/tmp/screenshot.png,pdf_parse_tool:page_image_ocr:pdf_page:3"
                .to_string()
        )
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("media_preprocess_artifact_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("media_preprocess_artifact_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("media_preprocess_consumption_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_consumed_by"),
        Some(&"image_page_raster:ocr,normalize_audio:stt".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_consumption_routes"),
        Some(&"image_page_raster:ocr_backend,normalize_audio:media_runtime_audio_stt".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_outcomes"),
        Some(
            &"extract_video_frames:preprocess_failed,image_page_raster:model_result_insufficient,normalize_audio:model_failed_after_preprocess"
                .to_string()
        )
    );
    assert_eq!(
        trace
            .metadata
            .get("media_preprocess_preprocess_failed_routes"),
        Some(&"extract_video_frames".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_model_failed_routes"),
        Some(&"normalize_audio".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("media_preprocess_result_insufficient_routes"),
        Some(&"image_page_raster".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("media_preprocess_outcome_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_followup_strategies"),
        Some(
            &"extract_video_frames:attachment_fallback,image_page_raster:clarification_or_manual_review,normalize_audio:alternate_model_fallback"
                .to_string()
        )
    );
    assert_eq!(
        trace.metadata.get("media_followup_strategies"),
        Some(
            &"extract_video_frames:attachment_fallback,image_page_raster:clarification_or_manual_review,normalize_audio:alternate_model_fallback"
                .to_string()
        )
    );
    assert_eq!(
        trace.metadata.get("media_followup_guidance_active"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_followup_capability_route"),
        Some(&"document_understanding".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_followup_execution_surface"),
        Some(&"document_understanding_alternate_model_fallback".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("media_preprocess_attachment_fallback_routes"),
        Some(&"extract_video_frames".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("media_preprocess_alternate_model_fallback_routes"),
        Some(&"normalize_audio".to_string())
    );
    assert_eq!(
        trace.metadata.get("media_preprocess_clarification_routes"),
        Some(&"image_page_raster".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("media_preprocess_strategy_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_media_preprocess_consumed_by"),
        Some(&"extract_video_frames:vlm,normalize_audio:stt".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("provider_media_preprocess_consumption_routes"),
        Some(
            &"extract_video_frames:native_provider_vision,normalize_audio:native_local_stt"
                .to_string()
        )
    );
    assert_eq!(
        trace.metadata.get("provider_media_preprocess_outcomes"),
        Some(
            &"extract_video_frames:model_failed_after_preprocess,normalize_audio:model_result_insufficient"
                .to_string()
        )
    );
    assert_eq!(
        trace
            .metadata
            .get("provider_media_preprocess_model_failed_routes"),
        Some(&"extract_video_frames".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("provider_media_preprocess_result_insufficient_routes"),
        Some(&"normalize_audio".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_media_preprocess_followup_strategies"),
        Some(
            &"extract_video_frames:alternate_model_fallback,normalize_audio:clarification_or_manual_review"
                .to_string()
        )
    );
    assert_eq!(
        trace
            .metadata
            .get("provider_media_preprocess_alternate_model_fallback_routes"),
        Some(&"extract_video_frames".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("provider_media_preprocess_clarification_routes"),
        Some(&"normalize_audio".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("provider_media_preprocess_outcome_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("provider_media_preprocess_strategy_contract_complete"),
        Some(&"true".to_string())
    );
}

#[test]
fn build_run_trace_surfaces_windows_native_runtime_metadata() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .name("windows-native-trace-agent")
        .session_id("session-windows-native")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-windows-native".to_string()),
        thread_id: "session-windows-native".to_string(),
    };

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[],
    );

    assert_eq!(
        trace.metadata.get("windows_native_product_mainline"),
        Some(&"windows_native_mainline".to_string())
    );
    assert_eq!(
        trace.metadata.get("windows_native_deployment_lane"),
        Some(&"validation_only".to_string())
    );
    assert_eq!(
        trace.metadata.get("windows_native_deployment_strategy"),
        Some(&"switch_to_windows_native_host".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("windows_native_small_model_runtime_target"),
        Some(&"onnx_runtime_directml_winml".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("windows_native_small_model_execution_provider"),
        Some(&"validation_only".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("windows_native_small_model_device_target"),
        Some(&"windows_native_accelerator".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("windows_native_small_model_fallback_mode"),
        Some(&"validation_only".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("windows_native_small_model_runtime_outcome"),
        Some(&"validation_only".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("windows_native_small_model_runtime_strategy"),
        Some(&"validation_host_only".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("windows_native_runtime_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("windows_native_runtime_surface_note_complete"),
        Some(&"true".to_string())
    );
}

#[test]
fn build_run_trace_surfaces_engram_windows_native_runtime_metadata() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .name("engram-windows-native-trace-agent")
        .session_id("session-engram-windows-native")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    {
        let mut capture = agent.runtime_hook_capture.write();
        capture.notes.push(
            "before_response:engram_windows_native_embed_outcome:fallback_runtime_active"
                .to_string(),
        );
        capture
            .notes
            .push("before_response:engram_windows_native_embed_class:fallback_runtime".to_string());
        capture.notes.push(
            "before_response:engram_windows_native_embed_provider:directml_winml".to_string(),
        );
        capture.notes.push(
            "before_response:engram_windows_native_embed_device_target:windows_native_accelerator"
                .to_string(),
        );
        capture.notes.push(
            "before_response:engram_windows_native_embed_fallback_mode:cpu_fallback_with_explicit_reason"
                .to_string(),
        );
        capture.notes.push(
            "before_response:engram_windows_native_embed_strategy:migrate_to_windows_native_runtime"
                .to_string(),
        );
        capture.notes.push(
            "before_response:engram_windows_native_embed_note:Embedding currently runs through the fallback runtime."
                .to_string(),
        );
        capture.notes.push(
            "before_response:engram_windows_native_rerank_outcome:windows_native_active"
                .to_string(),
        );
        capture
            .notes
            .push("before_response:engram_windows_native_rerank_class:active".to_string());
        capture.notes.push(
            "before_response:engram_windows_native_rerank_provider:directml_winml".to_string(),
        );
        capture.notes.push(
            "before_response:engram_windows_native_rerank_device_target:windows_native_accelerator"
                .to_string(),
        );
        capture.notes.push(
            "before_response:engram_windows_native_rerank_fallback_mode:cpu_fallback_with_explicit_reason"
                .to_string(),
        );
        capture
            .notes
            .push("before_response:engram_windows_native_rerank_strategy:active".to_string());
        capture.notes.push(
            "before_response:engram_windows_native_rerank_note:Rerank executed through the Windows-native small-model runtime."
                .to_string(),
        );
        capture.notes.push(
            "before_response:engram_windows_native_surface:embed_present=true:rerank_present=true"
                .to_string(),
        );
    }

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-engram-windows-native".to_string()),
        thread_id: "session-engram-windows-native".to_string(),
    };

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[],
    );

    assert_eq!(
        trace.metadata.get("engram_windows_native_embed_outcome"),
        Some(&"fallback_runtime_active".to_string())
    );
    assert_eq!(
        trace.metadata.get("engram_windows_native_embed_class"),
        Some(&"fallback_runtime".to_string())
    );
    assert_eq!(
        trace.metadata.get("engram_windows_native_embed_provider"),
        Some(&"directml_winml".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("engram_windows_native_embed_device_target"),
        Some(&"windows_native_accelerator".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("engram_windows_native_embed_fallback_mode"),
        Some(&"cpu_fallback_with_explicit_reason".to_string())
    );
    assert_eq!(
        trace.metadata.get("engram_windows_native_embed_strategy"),
        Some(&"migrate_to_windows_native_runtime".to_string())
    );
    assert_eq!(
        trace.metadata.get("engram_windows_native_rerank_outcome"),
        Some(&"windows_native_active".to_string())
    );
    assert_eq!(
        trace.metadata.get("engram_windows_native_rerank_class"),
        Some(&"active".to_string())
    );
    assert_eq!(
        trace.metadata.get("engram_windows_native_rerank_provider"),
        Some(&"directml_winml".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("engram_windows_native_rerank_device_target"),
        Some(&"windows_native_accelerator".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("engram_windows_native_rerank_fallback_mode"),
        Some(&"cpu_fallback_with_explicit_reason".to_string())
    );
    assert_eq!(
        trace.metadata.get("engram_windows_native_rerank_strategy"),
        Some(&"active".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("engram_windows_native_surface_note_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("engram_windows_native_surface_note_complete"),
        Some(&"true".to_string())
    );
}

#[test]
fn build_run_trace_surfaces_truth_verification_metadata() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .name("truth-verification-trace-agent")
        .session_id("session-truth-verification")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    {
        let mut capture = agent.runtime_hook_capture.write();
        capture
            .notes
            .push("before_response:truth_status:Verified".to_string());
        capture
            .notes
            .push("before_response:verification_domain:KnowledgeFact".to_string());
        capture
            .notes
            .push("before_response:verification_requirement:Required".to_string());
        capture
            .notes
            .push("before_response:verification_mode:WebSearchFetch".to_string());
        capture
            .notes
            .push("before_response:verification_outcome:VerificationSucceeded".to_string());
        capture
            .notes
            .push("before_response:verification_answer_readiness:search_results_only".to_string());
        capture
            .notes
            .push("before_response:verification_route_reason:external_fact_requires_search_then_source_read".to_string());
        capture
            .notes
            .push("before_response:verification_continuation:ContinueFetchOrBrowse".to_string());
        capture
            .notes
            .push("before_response:verification_termination:TentativeOnly".to_string());
        capture
            .notes
            .push("before_response:verification_requires_followup:true".to_string());
        capture
            .notes
            .push("before_response:verification_can_finalize_answer:false".to_string());
        capture
            .notes
            .push("before_response:verification_next_tools:web_fetch".to_string());
        capture
            .notes
            .push("before_response:verification_cite_required:true".to_string());
        capture
            .notes
            .push("before_response:verification_followup_note:Search results were observed, but source pages were not fetched yet.".to_string());
        capture.notes.push(format!(
            "before_response:verification_sources_json:{}",
            serde_json::to_string(&vec![json!({
                "kind": "web_page",
                "title": "OpenAI Pricing",
                "uri": "https://openai.com/api/pricing",
                "observed_at": "2026-03-30T00:00:00Z"
            })])
            .expect("verification sources should serialize")
        ));
        capture.notes.push(format!(
            "before_response:verification_execution_evidence_json:{}",
            serde_json::to_string(&vec!["command=git status exit=0".to_string()])
                .expect("verification execution evidence should serialize")
        ));
        capture.notes.push(format!(
            "before_response:verification_state_evidence_json:{}",
            serde_json::to_string(&vec![
                "runtime=quickjs available=true".to_string(),
                "source=embedded".to_string()
            ])
            .expect("verification state evidence should serialize")
        ));
        capture
            .notes
            .push("before_response:source_posture:SourcesAttached".to_string());
        capture
            .notes
            .push("before_response:verification_last_tool:web_search".to_string());
        capture
            .notes
            .push("before_response:verification_tools:web_fetch,web_search".to_string());
        capture
            .notes
            .push("before_llm:truth_verification_guidance_active".to_string());
        capture
            .notes
            .push("before_response:verification_source_count:2".to_string());
        capture
            .notes
            .push("before_response:verification_execution_evidence_count:1".to_string());
        capture
            .notes
            .push("before_response:verification_state_evidence_count:2".to_string());
        capture
            .notes
            .push("before_response:verification_note_count:1".to_string());
        capture.notes.push(
            "before_response:verification_surface:tools=web_fetch,web_search:count=2:latest_tool=web_search:complete=true"
                .to_string(),
        );
    }

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-truth-verification".to_string()),
        thread_id: "session-truth-verification".to_string(),
    };

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[],
    );

    assert_eq!(
        trace.metadata.get("truth_status"),
        Some(&"Verified".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_domain"),
        Some(&"KnowledgeFact".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_requirement"),
        Some(&"Required".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_mode"),
        Some(&"WebSearchFetch".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_outcome"),
        Some(&"VerificationSucceeded".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_answer_readiness"),
        Some(&"search_results_only".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_route_reason"),
        Some(&"external_fact_requires_search_then_source_read".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_continuation"),
        Some(&"ContinueFetchOrBrowse".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_termination"),
        Some(&"TentativeOnly".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_requires_followup"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_can_finalize_answer"),
        Some(&"false".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_next_tools"),
        Some(&"web_fetch".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_cite_required"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_sources_json"),
        Some(
            &serde_json::to_string(&vec![json!({
                "kind": "web_page",
                "title": "OpenAI Pricing",
                "uri": "https://openai.com/api/pricing",
                "observed_at": "2026-03-30T00:00:00Z"
            })])
            .expect("verification sources should serialize")
        )
    );
    assert_eq!(
        trace.metadata.get("verification_execution_evidence_json"),
        Some(
            &serde_json::to_string(&vec!["command=git status exit=0".to_string()])
                .expect("verification execution evidence should serialize")
        )
    );
    assert_eq!(
        trace.metadata.get("verification_state_evidence_json"),
        Some(
            &serde_json::to_string(&vec![
                "runtime=quickjs available=true".to_string(),
                "source=embedded".to_string()
            ])
            .expect("verification state evidence should serialize")
        )
    );
    assert_eq!(
        trace.metadata.get("source_posture"),
        Some(&"SourcesAttached".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_last_tool"),
        Some(&"web_search".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_tools"),
        Some(&"web_fetch,web_search".to_string())
    );
    assert_eq!(
        trace.metadata.get("truth_verification_guidance_active"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_source_count"),
        Some(&"2".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_execution_evidence_count"),
        Some(&"1".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_state_evidence_count"),
        Some(&"2".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_surface_note_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("verification_surface_note_complete"),
        Some(&"true".to_string())
    );
}

#[test]
fn runtime_middleware_chain_reaches_complete_runtime_evidence_contract() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .name("runtime-middleware-complete-agent")
        .session_id("session-runtime-complete")
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .with_extra_params(serde_json::json!({
            "session_title": "Runtime Middleware Complete"
        }))
        .build()
        .expect("agent should build");

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-runtime-complete".to_string()),
        thread_id: "session-runtime-complete".to_string(),
    };

    {
        let mut capture = agent.runtime_hook_capture.write();
        capture.skill_manual_read_count = 1;
        capture.skill_asset_read_count = 1;
        capture.memory_surface_count = 1;
        capture.subagent_surface_count = 1;
        capture.title_surface_count = 1;
        capture.summarization_surface_count = 1;
        capture.tool_error_count = 1;
        capture.clarification_surface_count = 1;

        capture
            .notes
            .push("before_llm:deferred_tool_filter:6/14:deferred=8".to_string());
        capture
            .notes
            .push("before_llm:ownership:visible=benshu:memory=engram:approval=benshu".to_string());
        capture
            .notes
            .push("before_llm:skill_manual:python_tooling".to_string());
        capture
            .notes
            .push("before_llm:skill_manual_gate_active".to_string());
        capture
            .notes
            .push("skill_manual_read:python_tooling".to_string());
        capture
            .notes
            .push("skill_surface_classification:python_tooling:executable".to_string());
        capture
            .notes
            .push("skill_surface_execution:python_tooling:runtime".to_string());
        capture
            .notes
            .push("skill_surface_runtime:python_tooling:uv".to_string());
        capture
            .notes
            .push("skill_surface_kind:python_tooling:tool".to_string());
        capture
            .notes
            .push("before_llm:skill_asset:references/setup.md".to_string());
        capture
            .notes
            .push("before_llm:skill_asset_gate_active".to_string());
        capture
            .notes
            .push("skill_asset_read:python_tooling:references/setup.md".to_string());
        capture
            .notes
            .push("skill_asset_followup:python_tooling:references/setup.md:shell".to_string());
        capture.notes.push(
            "skill_asset_execution_surface:python_tooling:references/setup.md:runtime:shell"
                .to_string(),
        );
        capture
            .notes
            .push("tool_error:web_search:network timeout".to_string());
        capture
            .notes
            .push("tool_error_surface:web_search".to_string());
        capture
            .notes
            .push("after_llm:finish:tool_calls".to_string());
        capture
            .notes
            .push("after_llm:provider:capture:gpt-4.1-mini".to_string());
        capture
            .notes
            .push("after_llm:provider_latency_ms:37".to_string());
        capture
            .notes
            .push("after_llm:provider_prompt_tokens:120".to_string());
        capture
            .notes
            .push("after_llm:provider_completion_tokens:45".to_string());
        capture
            .notes
            .push("after_llm:provider_total_tokens:165".to_string());
        capture
            .notes
            .push("after_llm:provider_finish_reason:tool_calls".to_string());
        capture
            .notes
            .push("after_llm:provider_tool_call_count:1".to_string());
        capture
            .notes
            .push("after_llm:provider_tool_contract_mode:tagged_json_tool_calls".to_string());
        capture
            .notes
            .push("after_llm:provider_mainline_stability:stable".to_string());
        capture.notes.push(
            "before_response:subagent_budget:delegation=false:handover=false:parallel_tools=4"
                .to_string(),
        );
        capture.notes.push(
            "before_response:title:present=true:source=extra_params.session_title:value=Runtime Middleware Complete"
                .to_string(),
        );
        capture.notes.push(
            "before_response:memory_session_surface:visible=benshu:memory=engram:approval=benshu:title_present=true:title_source=extra_params.session_title:summary_present=true"
                .to_string(),
        );
        capture
            .notes
            .push("post_run_eval:thoughts=1,tool_calls=0".to_string());
        capture.notes.push(
            "before_response:clarification_surface:status=thinking:event=resolved:prompt_present=true:original_present=true:json_valid=true"
                .to_string(),
        );
    }

    let mut waiting = Message::system("Session is waiting for clarification before continuing");
    waiting.metadata.insert(
        "session_status".to_string(),
        "awaiting_clarification".to_string(),
    );
    waiting.metadata.insert(
        "clarification_prompt".to_string(),
        "你想查哪个城市的天气？".to_string(),
    );
    waiting.metadata.insert(
        "clarification_original_request".to_string(),
        "帮我查一下天气".to_string(),
    );
    waiting.metadata.insert(
        "clarification_status_kind".to_string(),
        "awaiting_clarification".to_string(),
    );

    let mut resolved = Message::system("Clarification resolved; resuming request");
    resolved
        .metadata
        .insert("session_status".to_string(), "thinking".to_string());
    resolved
        .metadata
        .insert("clarification_resolved".to_string(), "true".to_string());
    resolved.metadata.insert(
        "clarification_prompt".to_string(),
        "你想查哪个城市的天气？".to_string(),
    );
    resolved.metadata.insert(
        "clarification_original_request".to_string(),
        "帮我查一下天气".to_string(),
    );
    resolved.metadata.insert(
        "clarification_status_kind".to_string(),
        "thinking".to_string(),
    );
    resolved.metadata.insert(
        "session_status_json".to_string(),
        serde_json::json!({
            "kind": "thinking"
        })
        .to_string(),
    );

    let outcome = ChatOutcome {
        response: "ok".to_string(),
        thoughts: vec!["thought".to_string()],
        tool_calls: vec![],
        metabolic_stats: None,
        ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
        delegation: None,
        handover: None,
        runtime_task: None,
        run_trace: None,
    };

    let trace = agent.build_run_trace(&seed, &outcome, &[waiting, resolved]);

    assert_eq!(
        trace.metadata.get("provider_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("provider_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_loading_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("skill_loading_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("clarification_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("memory_session_orchestration_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("deferred_tool_surface_note_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("tool_error_contract_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace
            .metadata
            .get("runtime_evidence_contract_core_complete"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("runtime_evidence_contract_complete"),
        Some(&"true".to_string())
    );
}

#[test]
fn build_run_trace_surfaces_forge_closed_loop_metadata() {
    let agent = AgentBuilder::new(crate::agent::provider::MockProvider::new("ok"))
        .with_security(Arc::new(MockSecurityHandler))
        .with_system_prompt("test")
        .build()
        .unwrap();

    let seed = RuntimeExecutionSeed {
        task_id: Uuid::nil(),
        run_id: Uuid::nil(),
        started_at: Utc::now(),
        session_id: Some("session-123".to_string()),
        thread_id: "session-123".to_string(),
    };

    {
        let mut capture = agent.runtime_hook_capture.write();
        capture.forge_surface_count = 1;
        capture
            .notes
            .push("forge_registered:pdf_builder".to_string());
        capture.notes.push("forge_source:forge".to_string());
        capture.notes.push("forge_scope:session".to_string());
        capture
            .notes
            .push("forge_execution_surface:pdf_builder:runtime".to_string());
        capture
            .notes
            .push("forge_smoke_status:pdf_builder:passed".to_string());
        capture
            .notes
            .push("forge_cleanup_recorded:pdf_builder:true".to_string());
        capture
            .notes
            .push("before_llm:forge_followup_tools:pdf_builder".to_string());
        capture
            .notes
            .push("before_llm:forge_followup_gate_active".to_string());
    }

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec!["thought".to_string()],
            tool_calls: vec![
                ToolCallData {
                    receipt_id: None,
                    tool_call_id: None,
                    name: "forge_skill".to_string(),
                    args: "{}".to_string(),
                    result: Some("{\"status\":\"success\"}".to_string()),
                    backup: None,
                    duration_ms: 5,
                    timestamp: 1,
                    caller_id: None,
                    safety_level: SafetyLevel::Green,
                    cpu_pressure: None,
                    vram_pressure: None,
                    result_truncated: false,
                    result_original_chars: None,
                    result_omitted_chars: None,
                    args_fingerprint: None,
                    result_fingerprint: None,
                    outcome: None,
                    replay: None,
                },
                ToolCallData {
                    receipt_id: None,
                    tool_call_id: None,
                    name: "pdf_builder".to_string(),
                    args: "{\"input\":\"report\"}".to_string(),
                    result: Some("built".to_string()),
                    backup: None,
                    duration_ms: 12,
                    timestamp: 2,
                    caller_id: None,
                    safety_level: SafetyLevel::Green,
                    cpu_pressure: None,
                    vram_pressure: None,
                    result_truncated: false,
                    result_original_chars: None,
                    result_omitted_chars: None,
                    args_fingerprint: None,
                    result_fingerprint: None,
                    outcome: None,
                    replay: None,
                },
            ],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[Message::system(
            "### FORGE_APPROVED\nforge request approved".to_string(),
        )],
    );

    assert_eq!(
        trace.metadata.get("forge_followup_candidates"),
        Some(&"pdf_builder".to_string())
    );
    assert_eq!(
        trace.metadata.get("forge_followup_gate_active"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("forge_followup_tools"),
        Some(&"pdf_builder".to_string())
    );
    assert_eq!(
        trace.metadata.get("forge_followup_execution_happened"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("forge_closed_loop_complete"),
        Some(&"true".to_string())
    );
}

#[test]
fn run_trace_projects_background_envelope_metadata() {
    let agent = AgentBuilder::new(MockProvider::new("ok"))
        .with_security(Arc::new(MockSecurityHandler))
        .with_session_id("background-trace-session")
        .build()
        .expect("agent should build");

    *agent.background_envelope.write() = Some(BackgroundEnvelope {
        relationship_layer: Some(RelationshipBackgroundLayer {
            relationship_summary: Some("用户把助手视作长期协作对象".to_string()),
            user_preferences: vec!["喜欢安静一点的交流方式".to_string()],
            ..Default::default()
        }),
        quality_signal: BackgroundQualitySignal::Stable,
        metadata: std::collections::HashMap::from([
            (
                "background_decision".to_string(),
                "promoterelationshipfact".to_string(),
            ),
            ("background_used_slm".to_string(), "false".to_string()),
            (
                "background_session_persistence_status".to_string(),
                "persisted".to_string(),
            ),
            ("durable_promotion_pending".to_string(), "false".to_string()),
            (
                "durable_promotion_status".to_string(),
                "pending_review".to_string(),
            ),
            ("background_total_attempts".to_string(), "3".to_string()),
            ("background_skip_count".to_string(), "1".to_string()),
            ("background_reject_count".to_string(), "0".to_string()),
            (
                "background_refresh_session_count".to_string(),
                "1".to_string(),
            ),
            (
                "background_promote_relationship_count".to_string(),
                "1".to_string(),
            ),
            ("background_rewrite_count".to_string(), "0".to_string()),
        ]),
        ..Default::default()
    });

    let seed = RuntimeExecutionSeed {
        run_id: Uuid::new_v4(),
        task_id: Uuid::new_v4(),
        session_id: Some("background-trace-session".to_string()),
        thread_id: "background-trace-thread".to_string(),
        started_at: Utc::now(),
    };
    let started_at = seed.started_at;
    let mut captured = agent.runtime_stage_capture.write();
    captured.push(RuntimeStageSignal {
        stage: RuntimeStage::PersistenceMemory,
        status: TraceStatus::Succeeded,
        at: started_at,
        detail: Some("background persisted".to_string()),
    });
    captured.push(RuntimeStageSignal {
        stage: RuntimeStage::TraceAudit,
        status: TraceStatus::Succeeded,
        at: started_at,
        detail: Some("trace metadata projected".to_string()),
    });
    captured.push(RuntimeStageSignal {
        stage: RuntimeStage::Egress,
        status: TraceStatus::Succeeded,
        at: started_at,
        detail: Some("response emitted".to_string()),
    });
    drop(captured);

    let trace = agent.build_run_trace(
        &seed,
        &ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(agent.config.role.clone(), agent.session_id.clone()),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        &[],
    );

    assert_eq!(
        trace.metadata.get("background_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("background_quality_signal"),
        Some(&"stable".to_string())
    );
    assert_eq!(
        trace.metadata.get("background_relationship_present"),
        Some(&"true".to_string())
    );
    assert_eq!(
        trace.metadata.get("background_decision"),
        Some(&"promoterelationshipfact".to_string())
    );
    assert_eq!(
        trace.metadata.get("background_durable_promotion_status"),
        Some(&"pending_review".to_string())
    );
    assert_eq!(
        trace.metadata.get("background_session_persistence_status"),
        Some(&"persisted".to_string())
    );
    assert_eq!(
        trace.metadata.get("background_total_attempts"),
        Some(&"3".to_string())
    );
    assert_eq!(
        trace.metadata.get("background_skip_count"),
        Some(&"1".to_string())
    );
    assert_eq!(
        trace.metadata.get("background_promote_relationship_count"),
        Some(&"1".to_string())
    );
    assert_eq!(
        trace.metadata.get("background_contract_complete"),
        Some(&"true".to_string())
    );

    let persistence = trace
        .stages
        .iter()
        .find(|stage| stage.stage == RuntimeStage::PersistenceMemory)
        .expect("persistence stage should exist");
    assert_eq!(
        persistence.metadata.get("background_revision"),
        trace.metadata.get("background_revision")
    );
    assert_eq!(
        persistence
            .metadata
            .get("background_durable_promotion_status"),
        Some(&"pending_review".to_string())
    );
    assert_eq!(
        persistence
            .metadata
            .get("background_session_persistence_status"),
        Some(&"persisted".to_string())
    );

    let egress = trace
        .stages
        .iter()
        .find(|stage| stage.stage == RuntimeStage::Egress)
        .expect("egress stage should exist");
    assert_eq!(
        egress.metadata.get("background_durable_promotion_status"),
        Some(&"pending_review".to_string())
    );
}
