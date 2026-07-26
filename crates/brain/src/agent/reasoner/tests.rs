use super::*;
use crate::agent::message::{ContentPart, ImageSource};
use crate::agent::provider::{ChatRequest, Provider, ProviderMetadata};
use crate::agent::streaming::{FinishReason, MockStreamBuilder, StreamingResponse};
use crate::agent::tactical::GlobalTacticalOrchestrator;
use crate::skills::tool::{RealtimeLookupKind, Tool, ToolDefinition};
use async_trait::async_trait;
use serde_json::json;

struct CaptureProvider {
    last_request: Arc<RwLock<Option<ChatRequest>>>,
}

impl CaptureProvider {
    fn new() -> Self {
        Self {
            last_request: Arc::new(RwLock::new(None)),
        }
    }
}

struct LocalCaptureProvider {
    inner: CaptureProvider,
}

impl LocalCaptureProvider {
    fn new() -> Self {
        Self {
            inner: CaptureProvider::new(),
        }
    }
}

#[async_trait]
impl Provider for LocalCaptureProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            id: "local-capture".into(),
            name: "LocalCapture".into(),
            description: "Local capture test provider".into(),
            icon: String::new(),
            fields: vec![],
            capabilities: vec!["runtime:local".to_string()],
            preferred_models: vec![],
        }
    }

    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        self.inner.stream_completion(request).await
    }

    fn name(&self) -> &str {
        "local-capture"
    }

    fn is_local(&self) -> bool {
        true
    }
}

#[async_trait]
impl Provider for CaptureProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            id: "capture".into(),
            name: "Capture".into(),
            description: "Capture test provider".into(),
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
        *self.last_request.write() = Some(request);
        Ok(MockStreamBuilder::new()
            .message("ok")
            .finish(FinishReason::Stop)
            .build())
    }

    fn name(&self) -> &str {
        "capture"
    }
}

#[test]
fn local_llm_timeout_scales_with_step_output_budget() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.llm_timeout = Duration::from_secs(60);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let request = ChatRequest {
        max_tokens: Some(reasoner_constants::LOCAL_MAX_STEP_TOKENS as u64),
        ..Default::default()
    };

    assert_eq!(
        reasoner.effective_llm_timeout_for_request(&request),
        Duration::from_secs(
            reasoner_constants::LOCAL_MAX_STEP_TOKENS as u64
                / reasoner_constants::LOCAL_OUTPUT_TOKENS_PER_TIMEOUT_SEC
        )
    );
}

#[tokio::test]
async fn think_attaches_worker_frontier_continuation_hint() {
    let provider = Arc::new(CaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.agent_name = Some("writer".to_string());
    config.model = "capture-model".to_string();
    config.session_id = Some("session-123".to_string());

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let _ = reasoner
        .think(
            vec![Message::user("继续上一章并保持连续性".to_string())],
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let hint = request.continuation_hint.expect("continuation hint");
    assert_eq!(hint.user_session_id.as_deref(), Some("session-123"));
    assert!(hint
        .worker_run_id
        .as_deref()
        .expect("worker run id")
        .contains("writer"));
    assert!(hint
        .continuation_frontier_id
        .as_deref()
        .expect("frontier id")
        .contains("frontier-"));
    assert!(hint.visible_prompt_fingerprint.is_some());
    let extra = request.extra_params.expect("extra params");
    assert_eq!(
        extra
            .get("continuation_worker_run_id")
            .and_then(|value| value.as_str()),
        hint.worker_run_id.as_deref()
    );
}

#[test]
fn local_llm_timeout_keeps_short_tool_turns_bounded() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.llm_timeout = Duration::from_secs(60);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let request = ChatRequest {
        max_tokens: Some(reasoner_constants::EXECUTION_TOOL_TURN_MAX_TOKENS),
        ..Default::default()
    };

    assert_eq!(
        reasoner.effective_llm_timeout_for_request(&request),
        Duration::from_secs(reasoner_constants::LOCAL_MEDIUM_LLM_TIMEOUT_SECS)
    );
}

#[test]
fn local_llm_timeout_keeps_short_answers_light() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.llm_timeout = Duration::from_secs(60);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let request = ChatRequest {
        max_tokens: Some(reasoner_constants::SHORT_ANSWER_MAX_TOKENS),
        ..Default::default()
    };

    assert_eq!(
        reasoner.effective_llm_timeout_for_request(&request),
        Duration::from_secs(reasoner_constants::LOCAL_SHORT_LLM_TIMEOUT_SECS)
    );
}

#[test]
fn execution_tool_turn_uses_small_output_budget_for_local_models() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.max_tokens = Some(128_000);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let messages = vec![
        Message::user("搜索网页并保存到知识库"),
        Message::system(reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED.to_string()),
    ];

    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            true,
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            )),
            CoordinatorTaskMode::ToolAgent,
            &messages,
        ),
        Some(reasoner_constants::EXECUTION_TOOL_TURN_MAX_TOKENS)
    );
}

#[test]
fn short_chat_turn_uses_short_answer_budget() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.max_tokens = Some(128_000);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            false,
            None,
            CoordinatorTaskMode::ChatLite,
            &[Message::user("你知道萨摩犬成犬多重吗")]
        ),
        Some(reasoner_constants::SHORT_ANSWER_MAX_TOKENS)
    );
}

#[test]
fn explanation_chat_turn_uses_explanation_budget() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.max_tokens = Some(128_000);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            false,
            None,
            CoordinatorTaskMode::ChatLite,
            &[Message::user("为什么天空是蓝色的？请解释一下原理。")]
        ),
        Some(reasoner_constants::BRIEF_EXPLANATION_MAX_TOKENS)
    );

    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            false,
            None,
            CoordinatorTaskMode::ChatLite,
            &[Message::user("为什么天空是蓝色的？请详细解释一下原理。")]
        ),
        Some(reasoner_constants::EXPLANATION_MAX_TOKENS)
    );
}

#[test]
fn standalone_chat_lite_turn_uses_latest_context_only() {
    assert!(
        Reasoner::<CaptureProvider>::should_use_latest_turn_context_only_for_query(
            "天空为什么是蓝色的？"
        )
    );
    assert!(
        Reasoner::<CaptureProvider>::should_use_latest_turn_context_only_for_query(
            "你知道萨摩犬成犬多重吗？"
        )
    );

    assert!(
        !Reasoner::<CaptureProvider>::should_use_latest_turn_context_only_for_query(
            "继续刚才的第二点"
        )
    );
    assert!(
        !Reasoner::<CaptureProvider>::should_use_latest_turn_context_only_for_query(
            "今天北京天气怎么样？"
        )
    );
}

#[test]
fn longform_chat_turn_uses_artifact_step_budget() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.max_tokens = Some(128_000);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            false,
            None,
            CoordinatorTaskMode::ChatLite,
            &[Message::user("请写一部 50 万字小说。")]
        ),
        Some(reasoner_constants::LONGFORM_STEP_MAX_TOKENS)
    );
}

struct StaticTool {
    name: &'static str,
}

#[async_trait]
impl Tool for StaticTool {
    fn name(&self) -> String {
        self.name.to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: format!("{} tool", self.name),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
        Ok("ok".to_string())
    }
}

#[test]
fn skill_manual_already_loaded_detects_matching_tool_result() {
    let message = Message::tool_result("call_1", "# Skill: python_tooling\n\nmanual")
        .with_tool_name("read_skill_manual");
    assert!(tool_result_reads_skill_manual(&message, "python_tooling"));
    assert!(skill_manual_already_loaded(&[message], "python_tooling"));
}

#[test]
fn routing_judgment_fallback_prefers_terminal_for_runtime_surface() {
    let text = Reasoner::<CaptureProvider>::routing_judgment_fallback_text(Some(
        CapabilityRouteHint::RuntimeSurface,
    ));
    assert!(text.contains("terminal"));
    assert!(text.contains("BenShu"));
}

#[test]
fn latest_lookup_result_for_followup_execution_accepts_direct_web_search_output() {
    let messages = vec![
        Message::user("请搜索柳叶刀最新治疗心脏病的论文，给我候选链接。".to_string()),
        Message::tool_result(
            "call_1",
            serde_json::json!({
                "kind": "web_search",
                "results": [
                    {
                        "title": "Lancet cardiology paper",
                        "url": "https://example.com/lancet-paper",
                        "snippet": "latest paper"
                    }
                ]
            })
            .to_string(),
        )
        .with_tool_name("web_search"),
    ];

    let result =
        Reasoner::<CaptureProvider>::latest_lookup_result_for_followup_execution(&messages)
            .expect("lookup result");
    assert!(result.contains("https://example.com/lancet-paper"));
}

#[test]
fn latest_lookup_result_for_followup_execution_accepts_delegate_tool_result_envelope() {
    let messages = vec![
        Message::user("请搜索可下载的开放小说来源，存入知识库。".to_string()),
        Message::tool_result(
            "call_delegate",
            r#"请求已执行完成。工具 `web_search` 的结果如下：[
  {
    "title": "Open fantasy catalog",
    "url": "https://example.com/open-fantasy",
    "snippet": "downloadable public-domain fantasy texts"
  }
]"#,
        )
        .with_tool_name("delegate"),
    ];

    let result =
        Reasoner::<CaptureProvider>::latest_lookup_result_for_followup_execution(&messages)
            .expect("delegated lookup result envelope");
    assert!(result.contains("https://example.com/open-fantasy"));
}

#[test]
fn latest_lookup_result_for_followup_execution_keeps_researcher_after_knowledge_import() {
    let messages = vec![
        Message::user("搜索起点前10免费小说，保存到知识库，然后写成txt。".to_string()),
        Message::tool_result(
            "call_research",
            "status: completed\nworker: researcher\nexecuted_tool: web_fetch\nsource_url: https://www.qidian.com/rank/recom/chn21/\nresult_summary:\n- 1. 夜无疆 | public metadata: 辰东|玄幻·东方玄幻 | source: https://www.qidian.com/rank/recom/chn21/",
        )
        .with_tool_name("delegate"),
        Message::tool_result(
            "call_import",
            "status: completed\nworker: knowledge\nexecuted_tool: knowledge_import_url\nresult:\nImported web knowledge into collection 'references' at path 'web/www-qidian-com/document-a89a86c8f6260d74'.",
        )
        .with_tool_name("delegate"),
    ];

    let result =
        Reasoner::<CaptureProvider>::latest_lookup_result_for_followup_execution(&messages)
            .expect("researcher lookup result");
    assert!(result.contains("worker: researcher"));
    assert!(result.contains("夜无疆"));
    assert!(result.contains("https://www.qidian.com/rank/recom/chn21/"));
}

#[test]
fn latest_lookup_result_for_followup_execution_accepts_researcher_browser_output() {
    let messages = vec![
        Message::user("搜索公开榜单并保存到知识库。".to_string()),
        Message::tool_result(
            "call_research",
            "status: completed\nworker: researcher\nexecuted_tool: browser_browse\nlookup_strategy: browser_search\nsource_url: https://example.com/search\nresult_summary:\n- 1. Item | public metadata: source: https://example.com/item",
        )
        .with_tool_name("delegate"),
    ];

    let result =
        Reasoner::<CaptureProvider>::latest_lookup_result_for_followup_execution(&messages)
            .expect("researcher browser lookup result");
    assert!(result.contains("worker: researcher"));
    assert!(result.contains("browser_browse"));
}

#[test]
fn latest_lookup_result_for_followup_execution_rejects_blocked_researcher_result() {
    let messages = vec![
        Message::user("搜索起点前10免费小说，保存到知识库。".to_string()),
        Message::tool_result(
            "call_research",
            "status: blocked\nworker: researcher\nexecuted_tool: web_fetch\nsource_url: https://www.zhihu.com/question/666367995\nblockers: fetched source did not provide enough verified evidence to answer",
        )
        .with_tool_name("delegate"),
    ];

    assert!(
        Reasoner::<CaptureProvider>::latest_lookup_result_for_followup_execution(&messages)
            .is_none()
    );
}

#[test]
fn compact_lookup_evidence_for_file_artifact_drops_raw_fetched_result() {
    let compact = Reasoner::<CaptureProvider>::compact_lookup_evidence_for_file_artifact(
        "status: completed\nworker: researcher\nexecuted_tool: web_fetch\nsource_url: https://www.qidian.com/rank/recom/chn21/\nresult_summary:\n- 1. 夜无疆 | public metadata: 辰东|玄幻 | source: https://www.qidian.com/rank/recom/chn21/\nfetched_result:\n{\"content\":\"raw escaped ..\\\\ path-like browser text\"}",
    );

    assert!(compact.contains("worker: researcher"));
    assert!(compact.contains("夜无疆"));
    assert!(!compact.contains("fetched_result"));
    assert!(!compact.contains("..\\"));
}

#[test]
fn local_file_continuation_query_is_not_external_research() {
    assert!(Reasoner::<CaptureProvider>::query_requests_local_file_continuation(
        "继续刚才那部本地长篇，读取 data/generated/longform/agent-artifact-1.txt，至少完成50章节，保存成txt文档"
    ));
    assert!(
        !Reasoner::<CaptureProvider>::query_requests_local_file_continuation(
            "搜索起点前10免费小说并保存到知识库"
        )
    );
    assert!(
        !Reasoner::<CaptureProvider>::query_requests_local_file_continuation(
            "搜索一部公网可下载或可阅读全文的热门玄幻小说，把可获取的正文内容作为素材收进知识库。然后基于知识库里的素材进行推理，创作一部全新的玄幻小说，目标超过50万字，并保存成 txt 文档。如果过程中遇到素材不可获取或写作无法继续，请自己判断下一步并说明 blocker。"
        )
    );
}

#[test]
fn post_import_file_artifact_uses_continuation_flow_for_oversized_text() {
    let (_, tool_name, args) = Reasoner::<CaptureProvider>::file_artifact_delegate_call(
        1,
        "根据知识库创造一个50万字的小说，并保存成txt文档，要求跑满50万字。",
        "根据知识库创造一个50万字的小说，并保存成txt文档，要求跑满50万字。",
    );

    assert_eq!(tool_name, "delegate");
    let task = args
        .get("task")
        .and_then(|value| value.as_str())
        .expect("delegate task should be present");
    assert!(task.contains("Create or continue"));
    assert!(task.contains("data/generated/tasks/"));
    assert!(task.contains("/agent-artifact-1.txt"));
    assert!(task.contains("checkpointed continuation"));
    assert!(task.contains("fresh, non-hardcoded title"));
    assert!(!task.contains("bounded starter artifact"));
}

#[test]
fn source_domain_dot_c_does_not_route_written_artifact_to_coder() {
    let task_context = "搜索一个科幻星际类型小说，尝试入知识库，根据这个的基础来写小说 50万字\n\nVerified researcher evidence:\nsource_url: https://collider.com/novels-like-interstellar/";
    let (_, _, args) = Reasoner::<CaptureProvider>::file_artifact_delegate_call(
        1,
        "搜索一个科幻星际类型小说，尝试入知识库，根据这个的基础来写小说 50万字",
        task_context,
    );

    assert_eq!(args["role"], "writer");
    let task = args["task"].as_str().expect("task");
    assert!(task.contains("requested written artifact"));
    assert!(!task.contains("requested local code or configuration artifact"));
}

#[test]
fn imported_receipt_api_text_does_not_route_written_artifact_to_coder() {
    let task_context = "搜索公网热门玄幻小说，把素材收进知识库，然后基于素材创作50万字小说并保存txt\n\nKnowledge import receipt:\nprovider: api\nsource_url: https://example.com/books/free.txt\nruntime_effect: knowledge.imported";
    let (_, _, args) = Reasoner::<CaptureProvider>::file_artifact_delegate_call(
        1,
        "搜索公网热门玄幻小说，把素材收进知识库，然后基于素材创作50万字小说并保存txt",
        task_context,
    );

    assert_eq!(args["role"], "writer");
}

#[test]
fn real_code_file_extension_routes_to_coder() {
    assert!(Reasoner::<CaptureProvider>::query_requests_code_artifact(
        "请创建 src/main.c 并写一个 hello world 程序"
    ));
    assert!(Reasoner::<CaptureProvider>::query_requests_code_artifact(
        "please update `crates/app/src/lib.rs`"
    ));
}

#[test]
fn generic_testing_phrase_does_not_route_to_code_artifact() {
    assert!(!Reasoner::<CaptureProvider>::query_requests_code_artifact(
        "你好，用一句中文回复：现在可以开始测试。"
    ));
    assert!(!Reasoner::<CaptureProvider>::query_requests_code_artifact(
        "现在可以开始测试。"
    ));
    assert_eq!(
        Reasoner::<CaptureProvider>::execution_required_route_for_query(
            "你好，用一句中文回复：现在可以开始测试。",
            false
        ),
        None
    );
    assert!(Reasoner::<CaptureProvider>::query_requests_code_artifact(
        "帮我测试这个 Rust 仓库"
    ));
}

#[test]
fn orchestration_marker_before_latest_user_does_not_block_new_turn() {
    let messages = vec![
        Message::user("上一轮请求".to_string()),
        Message::system("BENSHU_ORCHESTRATION_LOCAL_FILE_CONTINUATION".to_string()),
        Message::user("继续刚才那部本地长篇，读取 data/generated/longform/agent-artifact-1.txt，至少完成50章节，保存成txt文档".to_string()),
    ];

    assert!(Reasoner::<CaptureProvider>::has_system_marker(
        &messages,
        "BENSHU_ORCHESTRATION_LOCAL_FILE_CONTINUATION"
    ));
    assert!(
        !Reasoner::<CaptureProvider>::has_system_marker_after_latest_user(
            &messages,
            "BENSHU_ORCHESTRATION_LOCAL_FILE_CONTINUATION"
        )
    );
}

#[test]
fn should_prioritize_followup_execution_after_lookup_with_source_urls() {
    let messages = vec![
            Message::user("请搜索柳叶刀最新治疗心脏病的论文，给我候选链接。".to_string()),
            Message::tool_result(
                "call_1",
                "status: completed\nworker: researcher\nexecuted_tool: web_search\nresult:\nhttps://example.com/lancet-paper",
            )
            .with_tool_name("delegate"),
        ];

    assert!(
        Reasoner::<CaptureProvider>::should_prioritize_followup_execution(
            "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。",
            &messages
        )
    );
}

#[test]
fn should_not_repeat_knowledge_followup_after_import_receipt() {
    let messages = vec![
        Message::user("搜索公网资料，保存到知识库，然后写成 txt。".to_string()),
        Message::tool_result(
            "call_research",
            "status: completed\nworker: researcher\nexecuted_tool: web_fetch\nsource_url: https://example.com/source.txt\nresult: readable source body",
        )
        .with_tool_name("delegate"),
        Message::tool_result(
            "call_import",
            "status: completed\nworker: knowledge\nexecuted_tool: knowledge_import_url\nresult:\nruntime_effect: knowledge.imported\nstorage_target: durable_knowledge_store\ncollection: references\npath: web/example/source",
        )
        .with_tool_name("delegate"),
        Message::tool_result(
            "call_writer",
            "status: completed\nworker: writer\nexecuted_tool: writing\nruntime_effect: artifact.written\nartifact_path: /tmp/novels/project/chapters/0001.md\ntotal_units: 1200",
        )
        .with_tool_name("delegate"),
        Message::system(format!(
            "{}\n\nBENSHU_ARTIFACT_SCALE_CONTINUATION_REQUIRED",
            reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED
        )),
    ];

    assert!(Reasoner::<CaptureProvider>::current_turn_has_completed_knowledge_import(&messages));
    assert!(
        !Reasoner::<CaptureProvider>::should_prioritize_followup_execution(
            "搜索公网资料，保存到知识库，然后写一个50万字小说并保存成 txt。",
            &messages
        )
    );
}

#[test]
fn scaled_artifact_checkpoint_remains_continuation_until_units_satisfied() {
    let result = "status: completed\nworker: writer\nexecuted_tool: writing\nruntime_effect: artifact.written\nartifact_path: /tmp/novels/project/chapters/0001.md\ntotal_units: 1200";

    assert!(
        Reasoner::<CaptureProvider>::tool_result_is_scaled_artifact_continuation(
            "请写一个50万字小说并保存成 txt。",
            result
        )
    );
    assert!(
        !Reasoner::<CaptureProvider>::tool_result_satisfies_artifact_request(
            "请写一个50万字小说并保存成 txt。",
            result
        )
    );
}

#[test]
fn toolless_compound_lookup_recovery_delegates_to_researcher() {
    let query = "搜索公开资料并保存到知识库，然后根据知识库生成一份 txt 报告";
    let (_, tool_name, args) = Reasoner::<CaptureProvider>::toolless_execution_delegate_call(
        2,
        query,
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch),
        None,
    );

    assert_eq!(tool_name, "delegate");
    assert_eq!(
        args.get("role").and_then(|value| value.as_str()),
        Some("researcher")
    );
    let task = args
        .get("task")
        .and_then(|value| value.as_str())
        .expect("delegate task should be present");
    assert!(task.contains("Preserve the full original request"));
    assert!(task.contains(query));
}

#[test]
fn toolless_writing_recovery_delegates_to_writer() {
    let query = "请继续修订第二章，补全摘要、关键事实和连续性更新";
    let (_, tool_name, args) = Reasoner::<CaptureProvider>::toolless_execution_delegate_call(
        2,
        query,
        CapabilityRouteHint::Writing,
        None,
    );

    assert_eq!(tool_name, "delegate");
    assert_eq!(
        args.get("role").and_then(|value| value.as_str()),
        Some("writer")
    );
    assert!(
        Reasoner::<CaptureProvider>::route_allows_tooled_delegate_recovery(
            CapabilityRouteHint::Writing
        )
    );
}

#[test]
fn toolless_artifact_recovery_preserves_existing_project_context() {
    let query = "请继续写完整小说并保存成 txt 文档";
    let context = "- project_path: /home/user/app/data/generated/novels/万灵归一\n- artifact_path: /home/user/app/data/generated/novels/万灵归一/chapters/0001.md".to_string();
    let (_, tool_name, args) = Reasoner::<CaptureProvider>::toolless_execution_delegate_call(
        9,
        query,
        CapabilityRouteHint::Writing,
        Some(context),
    );

    assert_eq!(tool_name, "delegate");
    let task = args
        .get("task")
        .and_then(|value| value.as_str())
        .expect("delegate task should be present");
    assert!(task.contains("Existing artifact/work-in-progress context"));
    assert!(task.contains("/home/user/app/data/generated/novels/万灵归一"));
    assert!(task.contains("Do not create a new project/document"));
}

#[test]
fn artifact_continuation_context_infers_project_from_written_chapter_path() {
    let result =
        "status: completed\npath: /home/user/app/data/generated/novels/云霄劫/chapters/0001.md\n";
    let context = Reasoner::<CaptureProvider>::artifact_continuation_context_from_result(result)
        .expect("continuation context");

    assert!(context.contains("/home/user/app/data/generated/novels/云霄劫/chapters/0001.md"));
    assert!(context.contains("project_path: /home/user/app/data/generated/novels/云霄劫"));
}

#[test]
fn artifact_recovery_route_does_not_drift_to_knowledge() {
    let query = "继续完成已有小说项目的第三章，检查当前项目状态，确保第三章保存进项目并更新连续性";

    assert_eq!(
        Reasoner::<CaptureProvider>::artifact_execution_delegate_route(query),
        CapabilityRouteHint::Writing
    );
}

#[test]
fn normalize_local_pseudo_tool_call_maps_google_search_to_web_search() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "web_search" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider,
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let (name, args) = reasoner.normalize_local_pseudo_tool_call(
        "google_search".to_string(),
        json!({ "queries": ["llama.cpp Vulkan tutorial YouTube"] }),
    );

    assert_eq!(name, "web_search");
    assert_eq!(
        args.get("query").and_then(|value| value.as_str()),
        Some("llama.cpp Vulkan tutorial YouTube")
    );
}

#[test]
fn normalize_local_pseudo_tool_call_maps_worker_aliases_to_real_tools() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "cipher" });
    tools.add(StaticTool {
        name: "data_transform",
    });
    tools.add(StaticTool {
        name: "runtime_surface",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider,
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let (name, args) = reasoner.normalize_local_pseudo_tool_call(
        "crypto.hash_text".to_string(),
        json!({ "text": "BenShu", "algorithm": "sha256" }),
    );
    assert_eq!(name, "cipher");
    assert_eq!(
        args.get("action").and_then(|value| value.as_str()),
        Some("hash_text")
    );

    let (name, args) = reasoner
        .normalize_local_pseudo_tool_call("data.stats".to_string(), json!({ "data": [1, 2, 3] }));
    assert_eq!(name, "data_transform");
    assert_eq!(
        args.get("action").and_then(|value| value.as_str()),
        Some("stats")
    );

    let (name, args) =
        reasoner.normalize_local_pseudo_tool_call("runtime.catalog".to_string(), json!({}));
    assert_eq!(name, "runtime_surface");
    assert_eq!(
        args.get("action").and_then(|value| value.as_str()),
        Some("catalog")
    );
}

#[test]
fn normalize_local_pseudo_tool_call_recovers_colon_embedded_keys() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "novel_studio",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider,
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let (name, args) = reasoner.normalize_local_pseudo_tool_call(
        "novel_studio".to_string(),
        json!({
            "action:<|\\\"|>audit_chapter<|\\\"|>": null,
            "chapter_number:1": null,
            "project_path:<|\\\"|>/tmp/project<|\\\"|>": null
        }),
    );

    assert_eq!(name, "novel_studio");
    assert_eq!(
        args.get("action").and_then(|value| value.as_str()),
        Some("audit_chapter")
    );
    assert_eq!(
        args.get("chapter_number").and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        args.get("project_path").and_then(|value| value.as_str()),
        Some("/tmp/project")
    );
}

#[test]
fn extract_inline_pseudo_tool_calls_parses_direct_knowledge_search_contract() {
    let calls = Reasoner::<CaptureProvider>::extract_inline_pseudo_tool_calls(
        "<|tool_call>call:knowledge_search{query: \"柳叶刀心脏病治疗\"}<tool_call|>",
    );

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "knowledge_search");
    assert_eq!(
        calls[0].1,
        serde_json::json!({ "query": "柳叶刀心脏病治疗" })
    );
}

#[test]
fn extract_inline_pseudo_tool_calls_parses_json_tool_contracts() {
    let calls = Reasoner::<CaptureProvider>::extract_inline_pseudo_tool_calls(
        r#"<|tool_call>{"name":"cipher","arguments":{"action":"hash_text","text":"BenShu","algorithm":"sha256"}}<tool_call|>"#,
    );

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "cipher");
    assert_eq!(
        calls[0].1,
        serde_json::json!({
            "action": "hash_text",
            "text": "BenShu",
            "algorithm": "sha256"
        })
    );
}

#[test]
fn extract_inline_pseudo_tool_calls_parses_openai_like_function_contracts() {
    let calls = Reasoner::<CaptureProvider>::extract_inline_pseudo_tool_calls(
        r#"<|tool_call>{"type":"function","function":{"name":"cipher","arguments":"{\"action\":\"hash_text\",\"text\":\"BenShu\",\"algorithm\":\"sha256\"}"}}<tool_call|>"#,
    );

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "cipher");
    assert_eq!(
        calls[0].1.get("action").and_then(|value| value.as_str()),
        Some("hash_text")
    );
}

#[test]
fn extract_inline_pseudo_tool_calls_parses_assistant_tool_request_contract() {
    let calls = Reasoner::<CaptureProvider>::extract_inline_pseudo_tool_calls(
        r#"[Assistant tool request] delegate(role="writer", task="继续写第五章", full_user_request="继续写第五章")"#,
    );

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "delegate");
    assert_eq!(
        calls[0].1.get("role").and_then(|value| value.as_str()),
        Some("writer")
    );
    assert_eq!(
        calls[0].1.get("task").and_then(|value| value.as_str()),
        Some("继续写第五章")
    );
}

#[tokio::test]
async fn think_requires_read_skill_manual_before_other_tools() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "read_skill_manual",
    });
    tools.add(StaticTool { name: "shell" });
    tools.add(StaticTool {
        name: "tool_search",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();
    config.extra_params = Some(serde_json::json!({
        "matched_skill_manual": "python_tooling"
    }));

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![Message::user(
        "请按 python_tooling 这个 skill 来做".to_string(),
    )];
    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();
    assert_eq!(names, vec!["read_skill_manual".to_string()]);
}

#[tokio::test]
async fn think_detects_matched_skill_manual_from_system_message() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "read_skill_manual",
    });
    tools.add(StaticTool { name: "shell" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![
            Message::system(
                "### RUNTIME_SURFACE_HARD_ROUTE\n\
                 This request matches the skill `python_tooling`. Call `read_skill_manual` for that skill before executing runtime steps, unless you already loaded that manual in this turn.\n"
                    .to_string(),
            ),
            Message::user("请按 python_tooling 这个 skill 来做".to_string()),
        ];
    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();
    assert_eq!(names, vec!["read_skill_manual".to_string()]);
}

#[test]
fn skill_asset_already_loaded_detects_matching_tool_result() {
    let message = Message::tool_result("call_1", "# Skill Asset: references/setup.md\n\nasset")
        .with_tool_name("read_skill_asset");
    assert!(tool_result_reads_skill_asset(
        &message,
        "references/setup.md"
    ));
    assert!(skill_asset_already_loaded(
        &[message],
        "references/setup.md"
    ));
}

#[tokio::test]
async fn think_requires_read_skill_asset_when_user_explicitly_mentions_asset_path() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "read_skill_manual",
    });
    tools.add(StaticTool {
        name: "read_skill_asset",
    });
    tools.add(StaticTool { name: "shell" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![
            Message::system(
                "### RUNTIME_SURFACE_HARD_ROUTE\n\
                 This request matches the skill `python_tooling`. Call `read_skill_manual` for that skill before executing runtime steps, unless you already loaded that manual in this turn.\n"
                    .to_string(),
            ),
            Message::tool_result("call_1", "# Skill: python_tooling\n\nmanual")
                .with_tool_name("read_skill_manual"),
            Message::user(
                "请按 python_tooling 里的 references/setup.md 继续做".to_string(),
            ),
        ];
    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();
    assert_eq!(names, vec!["read_skill_asset".to_string()]);
}

#[tokio::test]
async fn think_requires_read_skill_asset_when_user_mentions_reference_kind_after_manual() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "read_skill_manual",
    });
    tools.add(StaticTool {
        name: "read_skill_asset",
    });
    tools.add(StaticTool { name: "shell" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![
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
            Message::user("先看看这个 skill 的参考资料再继续".to_string()),
        ];
    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();
    assert_eq!(names, vec!["read_skill_asset".to_string()]);
}

#[tokio::test]
async fn think_prioritizes_forged_session_tool_after_approved_forge() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "forge_skill",
    });
    tools.add(StaticTool {
        name: "pdf_builder",
    });
    tools.add(StaticTool { name: "shell" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![
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
        Message::user("继续执行这个刚刚 forge 的工具".to_string()),
    ];

    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let names: Vec<_> = request.tools.into_iter().map(|tool| tool.name).collect();
    assert_eq!(names, vec!["pdf_builder".to_string()]);
}

#[tokio::test]
async fn think_injects_media_followup_guidance_into_system_prompt() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "document_understand",
    });
    tools.add(StaticTool {
        name: "runtime_surface",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![
        Message::tool_result(
            "call_1",
            serde_json::json!({
                "status": "needs_followup",
                "media_preprocess_route": "normalize_audio",
                "media_pipeline_outcome": "model_failed_after_preprocess"
            })
            .to_string(),
        )
        .with_tool_name("document_understand"),
        Message::user("继续处理这个音频".to_string()),
    ];

    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let system_prompt = request.system_prompt.expect("system prompt present");
    assert!(system_prompt.contains("MEDIA FOLLOW-UP STRATEGY"));
    assert!(system_prompt.contains("alternate_model_fallback"));
}

#[tokio::test]
async fn think_injects_truth_verification_guidance_into_system_prompt() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![Message::user("当前 OpenAI API 定价政策是什么".to_string())];

    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let system_prompt = request.system_prompt.expect("system prompt present");
    assert!(system_prompt.contains("TRUTH AND VERIFICATION CONTRACT"));
    assert!(system_prompt.contains("Never present unverified claims as confirmed facts."));
}

#[tokio::test]
async fn think_skips_reasoning_banner_for_chat_lite_turns() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let _ = reasoner
        .think(
            vec![Message::user("吃饭了吗".to_string())],
            &ReasoningStrategy::Reflexion,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let system_prompt = request.system_prompt.expect("system prompt present");
    assert!(system_prompt.contains("BENSHU_CHAT_LITE"));
    assert!(!system_prompt.contains("### REFLEXION MODE"));
}

#[tokio::test]
async fn think_keeps_reasoning_banner_for_tool_agent_turns() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "delegate" });
    tools.add(StaticTool {
        name: "shared_board",
    });
    tools.add(StaticTool {
        name: "tool_search",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let _ = reasoner
        .think(
            vec![Message::user(
                "帮我修一下这个 Rust 仓库里的 bug 并提交补丁".to_string(),
            )],
            &ReasoningStrategy::Reflexion,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let system_prompt = request.system_prompt.expect("system prompt present");
    assert!(system_prompt.contains("BENSHU_TOOL_AGENT"));
    assert!(system_prompt.contains("### REFLEXION MODE"));
}

#[tokio::test]
async fn think_keeps_only_compact_frontstage_core_tools_for_plain_chat_lite() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "delegate" });
    tools.add(StaticTool {
        name: "shared_board",
    });
    tools.add(StaticTool {
        name: "tool_search",
    });
    tools.add(StaticTool {
        name: "search_history",
    });
    tools.add(StaticTool {
        name: "knowledge_search",
    });
    tools.add(StaticTool {
        name: "tiered_search",
    });
    tools.add(StaticTool {
        name: "remember_this",
    });
    tools.add(StaticTool {
        name: "manage_facts",
    });
    tools.add(StaticTool {
        name: "transcribe_audio",
    });
    tools.add(StaticTool {
        name: "text_to_speech",
    });
    tools.add(StaticTool {
        name: "runtime_surface",
    });
    tools.add(StaticTool { name: "web_search" });
    tools.add(StaticTool {
        name: "document_understand",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let _ = reasoner
        .think(
            vec![Message::user("帮我简单解释一下光合作用".to_string())],
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let names: std::collections::HashSet<_> = request
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(names.len(), 8);
    for expected in [
        "delegate",
        "shared_board",
        "tool_search",
        "search_history",
        "remember_this",
        "manage_facts",
        "transcribe_audio",
        "text_to_speech",
    ] {
        assert!(
            names.contains(expected),
            "missing frontstage core tool {expected}"
        );
    }
    assert!(!names.contains("runtime_surface"));
    assert!(!names.contains("web_search"));
    assert!(!names.contains("document_understand"));

    let delegate = request
        .tools
        .iter()
        .find(|tool| tool.name == "delegate")
        .expect("delegate is present");
    assert_eq!(
        delegate.description,
        "Delegate execution to one specialist worker."
    );
    assert!(delegate.parameters_ts.is_none());
    assert!(delegate.usage_guidelines.is_none());
}

#[tokio::test]
async fn think_keeps_coordinator_surface_for_alternate_media_fallback() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "document_understand",
    });
    tools.add(StaticTool { name: "pdf_parse" });
    tools.add(StaticTool {
        name: "text_extract",
    });
    tools.add(StaticTool {
        name: "tool_search",
    });
    tools.add(StaticTool { name: "delegate" });
    tools.add(StaticTool {
        name: "shared_board",
    });
    tools.add(StaticTool {
        name: "runtime_surface",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![
        Message::tool_result(
            "call_1",
            serde_json::json!({
                "status": "needs_followup",
                "media_preprocess_route": "normalize_audio",
                "media_pipeline_outcome": "model_failed_after_preprocess"
            })
            .to_string(),
        )
        .with_tool_name("document_understand"),
        Message::user("继续处理这个音频".to_string()),
    ];

    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let names: std::collections::HashSet<_> =
        request.tools.into_iter().map(|tool| tool.name).collect();
    assert!(names.contains("delegate"));
    assert!(names.contains("shared_board"));
    assert!(names.contains("tool_search"));
    assert!(!names.contains("document_understand"));
    assert!(!names.contains("pdf_parse"));
    assert!(!names.contains("text_extract"));
    assert!(!names.contains("runtime_surface"));
}

#[tokio::test]
async fn think_keeps_coordinator_surface_for_attachment_media_fallback() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "document_understand",
    });
    tools.add(StaticTool { name: "pdf_parse" });
    tools.add(StaticTool {
        name: "text_extract",
    });
    tools.add(StaticTool {
        name: "tool_search",
    });
    tools.add(StaticTool { name: "delegate" });
    tools.add(StaticTool {
        name: "shared_board",
    });
    tools.add(StaticTool {
        name: "runtime_surface",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![
        Message::tool_result(
            "call_1",
            serde_json::json!({
                "status": "needs_followup",
                "media_preprocess_route": "extract_video_frames",
                "media_pipeline_outcome": "preprocess_failed"
            })
            .to_string(),
        )
        .with_tool_name("document_understand"),
        Message::user("继续处理这个视频".to_string()),
    ];

    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let names: std::collections::HashSet<_> =
        request.tools.into_iter().map(|tool| tool.name).collect();
    assert!(names.contains("delegate"));
    assert!(names.contains("shared_board"));
    assert!(names.contains("tool_search"));
    assert!(!names.contains("document_understand"));
    assert!(!names.contains("pdf_parse"));
    assert!(!names.contains("text_extract"));
    assert!(!names.contains("runtime_surface"));
}

#[tokio::test]
async fn think_keeps_coordinator_surface_for_text_extract_media_fallback() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "document_understand",
    });
    tools.add(StaticTool { name: "pdf_parse" });
    tools.add(StaticTool {
        name: "text_extract",
    });
    tools.add(StaticTool {
        name: "tool_search",
    });
    tools.add(StaticTool { name: "delegate" });
    tools.add(StaticTool {
        name: "shared_board",
    });
    tools.add(StaticTool {
        name: "runtime_surface",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let messages = vec![
        Message::tool_result(
            "call_1",
            serde_json::json!({
                "status": "error",
                "media_preprocess_route": "image_page_raster",
                "media_pipeline_outcome": "model_failed_after_preprocess"
            })
            .to_string(),
        )
        .with_tool_name("text_extract"),
        Message::user("继续处理这个截图".to_string()),
    ];

    let _ = reasoner
        .think(
            messages,
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("request captured");
    let names: std::collections::HashSet<_> =
        request.tools.into_iter().map(|tool| tool.name).collect();
    assert!(names.contains("delegate"));
    assert!(names.contains("shared_board"));
    assert!(names.contains("tool_search"));
    assert!(!names.contains("document_understand"));
    assert!(!names.contains("pdf_parse"));
    assert!(!names.contains("text_extract"));
    assert!(!names.contains("runtime_surface"));
}

#[tokio::test]
async fn think_sets_capability_route_for_media_followup_contract() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "document_understand",
    });
    tools.add(StaticTool { name: "pdf_parse" });
    tools.add(StaticTool {
        name: "text_extract",
    });
    tools.add(StaticTool {
        name: "tool_search",
    });
    tools.add(StaticTool {
        name: "runtime_surface",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();
    config.preamble = "test".to_string();
    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let _ = reasoner
        .think(
            vec![
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
            ],
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think result");

    let request = provider
        .last_request
        .read()
        .clone()
        .expect("captured request");
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
fn media_followup_capability_contract_covers_remaining_followup_strategies() {
    let attachment = media_followup_capability_contract(&[
        "extract_video_frames:attachment_fallback".to_string(),
    ])
    .expect("attachment contract");
    assert_eq!(attachment.capability_route, "document_understanding");
    assert_eq!(
        attachment.execution_surface,
        "document_understanding_attachment_fallback"
    );
    assert!(attachment.prefer_document_understanding_tools);

    let clarification = media_followup_capability_contract(&[
        "image_page_raster:clarification_or_manual_review".to_string(),
    ])
    .expect("clarification contract");
    assert_eq!(clarification.capability_route, "document_understanding");
    assert_eq!(
        clarification.execution_surface,
        "document_understanding_clarification_or_manual_review"
    );
    assert!(clarification.prefer_document_understanding_tools);
}

#[test]
fn concise_media_followup_label_is_not_low_value() {
    assert!(!Reasoner::<CaptureProvider>::is_low_value_media_answer(
        "继续基于刚才那张 UI 截图回答：用户当前选中的左侧菜单是什么？只回答菜单名。",
        "Local Models",
    ));
}

#[test]
fn media_followup_strategies_include_provider_metadata_from_assistant_messages() {
    let mut assistant = Message::assistant("local provider fallback");
    assistant.metadata.insert(
            "provider_media_preprocess_followup_strategies".to_string(),
            "extract_video_frames:alternate_model_fallback,normalize_audio:clarification_or_manual_review".to_string(),
        );

    let strategies = media_followup_strategies_from_messages(&[assistant]);
    assert_eq!(
        strategies,
        vec![
            "extract_video_frames:alternate_model_fallback".to_string(),
            "normalize_audio:clarification_or_manual_review".to_string()
        ]
    );
}

#[tokio::test]
async fn think_returns_explicit_image_generation_unavailable_when_tool_missing() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "knowledge" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let result = reasoner
        .think(
            vec![Message::user("请帮我生成一张图片".to_string())],
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    assert!(result.text.contains("当前没有可用的图片生成模型支持"));
    assert!(provider.last_request.read().is_none());
}

#[tokio::test]
async fn think_does_not_treat_media_understanding_as_image_generation_fallback() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "knowledge" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let result = reasoner
        .think(
            vec![Message::user(Content::Parts(vec![
                ContentPart::Text {
                    text: "请描述这张图片里有什么，只用中文简短回答。".to_string(),
                },
                ContentPart::Image {
                    source: ImageSource::Url {
                        url: "file:///tmp/test.png".to_string(),
                    },
                },
            ]))],
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    assert!(!result.text.contains("当前没有可用的图片生成模型支持"));
    assert!(provider.last_request.read().is_some());
}

#[tokio::test]
async fn think_does_not_treat_creation_planning_with_illustrator_as_image_generation() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "knowledge" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider.clone(),
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let result = reasoner
        .think(
            vec![Message::user(
                "[BENSHU_CREATION_PLANNING_DIALOGUE]\n请给小说生成候选书名，女主是独立插画师。"
                    .to_string(),
            )],
            &ReasoningStrategy::ReAct,
            |_| {},
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .expect("think succeeds");

    assert!(!result.text.contains("当前没有可用的图片生成模型支持"));
    assert!(provider.last_request.read().is_some());
}

#[tokio::test]
async fn explicit_image_generation_turn_detects_available_execution_tool() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "generate_image",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider,
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let available = reasoner.available_execution_tools_for_query("请帮我生成一张图片");
    assert_eq!(available, vec!["generate_image".to_string()]);
}

#[tokio::test]
async fn realtime_lookup_recovery_keeps_browser_when_worker_has_it() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "web_search" });
    tools.add(StaticTool { name: "web_fetch" });
    tools.add(StaticTool {
        name: "browser_browse",
    });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider,
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let available = reasoner.available_execution_tools_for_query("查找网页里的公开列表证据");
    assert!(available.contains(&"web_search".to_string()));
    assert!(available.contains(&"browser_browse".to_string()));
}

#[tokio::test]
async fn governed_fiction_recovery_prefers_novel_studio_over_generic_writing() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "novel_studio",
    });
    tools.add(StaticTool {
        name: "writing_studio",
    });
    tools.add(StaticTool { name: "write_file" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider,
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let available =
        reasoner.available_execution_tools_for_query("根据资料写一部50万字的科幻星际小说");
    assert!(available.contains(&"novel_studio".to_string()));
    assert!(!available.contains(&"writing_studio".to_string()));
}

#[tokio::test]
async fn continuation_chapter_execution_keeps_writing_tool_surface() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "novel_studio",
    });
    tools.add(StaticTool { name: "delegate" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider,
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let query = "继续写第五章，保持刚才的中文合同、人物和世界观，不要重写前文。";
    assert_eq!(
        Reasoner::<CaptureProvider>::execution_required_route_for_query(query, false),
        Some(CapabilityRouteHint::Writing)
    );
    let available = reasoner.available_execution_tools_for_query(query);
    assert!(available.contains(&"novel_studio".to_string()));
}

#[tokio::test]
async fn creation_planning_dialogue_has_no_execution_tools() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "delegate" });
    tools.add(StaticTool {
        name: "novel_studio",
    });
    tools.add(StaticTool {
        name: "writing_studio",
    });
    tools.add(StaticTool { name: "write_file" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider,
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let query = "[BENSHU_CREATION_PLANNING_DIALOGUE]\n只定大纲，不写正文。目标50万字，每章3000字。";
    assert!(reasoner
        .available_execution_tools_for_query(query)
        .is_empty());
    assert_eq!(
        Reasoner::<CaptureProvider>::execution_required_route_for_query(query, false),
        None
    );
    assert!(!Reasoner::<CaptureProvider>::query_requests_file_artifact(
        query
    ));
    assert!(!Reasoner::<CaptureProvider>::query_requests_artifact_mutation(query));
    assert!(!Reasoner::<CaptureProvider>::query_requests_local_file_continuation(query));
    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            false,
            None,
            CoordinatorTaskMode::ChatLite,
            &[Message::user(query)]
        ),
        Some(2048)
    );
}

#[test]
fn creation_planning_dialogue_outline_budget_stays_compact_for_requested_structure() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.max_tokens = Some(128_000);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let query = "[BENSHU_CREATION_PLANNING_DIALOGUE]\n先不要写正文，请给完整框架、主角名字、核心矛盾和20章左右的章节大纲。";

    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            false,
            None,
            CoordinatorTaskMode::ChatLite,
            &[Message::user(query)]
        ),
        Some(4096)
    );
}

#[test]
fn creation_planning_dialogue_budget_does_not_scale_linearly_with_chapter_count() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.max_tokens = Some(128_000);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let query = "[BENSHU_CREATION_PLANNING_DIALOGUE]\n总目标字数：50000\n每章目标档位：2500\n请先给完整框架和章节大纲。";

    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            false,
            None,
            CoordinatorTaskMode::ChatLite,
            &[Message::user(query)]
        ),
        Some(4096)
    );
}

#[test]
fn creation_planning_skeleton_stage_uses_small_step_budget() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.max_tokens = Some(128_000);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let query = "[BENSHU_CREATION_PLANNING_DIALOGUE]\n合同分段补齐阶段：Skeleton\n请只补合同骨架。";

    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            false,
            None,
            CoordinatorTaskMode::ChatLite,
            &[Message::user(query)]
        ),
        Some(1024)
    );
}

#[test]
fn local_creation_planning_skeleton_timeout_is_capped() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.llm_timeout = Duration::from_secs(30);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let request = ChatRequest {
        messages: vec![Message::user(
            "[BENSHU_CREATION_PLANNING_DIALOGUE]\n合同分段补齐阶段：Skeleton\n请只补合同骨架。",
        )],
        max_tokens: Some(1024),
        ..Default::default()
    };

    assert_eq!(
        reasoner.effective_llm_timeout_for_request(&request),
        Duration::from_secs(45)
    );
}

#[test]
fn tool_boundary_recovery_prompt_uses_available_tools_from_runtime_error() {
    let error = "Runtime tool error in `delegate`: tool is not equipped for this agent. Available tools right now: browser_browse, web_fetch, web_search.";

    assert!(Reasoner::<CaptureProvider>::tool_error_is_not_equipped(
        error
    ));
    assert_eq!(
        Reasoner::<CaptureProvider>::available_tools_from_not_equipped_error(error).as_deref(),
        Some("browser_browse, web_fetch, web_search")
    );

    let prompt = Reasoner::<CaptureProvider>::tool_boundary_recovery_prompt("delegate", error, &[]);

    assert!(prompt.contains("BENSHU_TOOL_BOUNDARY_RECOVERY"));
    assert!(prompt.contains("Do not call `delegate` again"));
    assert!(prompt.contains("browser_browse, web_fetch, web_search"));
    assert!(prompt.contains("return a compact blocker"));
}

#[test]
fn knowledge_import_handoff_result_preserves_source_for_coordinator() {
    let result = "status: completed\nworker: researcher\nexecuted_tool: web_fetch\nsource_url: https://example.org/source.txt\nresult_summary: readable source body\ncontent: 第一章 少年从边荒醒来，灵脉复苏。";

    let handoff = Reasoner::<CaptureProvider>::knowledge_import_coordinator_handoff_result(
        "把素材存入知识库，然后继续写作",
        "https://example.org/source.txt",
        result,
    );

    assert!(handoff.contains("status: completed"));
    assert!(handoff.contains("worker: researcher"));
    assert!(handoff.contains("handoff_required: knowledge_import"));
    assert!(handoff.contains("runtime_effect: source.evidence.ready"));
    assert!(handoff.contains("source_url: https://example.org/source.txt"));
    assert!(!handoff.contains("knowledge.imported"));
}

#[test]
fn knowledge_import_delegate_evidence_is_byte_capped() {
    let evidence = format!("fetched_result:\n{}", "玄幻正文素材。".repeat(2000));
    let (_, tool, args) = Reasoner::<CaptureProvider>::knowledge_import_delegate_call_with_evidence(
        7,
        "https://example.org/source.txt",
        &"请把这个素材存入知识库，然后继续写新小说。".repeat(100),
        Some(&evidence),
    );

    let serialized = args.to_string();
    assert_eq!(tool, "delegate");
    assert!(serialized.contains("https://example.org/source.txt"));
    assert!(serialized.contains("truncated_for_tool_arg"));
    assert!(
        serialized.len() < 8_000,
        "delegate args should remain below the internal safety guard; got {} bytes",
        serialized.len()
    );
}

#[test]
fn workspace_boundary_delegate_blocker_is_recoverable_once() {
    let result = "status: blocked\nworker: writer\nexecuted_tool: read_file\nblockers: requested path is outside the current BenShu workspace\npath: /home/user/.benshu/project.md\nworkspace_root: /home/user/benshu\nrecovery_hint: retry only with a path inside workspace_root";

    assert!(
        Reasoner::<CaptureProvider>::delegate_blocker_is_recoverable_workspace_boundary(result)
    );

    let prompt = Reasoner::<CaptureProvider>::workspace_boundary_recovery_prompt(result);
    assert!(prompt.contains("BENSHU_WORKSPACE_BOUNDARY_RECOVERY"));
    assert!(prompt.contains("/home/user/benshu"));
    assert!(prompt.contains("Do not treat hidden sibling directories"));
}

#[test]
fn structured_tool_contract_error_is_not_successful_delivery() {
    let result = r#"请求已执行完成。工具 `novel_studio` 的结果如下：{
  "action": "write_draft",
  "error_kind": "missing_required_content",
  "example_shape": {"content": "<full text to save>"},
  "next_step_hint": "Generate the actual body text first, then call this action again."
}"#;
    let messages = vec![
        Message::user("请继续写第三章并保存".to_string()),
        Message::tool_result("call_delegate", result).with_tool_name("delegate"),
    ];

    assert!(Reasoner::<CaptureProvider>::tool_result_content_is_runtime_error(result));
    assert!(Reasoner::<CaptureProvider>::latest_successful_tool_result(&messages).is_none());
    assert!(Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages).is_none());
    assert!(Reasoner::<CaptureProvider>::tool_error_is_recoverable_contract(result));

    let prompt = Reasoner::<CaptureProvider>::tool_contract_recovery_prompt(
        "novel_studio",
        result,
        &["novel_studio".to_string(), "write_file".to_string()],
    );
    assert!(prompt.contains("BENSHU_TOOL_CONTRACT_RECOVERY"));
    assert!(prompt.contains("generate the actual body/content"));
    assert!(prompt.contains("URL, knowledge receipt, imported document path"));
    assert!(prompt.contains("runtime can attach that body"));
    assert!(prompt.contains("Tool names are top-level calls"));
    assert!(prompt.contains("Do not call write/edit/file tools to record recovery notes"));
}

#[test]
fn content_required_contract_recovery_can_attach_generated_body_text() {
    let result = r#"{
  "action": "write_draft",
  "error_kind": "missing_required_content",
  "required_fields": ["content", "project_path", "chapter_number or chapter_title"],
  "example_shape": {
    "action": "write_draft",
    "project_path": "/tmp/novel",
    "chapter_number": 3,
    "content": "<full text to save>"
  },
  "next_step_hint": "Generate the actual body text first, then call this action again."
}"#;
    let body = "第三章 雨夜的灯\n\n沈照沿着湿冷的石阶往上走，城门后的铃声一阵紧过一阵。前两章留下的银灯仍在他掌心发热，像是在提醒他：不要相信任何没有影子的人。守夜人没有再阻拦，只把一枚缺口铜钱放进他手里，说旧约会在黎明前醒来。沈照明白，这不是新的冒险开端，而是上一场选择必须偿还的代价。\n\n他推开塔门，看到墙上挂满无名人的影子。每一枚影子都在重复同一句话：欠下的路，只能自己走完。沈照把铜钱按进灯座，银火忽然分成三缕，分别照向城北的旧井、云端的断桥，以及母亲遗留的黑色书匣。";
    let messages = vec![
        Message::user("请继续写第三章并保存".to_string()),
        Message::tool_result("call_novel_studio", result).with_tool_name("novel_studio"),
    ];

    let (id, tool, args) =
        Reasoner::<CaptureProvider>::content_required_tool_call_from_generated_text(
            &messages, body,
        )
        .expect("repair call");

    assert_eq!(id, "content-required-contract-repair");
    assert_eq!(tool, "novel_studio");
    assert_eq!(args["action"], "write_draft");
    assert_eq!(args["project_path"], "/tmp/novel");
    assert_eq!(args["chapter_number"], 3);
    assert_eq!(args["content"], body);
}

#[test]
fn pending_content_action_can_attach_generated_body_text() {
    let result = r#"{
  "success": true,
  "runtime_effect": "artifact.checkpointed",
  "stage": "writer_packet",
  "next_action": "write_draft",
  "pending_content_action": {
    "tool": "novel_studio",
    "content_field": "content",
    "args": {
      "action": "write_draft",
      "project_path": "/tmp/novel",
      "chapter_number": 1,
      "chapter_title": "第1章"
    }
  }
}"#;
    let body = "第一章 灰烬未冷\n\n陆沉在城墙下醒来时，掌心里的余烬仍在发光。那光不是火，而是世界还没有彻底熄灭的证据。苏瑶站在断裂的铜钟旁，告诉他霜序的人已经越过北门，所有温区都将在三日内失去律火。陆沉没有立刻回答。他看见每一块砖缝里都藏着细小的灰线，像某种被抹去的道路，又像有人刻意留下的求救。\n\n当第一缕冷风穿过城门，灰线同时亮起，指向地底深处一座无人承认存在的旧炉。";
    let messages = vec![
        Message::user("请写第一章并保存".to_string()),
        Message::tool_result("call_novel_studio", result).with_tool_name("novel_studio"),
    ];

    let (id, tool, args) =
        Reasoner::<CaptureProvider>::pending_content_action_tool_call_from_generated_text(
            &messages, body,
        )
        .expect("pending content call");

    assert!(id.starts_with("BENSHU_PENDING_CONTENT_ACTION:"));
    assert_eq!(tool, "novel_studio");
    assert_eq!(args["action"], "write_draft");
    assert_eq!(args["project_path"], "/tmp/novel");
    assert_eq!(args["chapter_number"], 1);
    assert_eq!(args["content"], body);
}

#[test]
fn pending_content_generation_turn_uses_longform_output_budget() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.max_tokens = Some(128_000);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let packet = r#"{
  "success": true,
  "runtime_effect": "artifact.checkpointed",
  "stage": "writer_packet",
  "next_action": "write_draft",
  "pending_content_action": {
    "tool": "novel_studio",
    "content_field": "content",
    "args": {
      "action": "write_draft",
      "project_path": "/tmp/novel",
      "chapter_number": 1,
      "chapter_title": "第1章"
    }
  }
}"#;
    let messages = vec![
        Message::user("请写一本玄幻小说的第一章并保存".to_string()),
        Message::tool_result("call_novel_studio", packet).with_tool_name("novel_studio"),
    ];

    assert!(Reasoner::<CaptureProvider>::turn_requires_generated_artifact_content(&messages));
    assert_eq!(
        reasoner.request_max_tokens_for_turn(true, None, CoordinatorTaskMode::ToolAgent, &messages),
        Some(reasoner_constants::LONGFORM_STEP_MAX_TOKENS)
    );
}

#[test]
fn missing_content_error_generation_turn_uses_longform_output_budget() {
    let provider = Arc::new(LocalCaptureProvider::new());
    let mut config = ReasonerConfig::default();
    config.max_tokens = Some(128_000);
    let reasoner = Reasoner::new(
        provider,
        config,
        ToolSet::new(),
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let error = r#"{
  "success": false,
  "recoverable": true,
  "error_kind": "missing_required_content",
  "required_fields": ["content", "project_path", "chapter_number or chapter_title"],
  "example_shape": {
    "action": "write_draft",
    "project_path": "/tmp/novel",
    "chapter_number": 1,
    "content": "<full text to save>"
  }
}"#;
    let messages = vec![
        Message::user("请写一本玄幻小说的第一章并保存".to_string()),
        Message::tool_result("call_novel_studio", error).with_tool_name("novel_studio"),
    ];

    assert!(Reasoner::<CaptureProvider>::turn_requires_generated_artifact_content(&messages));
    assert_eq!(
        reasoner.request_max_tokens_for_turn(
            false,
            None,
            CoordinatorTaskMode::ToolAgent,
            &messages
        ),
        Some(reasoner_constants::LONGFORM_STEP_MAX_TOKENS)
    );
}

#[test]
fn content_required_contract_recovery_rejects_process_notes() {
    let result = r#"{
  "action": "write_draft",
  "error_kind": "missing_required_content",
  "required_fields": ["content"],
  "example_shape": {"action": "write_draft", "content": "<full text to save>"}
}"#;
    let note = "状态报告：我需要继续调用 novel_studio write_draft，但还没有生成正文。接下来我会根据计划进行创作，然后再保存。这个说明只是过程记录，不应该被当成正文。";
    let messages = vec![
        Message::user("请继续写第三章并保存".to_string()),
        Message::tool_result("call_novel_studio", result).with_tool_name("novel_studio"),
    ];

    assert!(
        Reasoner::<CaptureProvider>::content_required_tool_call_from_generated_text(
            &messages, note
        )
        .is_none()
    );
}

#[test]
fn declared_next_action_recovery_continues_same_tool_checkpoint() {
    let result = r#"{
  "success": true,
  "stage": "planner",
  "next_action": "compose_chapter",
  "project_path": "/tmp/novel",
  "chapter_number": 1,
  "state": {"target_units": 500000, "chapters": 0}
}"#;

    let (_marker, tool, args) =
        Reasoner::<CaptureProvider>::declared_next_action_tool_call_from_result(
            "novel_studio",
            result,
        )
        .expect("next action call");

    assert_eq!(tool, "novel_studio");
    assert_eq!(args["action"], "compose_chapter");
    assert_eq!(args["project_path"], "/tmp/novel");
    assert_eq!(args["chapter_number"], 1);
}

#[test]
fn blocked_tool_status_is_not_successful_delegate_result() {
    for result in [
        "status: blocked\nworker: knowledge\nerror_kind: source_alignment_evidence_required\nblockers: source body evidence is required before durable ingestion",
        "status: needs_confirmation\nworker: knowledge\nexecuted_tool: knowledge_manage_document\nresult:\nKnowledge document candidates:\n1. collection: knowledge\npath: manual/example.md",
    ] {
        let messages = vec![
            Message::user("把材料导入知识库后写文章".to_string()),
            Message::tool_result("call_delegate", result).with_tool_name("delegate"),
        ];

        assert!(
            Reasoner::<CaptureProvider>::latest_successful_tool_result_text(&messages, "delegate")
                .is_none()
        );
        assert!(Reasoner::<CaptureProvider>::latest_successful_tool_result(&messages).is_none());
        let (tool_name, blocked) =
            Reasoner::<CaptureProvider>::latest_blocked_tool_result(&messages)
                .expect("blocked tool result should remain available for final delivery");
        assert_eq!(tool_name, "delegate");
        assert!(blocked.contains("status:"));
    }
}

#[test]
fn bare_tool_invocation_file_content_error_is_recoverable_contract() {
    let result = "Error executing tool 'write_file': Tool execution error: write_file - Runtime tool error in `write_file`: content looks like a bare tool invocation, not file content; call that tool directly and write only the resulting material";

    assert!(Reasoner::<CaptureProvider>::tool_error_is_recoverable_contract(result));

    let prompt = Reasoner::<CaptureProvider>::tool_contract_recovery_prompt(
        "write_file",
        result,
        &["fetch_document".to_string(), "novel_studio".to_string()],
    );
    assert!(prompt.contains("bare tool invocation"));
    assert!(prompt.contains("call that separate equipped tool directly"));
    assert!(prompt.contains("never values for another tool's `action` field or file `content`"));
}

#[test]
fn tool_contract_recovery_marker_is_scoped_to_failed_action() {
    let first = r#"{
  "action": "add_source",
  "error_kind": "missing_required_content",
  "missing_required": ["content"]
}"#;
    let second = r#"{
  "action": "write_draft",
  "error_kind": "missing_required_content",
  "missing_required": ["content"]
}"#;

    let first_marker = Reasoner::<CaptureProvider>::tool_contract_recovery_marker(
        "BENSHU_TOOL_CONTRACT_RECOVERY",
        "novel_studio",
        first,
    );
    let second_marker = Reasoner::<CaptureProvider>::tool_contract_recovery_marker(
        "BENSHU_TOOL_CONTRACT_RECOVERY",
        "novel_studio",
        second,
    );

    assert_ne!(first_marker, second_marker);
}

#[test]
fn delegated_worker_contract_recovery_redelegates_same_worker_with_body_instruction() {
    let result = r#"status: blocked
worker: writer
blockers: delegated worker returned a structured tool contract error before producing a reliable result
runtime_error_preview: {
  "action": "write_draft",
  "error_kind": "missing_required_content",
  "required_fields": ["content", "project_path", "chapter_number or chapter_title"],
  "example_shape": {"content": "<full text to save>"},
  "next_step_hint": "Generate the actual body text first, then call this action again."
}"#;

    let (id, tool, args) = Reasoner::<CaptureProvider>::worker_tool_contract_recovery_delegate_call(
        7,
        "writer",
        "请继续写第三章并保存",
        result,
    );

    assert_eq!(id, "orchestrated-worker-tool-contract-recovery-7");
    assert_eq!(tool, "delegate");
    assert_eq!(args["role"], "writer");
    let task = args["task"].as_str().expect("task text");
    assert!(task.contains("same delegated task"));
    assert!(task.contains("actual body/content"));
    assert!(task.contains("runtime can attach that body"));
    assert!(task.contains("retrieval/read tool"));
    assert!(task.contains("Tool names are top-level calls"));
    assert!(task.contains("Do not call write/edit/file tools"));
    assert!(task.contains("example_shape"));
    assert!(task.contains("required_fields"));
    assert!(task.contains("next_step_hint"));
    assert!(task.contains("请继续写第三章并保存"));
}

#[test]
fn delegated_worker_contract_recovery_preserves_artifact_repair_context() {
    let result = r#"status: blocked
worker: writer
runtime_error_preview: {
  "success": false,
  "project_path": "/home/user/app/data/generated/novels/九霄劫尘录",
  "artifact_path": "/home/user/app/data/generated/novels/九霄劫尘录/chapters/0001.md",
  "runtime_effect": "artifact.needs_revision",
  "status": "needs_revision",
  "next_action": "revise_chapter",
  "quality_gate": {"passed": false, "issues": ["contract character missing: 林霄"]}
}"#;

    let (_, _, args) = Reasoner::<CaptureProvider>::worker_tool_contract_recovery_delegate_call(
        8,
        "writer",
        "请继续写完整小说并保存成 txt 文档",
        result,
    );

    let task = args["task"].as_str().expect("task text");
    assert!(task.contains("Existing artifact/work-in-progress context"));
    assert!(task.contains("/home/user/app/data/generated/novels/九霄劫尘录"));
    assert!(task.contains("revise_chapter"));
    assert!(task.contains("Do not call init/create/new"));
}

#[test]
fn tool_observation_not_found_is_not_contract_error() {
    let result = r#"{
  "alternative_projects": [],
  "error": "chapter 3 not found in selected project",
  "error_kind": "chapter_not_found",
  "recoverable": true,
  "success": false,
  "next_step_hint": "Continue by composing from the selected project's latest available chapter."
}"#;

    assert!(!Reasoner::<CaptureProvider>::tool_result_content_is_runtime_error(result));
    assert!(!Reasoner::<CaptureProvider>::tool_error_is_recoverable_contract(result));
}

#[test]
fn delegated_worker_tool_boundary_error_is_detected_without_equipping_delegate_to_worker() {
    let error = "status: blocked\nworker: writer\nblockers: delegated worker hit a tool boundary or runtime execution boundary before producing a reliable result\navailable_tools: edit_file, novel_studio, write_file, writing_studio\nruntime_error_preview: Runtime tool error in `delegate`: tool is not equipped for this agent. Available tools right now: edit_file, novel_studio, write_file, writing_studio.";

    assert!(Reasoner::<CaptureProvider>::delegate_worker_tool_boundary_error(error));
    assert_eq!(
        Reasoner::<CaptureProvider>::delegate_worker_role_from_error(error).as_deref(),
        Some("writer")
    );
    assert_eq!(
        Reasoner::<CaptureProvider>::available_tools_from_not_equipped_error(error).as_deref(),
        Some("edit_file, novel_studio, write_file, writing_studio")
    );

    let (_, tool, args) = Reasoner::<CaptureProvider>::worker_tool_boundary_recovery_delegate_call(
        7,
        "writer",
        "修订第二章",
        error,
    );
    assert_eq!(tool, "delegate");
    assert_eq!(args["role"], "writer");
    assert!(args["task"]
        .as_str()
        .expect("task")
        .contains("Do not call unavailable orchestration"));
}

#[test]
fn tool_boundary_recovery_does_not_misread_nested_worker_error_for_equipped_tool() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "delegate" });

    let reasoner = Reasoner::new(
        provider,
        ReasonerConfig::default(),
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let nested_worker_error = "Runtime tool error in `delegate`: worker `researcher` returned a runtime failure instead of completed delegated output: Runtime tool error in `delegate`: tool is not equipped for this agent. Available tools right now: browser_browse, web_fetch, web_search.";

    assert!(!reasoner.should_retry_tool_boundary_recovery("delegate", nested_worker_error));
}

#[test]
fn loop_guard_recovery_prompt_excludes_repeated_tool_when_alternatives_exist() {
    let error = "Runtime tool error in `web_search`: Loop prevention triggered. CRITICAL: 'web_search' has been called 4 times in this session. This indicates a plan stagnation.";
    let prompt = Reasoner::<CaptureProvider>::loop_guard_recovery_prompt(
        "web_search",
        error,
        &[
            "web_search".to_string(),
            "web_fetch".to_string(),
            "browser_browse".to_string(),
        ],
    )
    .expect("alternate tools should produce a recovery prompt");

    assert!(Reasoner::<CaptureProvider>::tool_error_is_loop_prevention(
        error
    ));
    assert!(prompt.contains("BENSHU_LOOP_GUARD_RECOVERY"));
    assert!(prompt.contains("Do not call `web_search` again"));
    assert!(prompt.contains("web_fetch, browser_browse"));
    assert!(!prompt.contains("alternative currently available tools: web_search"));
}

#[test]
fn repeated_empty_lookup_result_requests_observation_recovery() {
    let messages = vec![
        Message::user("查找公开网页列表并保存证据".to_string()),
        Message::tool_result(
            "call_1",
            "[]\n\n---\n### NOTICE: First use of skill 'web_search'.",
        )
        .with_tool_name("web_search"),
        Message::tool_result("call_2", "{\"kind\":\"web_search\",\"results\":[]}")
            .with_tool_name("web_search"),
    ];

    let recovered = Reasoner::<CaptureProvider>::latest_repeated_empty_lookup_result(&messages)
        .expect("repeated empty lookup should be detected");

    assert!(recovered.contains("results"));
}

#[test]
fn repeated_blocked_lookup_result_requests_observation_recovery() {
    let messages = vec![
        Message::user("请在公网查找可以下载的资料并保存证据".to_string()),
        Message::tool_result(
            "call_1",
            "status: blocked\nexecuted_tool: web_search\nresults: []\nblockers: no candidate search results survived source retrieval and relevance filtering",
        )
        .with_tool_name("web_search"),
        Message::tool_result(
            "call_2",
            "status: blocked\nexecuted_tool: web_search\nresults: []\nblockers: no candidate search results survived source retrieval and relevance filtering",
        )
        .with_tool_name("web_search"),
    ];

    let recovered = Reasoner::<CaptureProvider>::latest_repeated_empty_lookup_result(&messages)
        .expect("repeated blocked lookup should be detected");

    assert!(recovered.contains("status: blocked"));
    assert!(recovered.contains("results: []"));
}

#[test]
fn reused_empty_lookup_result_is_not_successful_delivery() {
    let mut reused = Message::tool_result(
        "call_2",
        "[]\n\n---\n### NOTICE: First use of skill 'web_search'.",
    )
    .with_tool_name("web_search");
    reused
        .metadata
        .insert("loop_guard_reused_previous".to_string(), "true".to_string());

    let messages = vec![
        Message::user("查找公开网页列表并保存证据".to_string()),
        reused,
    ];

    let (tool, result) = Reasoner::<CaptureProvider>::latest_reused_empty_lookup_result(&messages)
        .expect("reused empty lookup should be detected");

    assert_eq!(tool, "web_search");
    assert!(result.contains("[]"));
}

#[test]
fn observation_recovery_tool_call_uses_browser_browse_semantic_surface() {
    let (_, tool, args) =
        Reasoner::<CaptureProvider>::observation_recovery_tool_call(3, "查找公开网页列表");

    assert_eq!(tool, "browser_browse");
    assert_eq!(args["action"], "search");
    assert_eq!(args["text"], "查找公开网页列表");
    assert_eq!(args["structured"], true);
}

#[test]
fn synthesize_successful_tool_delivery_uses_generated_image_path() {
    let messages = vec![
        Message::user("请帮我生成一张可爱的猫咪图片".to_string()),
        Message::tool_result(
            "call_1",
            "🖼️ Image successfully generated (via local-image-model) and saved to: /tmp/cat.png",
        )
        .with_tool_name("generate_image"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("图片已经生成完成"));
    assert!(synthesized.contains("/tmp/cat.png"));
}

#[test]
fn synthesize_successful_tool_delivery_falls_back_to_generic_tool_success() {
    let messages = vec![
        Message::user("请帮我执行这个动作".to_string()),
        Message::tool_result("call_2", "Action finished successfully")
            .with_tool_name("run_workflow"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("请求已执行完成"));
    assert!(synthesized.contains("run_workflow"));
    assert!(synthesized.contains("Action finished successfully"));
}

#[test]
fn direct_tool_display_delivery_uses_finalizable_display_payload() {
    let messages = vec![
        Message::user("帮我查一下广州天气".to_string()),
        Message::tool_result(
            "call_weather",
            r#"{
              "display": {
                "zh": "广州当前天气，毛毛雨，气温 27.0°C。来源：open-meteo。",
                "en": "Current weather for Guangzhou is drizzle."
              },
              "orchestration_decision": {
                "can_finalize_answer": true,
                "requires_followup": false
              },
              "realtime_receipt": {
                "status": "verified",
                "freshness": {"ok": true},
                "sources": [
                  {
                    "title": "Open-Meteo Forecast",
                    "url": "https://api.open-meteo.com/v1/forecast",
                    "observed_at": "2026-05-20T00:00:00Z"
                  }
                ],
                "blockers": []
              }
            }"#,
        )
        .with_tool_name("weather_lookup"),
    ];

    let delivered =
        Reasoner::<CaptureProvider>::direct_tool_display_delivery(&messages, "帮我查一下广州天气")
            .expect("display delivery");

    assert!(delivered.contains("广州当前天气"));
    assert!(delivered.contains("open-meteo"));
}

#[test]
fn direct_tool_display_delivery_tolerates_first_use_tool_injection_suffix() {
    let messages = vec![
        Message::user("帮我查一下广州天气".to_string()),
        Message::tool_result(
            "call_weather",
            r#"{
              "display": {
                "zh": "广州当前天气，毛毛雨，气温 27.0°C。来源：open-meteo。"
              },
              "orchestration_decision": {
                "can_finalize_answer": true,
                "requires_followup": false
              },
              "realtime_receipt": {
                "status": "verified",
                "freshness": {"ok": true},
                "sources": [
                  {
                    "title": "Open-Meteo Forecast",
                    "url": "https://api.open-meteo.com/v1/forecast",
                    "observed_at": "2026-05-20T00:00:00Z"
                  }
                ],
                "blockers": []
              }
            }

---
工具提示：这是第一次使用 weather_lookup。"#,
        )
        .with_tool_name("weather_lookup"),
    ];

    let delivered =
        Reasoner::<CaptureProvider>::direct_tool_display_delivery(&messages, "帮我查一下广州天气")
            .expect("display delivery");

    assert!(delivered.contains("广州当前天气"));
    assert!(!delivered.contains("工具提示"));
}

#[test]
fn direct_tool_trace_display_delivery_finalizes_latest_info_results() {
    let result = r#"{
      "display": {
        "zh": "已找到近期公开来源：\n1. 示例新闻 - https://example.com/news"
      },
      "orchestration_decision": {
        "can_finalize_answer": true,
        "requires_followup": false
      },
      "realtime_receipt": {
        "status": "verified",
        "freshness": {"ok": true},
        "sources": [
          {
            "title": "示例新闻",
            "url": "https://example.com/news",
            "observed_at": "2026-05-24T00:00:00Z"
          }
        ],
        "blockers": []
      }
    }"#;
    let trace = vec![ToolCallData {
        receipt_id: None,
        tool_call_id: None,
        name: "latest_info_lookup".to_string(),
        args: "{\"topic\":\"今天有什么重要时事新闻？\"}".to_string(),
        result: Some(result.to_string()),
        backup: None,
        duration_ms: 123,
        timestamp: 0,
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
    }];

    let delivered = Reasoner::<CaptureProvider>::direct_tool_trace_display_delivery(
        &trace,
        "今天有什么重要时事新闻？",
        "latest_info_lookup",
    )
    .expect("latest info display");

    assert!(delivered.contains("已找到近期公开来源"));
    assert!(delivered.contains("https://example.com/news"));
}

#[test]
fn direct_tool_trace_display_delivery_reads_spilled_full_result_when_truncated() {
    let result = r#"{
      "display": {
        "zh": "已找到近期公开来源：\n1. 示例新闻 - https://example.com/news"
      },
      "orchestration_decision": {
        "can_finalize_answer": true,
        "requires_followup": false
      },
      "realtime_receipt": {
        "status": "verified",
        "freshness": {"ok": true},
        "sources": [
          {
            "title": "示例新闻",
            "url": "https://example.com/news",
            "observed_at": "2026-05-24T00:00:00Z"
          }
        ],
        "blockers": []
      }
    }"#;
    let tempdir = tempfile::tempdir().expect("tempdir");
    let full_result_path = tempdir.path().join("latest_info_lookup-full.txt");
    std::fs::write(&full_result_path, result).expect("write full result");
    let trace = vec![ToolCallData {
        receipt_id: None,
        tool_call_id: None,
        name: "latest_info_lookup".to_string(),
        args: "{\"topic\":\"今天有什么重要时事新闻？\"}".to_string(),
        result: Some(
            "{\n  \"display\": {\n\n[Note: Output truncated; 2000 characters omitted]\n\n}"
                .to_string(),
        ),
        backup: None,
        duration_ms: 123,
        timestamp: 0,
        caller_id: None,
        safety_level: SafetyLevel::Green,
        cpu_pressure: None,
        vram_pressure: None,
        result_truncated: true,
        result_original_chars: Some(result.chars().count()),
        result_omitted_chars: Some(2000),
        args_fingerprint: None,
        result_fingerprint: None,
        outcome: Some(ToolOutcomeMeta {
            status: "completed".to_string(),
            kind: "realtime".to_string(),
            error_class: None,
            preview_chars: None,
            full_artifact_ref: Some(full_result_path.to_string_lossy().to_string()),
            evidence_count: Some(1),
            progress_signal: false,
        }),
        replay: None,
    }];

    let delivered = Reasoner::<CaptureProvider>::direct_tool_trace_display_delivery(
        &trace,
        "今天有什么重要时事新闻？",
        "latest_info_lookup",
    )
    .expect("latest info display from full artifact");

    assert!(delivered.contains("已找到近期公开来源"));
    assert!(delivered.contains("https://example.com/news"));
}

#[test]
fn direct_realtime_display_requires_verified_receipt() {
    let messages = vec![
        Message::user("帮我查一下广州天气".to_string()),
        Message::tool_result(
            "call_weather",
            r#"{
              "display": {
                "zh": "广州当前天气，毛毛雨，气温 27.0°C。来源：open-meteo。"
              },
              "orchestration_decision": {
                "can_finalize_answer": true,
                "requires_followup": false
              }
            }"#,
        )
        .with_tool_name("weather_lookup"),
    ];

    assert!(Reasoner::<CaptureProvider>::direct_tool_display_delivery(
        &messages,
        "帮我查一下广州天气"
    )
    .is_none());
}

#[test]
fn direct_tool_display_delivery_defers_durable_goals() {
    let messages = vec![
        Message::user("搜索资料并存到知识库".to_string()),
        Message::tool_result(
            "call_lookup",
            r#"{
              "display": {"zh": "找到一个来源。"},
              "orchestration_decision": {
                "can_finalize_answer": true,
                "requires_followup": false
              }
            }"#,
        )
        .with_tool_name("web_search"),
    ];

    assert!(Reasoner::<CaptureProvider>::direct_tool_display_delivery(
        &messages,
        "搜索资料并存到知识库"
    )
    .is_none());
}

#[test]
fn direct_realtime_tool_call_extracts_simple_price_and_weather_args() {
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "price_lookup",
    });
    tools.add(StaticTool {
        name: "weather_lookup",
    });
    tools.add(StaticTool { name: "fx_lookup" });
    let reasoner = Reasoner::<CaptureProvider>::new(
        Arc::new(CaptureProvider::new()),
        ReasonerConfig::default(),
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let (_, price_tool, price_args) = reasoner
        .direct_realtime_tool_call_for_query("帮我查一下比特币现在的价格")
        .expect("price lookup");
    assert_eq!(price_tool, "price_lookup");
    assert_eq!(price_args["symbol"], "比特币");

    let (_, sourced_price_tool, sourced_price_args) = reasoner
        .direct_realtime_tool_call_for_query("帮我查一下比特币现在的价格，用中文回答并给出来源。")
        .expect("sourced price lookup");
    assert_eq!(sourced_price_tool, "price_lookup");
    assert_eq!(sourced_price_args["symbol"], "比特币");

    let (_, index_tool, index_args) = reasoner
        .direct_realtime_tool_call_for_query("纳斯达克点数多少？")
        .expect("index price lookup");
    assert_eq!(index_tool, "price_lookup");
    assert_eq!(index_args["symbol"], "纳斯达克");

    let (_, crypto_tool, crypto_args) = reasoner
        .direct_realtime_tool_call_for_query("比特币现在多少钱？")
        .expect("crypto price lookup");
    assert_eq!(crypto_tool, "price_lookup");
    assert_eq!(crypto_args["symbol"], "比特币");

    let (_, eth_tool, eth_args) = reasoner
        .direct_realtime_tool_call_for_query("现在以太坊多少钱？")
        .expect("ethereum price lookup");
    assert_eq!(eth_tool, "price_lookup");
    assert_eq!(eth_args["symbol"], "以太坊");

    let (_, stock_tool, stock_args) = reasoner
        .direct_realtime_tool_call_for_query("AAPL 股票现在多少钱？")
        .expect("stock price lookup");
    assert_eq!(stock_tool, "price_lookup");
    assert_eq!(stock_args["symbol"], "AAPL");
    assert!(reasoner
        .direct_realtime_tool_call_for_query("苹果股票现在多少钱？")
        .is_none());

    let (_, weather_tool, weather_args) = reasoner
        .direct_realtime_tool_call_for_query("帮我查一下广州现在的气温和天气")
        .expect("weather lookup");
    assert_eq!(weather_tool, "weather_lookup");
    assert_eq!(weather_args["location"], "广州");

    let (_, sourced_weather_tool, sourced_weather_args) = reasoner
        .direct_realtime_tool_call_for_query(
            "帮我查一下广州现在的气温和天气，用中文回答并给出来源。",
        )
        .expect("sourced weather lookup");
    assert_eq!(sourced_weather_tool, "weather_lookup");
    assert_eq!(sourced_weather_args["location"], "广州");

    let followup_messages = vec![
        Message::system(
            "Latest session checkpoints: agent:benshu:tool:weather_lookup:end success=true"
                .to_string(),
        ),
        Message::user("那上海呢？".to_string()),
    ];
    let (_, followup_tool, followup_args) = reasoner
        .direct_realtime_followup_tool_call_for_query(&followup_messages, "那上海呢？")
        .expect("weather followup lookup");
    assert_eq!(followup_tool, "weather_lookup");
    assert_eq!(followup_args["location"], "上海");

    let (_, fx_tool, fx_args) = reasoner
        .direct_realtime_tool_call_for_query("美元兑人民币现在多少？")
        .expect("fx lookup");
    assert_eq!(fx_tool, "fx_lookup");
    assert_eq!(fx_args["base_currency"], "USD");
    assert_eq!(fx_args["quote_currency"], "CNY");

    assert!(reasoner
        .direct_realtime_tool_call_for_query("请让 researcher 搜索今天人工智能领域的最新新闻。")
        .is_none());
}

#[test]
fn direct_realtime_tool_call_defers_compound_followup_tasks() {
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "latest_info_lookup",
    });
    let reasoner = Reasoner::<CaptureProvider>::new(
        Arc::new(CaptureProvider::new()),
        ReasonerConfig::default(),
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    assert!(reasoner
        .direct_realtime_tool_call_for_query("查找最新论文并存到知识库，然后写成 PDF")
        .is_none());
}

#[test]
fn direct_realtime_tool_call_defers_creation_planning_dialogue() {
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "latest_info_lookup",
    });
    let reasoner = Reasoner::<CaptureProvider>::new(
        Arc::new(CaptureProvider::new()),
        ReasonerConfig::default(),
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    assert!(reasoner
        .direct_realtime_tool_call_for_query(
            "[BENSHU_CREATION_PLANNING_DIALOGUE]\n写一部5万字爱情小说，先定框架。"
        )
        .is_none());
}

#[test]
fn synthesize_successful_tool_delivery_does_not_mark_blocked_lookup_complete() {
    let messages = vec![
        Message::user("请在公网查找资料并保存到知识库".to_string()),
        Message::tool_result(
            "call_blocked",
            "status: blocked\nexecuted_tool: web_search\nresults: []\nblockers: no candidate search results survived source retrieval and relevance filtering",
        )
        .with_tool_name("web_search"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("blocked tool result should be summarized");

    assert!(synthesized.contains("不能声明知识库写入完成"));
    assert!(synthesized.contains("当前具体卡点"));
    assert!(!synthesized.contains("请求已执行完成"));
}

#[test]
fn synthesize_successful_tool_delivery_does_not_complete_durable_goal_from_fetch_only() {
    let messages = vec![
        Message::user("请搜索公开资料，把内容存到知识库里，然后写成 txt 文档。".to_string()),
        Message::tool_result(
            "call_fetch",
            "Fetched page title\nA listing page with several links but no import receipt.",
        )
        .with_tool_name("web_fetch"),
    ];

    assert!(Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages).is_none());
}

#[test]
fn current_turn_tool_delivery_ignores_previous_turn_tool_error() {
    let messages = vec![
        Message::user("请帮我识别图片文字".to_string()),
        Message::runtime_tool_error_result(
            "old_call",
            "manage_facts",
            "error executing tool: ocr.ocr(path=\"/tmp/old.png\") is not available",
        )
        .with_tool_name("manage_facts"),
        Message::user("请委托 office 子agent 解析 Word 文件并返回 sentinel".to_string()),
        Message::tool_result("new_call", "BenShu DOCX SENTINEL rose-817 张三 13800001111")
            .with_tool_name("run_workflow"),
    ];

    assert!(Reasoner::<CaptureProvider>::latest_tool_error_result(&messages).is_none());

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized current-turn result");

    assert!(synthesized.contains("rose-817"));
    assert!(!synthesized.contains("manage_facts"));
    assert!(!synthesized.contains("ocr.ocr"));
}

#[test]
fn latest_tool_error_ignores_stale_same_turn_error_after_progress() {
    let messages = vec![
        Message::user("请查资料并继续执行".to_string()),
        Message::runtime_tool_error_result(
            "bad_call",
            "delegate",
            "Runtime tool error in `delegate`: tool is not equipped for this agent.",
        )
        .with_tool_name("delegate"),
        Message::tool_result(
            "good_call",
            "status: completed\nworker: researcher\nexecuted_tool: web_search\nresult: concrete progress",
        )
        .with_tool_name("web_search"),
    ];

    assert!(Reasoner::<CaptureProvider>::latest_tool_error_result(&messages).is_none());
}

#[test]
fn synthesize_successful_tool_delivery_uses_knowledge_search_snippet() {
    let messages = vec![
            Message::user(
                "请只根据知识库回答：BENSHU_RAG_TEST_SENTINEL 对应的特殊验证答案是什么？"
                    .to_string(),
            ),
            Message::tool_result(
                "call_3",
                "### Knowledge Search Results\n\n1. **demo**\n   *Snippet*: BENSHU_RAG_TEST_SENTINEL: cobalt-velvet-7421 The special verification answer is: 蓝色天鹅绒7421。 ...",
            )
            .with_tool_name("knowledge_search"),
        ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert_eq!(synthesized, "根据知识库，答案是：蓝色天鹅绒7421");
}

#[test]
fn synthesize_successful_tool_delivery_strips_first_use_notice_from_generic_tool_output() {
    let messages = vec![
            Message::user("请执行测试动作".to_string()),
            Message::tool_result(
                "call_4",
                "Action finished successfully\n---\n### NOTICE: First use of skill 'demo'.\n#### Official TypeScript Schema:\n```typescript\ninterface Demo {}\n```",
            )
            .with_tool_name("run_workflow"),
        ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("Action finished successfully"));
    assert!(!synthesized.contains("NOTICE"));
    assert!(!synthesized.contains("TypeScript Schema"));
}

#[test]
fn synthesize_successful_tool_delivery_summarizes_remember_this_cleanly() {
    let messages = vec![
            Message::user("请记住我的测试标记是 abc_marker_123".to_string()),
            Message::tool_result(
                "call_memory",
                "Memory successfully saved as 'Memory Panel Test Marker' in collection 'testing'.\n\n---\n### NOTICE: First use of skill 'remember_this'.",
            )
            .with_tool_name("remember_this"),
        ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert_eq!(synthesized, "我已经记住了。");
}

#[test]
fn best_lookup_source_url_for_query_rejects_irrelevant_search_garbage() {
    let query = "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。";
    let result = r#"{
            "kind":"web_search",
            "results":[
                {"title":"Google Search Help","url":"https://support.google.com/websearch/?hl=en","snippet":"help"},
                {"title":"Some unrelated page","url":"https://example.com/unrelated","snippet":"nothing useful here"}
            ]
        }"#;

    let best = Reasoner::<CaptureProvider>::best_lookup_source_url_for_query(query, result);
    assert!(best.is_none());
}

#[test]
fn best_lookup_source_url_for_query_prefers_specific_academic_records() {
    let query = "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。请优先使用 DOI、PubMed、PMC 或其他开放来源。";
    let result = r#"{
            "kind":"web_search",
            "results":[
                {
                    "title":"PubMed",
                    "url":"https://pubmed.ncbi.nlm.nih.gov/",
                    "snippet":"PubMed homepage"
                },
                {
                    "title":"Lancet trial full text",
                    "url":"https://www.thelancet.com/journals/lancet/article/PIIS0140-6736(25)01665-4/fulltext",
                    "snippet":"A Lancet cardiovascular heart disease treatment paper."
                },
                {
                    "title":"Heart disease treatment trial",
                    "url":"https://pubmed.ncbi.nlm.nih.gov/40234567/",
                    "snippet":"A PubMed record for a Lancet cardiovascular heart disease trial."
                }
            ]
        }"#;

    let best = Reasoner::<CaptureProvider>::best_lookup_source_url_for_query(query, result)
        .expect("best source");
    assert_eq!(best, "https://pubmed.ncbi.nlm.nih.gov/40234567/");
}

#[test]
fn followup_execution_source_url_prefers_explicit_delegate_source_url() {
    let query = "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。";
    let result = r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=42020654&retmode=json
result:
{
  "backend":"http_fetch",
  "url":"https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&term=lancet"
}"#;

    let best = Reasoner::<CaptureProvider>::followup_execution_source_url(query, result)
        .expect("follow-up source url");
    assert_eq!(
            best,
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=42020654&retmode=json"
        );
}

#[test]
fn followup_execution_source_url_rejects_search_page_as_import_source() {
    let query = "请搜索可下载免费小说并保存进知识库。";
    let result = r#"status: completed
worker: researcher
executed_tool: browser_browse
lookup_strategy: browser_search
source_url: https://www.bing.com/search?q=downloadable+free+fantasy+novels
result:
{
  "engine":"bing",
  "results":[
    {
      "title":"Example Novel",
      "url":"https://example.com/example-novel.txt",
      "snippet":"downloadable free fantasy novel"
    }
  ]
}"#;

    let best = Reasoner::<CaptureProvider>::followup_execution_source_url(query, result)
        .expect("follow-up source url");
    assert_eq!(best, "https://example.com/example-novel.txt");
}

#[test]
fn knowledge_persistence_detection_does_not_treat_readback_as_reimport() {
    assert!(
        Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。"
        )
    );
    assert!(
        Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "请搜索起点中文网免费玄幻小说，把公开元数据放进知识库。"
        )
    );
    assert!(
        Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "请整理这些资料并收进知识库。"
        )
    );
    assert!(
        Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "查找柳叶刀最近治疗心脏病的论文，然后存入数据库。"
        )
    );
    assert!(
        Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "Find the latest treatment papers and save them to the document store."
        )
    );
    assert!(
        !Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "请从知识库里读出你刚刚保存的那条柳叶刀心脏病治疗相关资料，告诉我标题和摘要。"
        )
    );
    assert!(
        !Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "请从数据库里读出刚保存的论文标题和摘要。"
        )
    );
    assert!(
        !Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "请测试 web_fetch，不要写入知识库。"
        )
    );
    assert!(
        !Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "请测试 web_fetch，不要写入数据库。"
        )
    );
    assert!(
        !Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "Please fetch this URL, do not save to the knowledge base."
        )
    );
    assert!(
            Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
                "把这10部小说的公开元数据放进知识库：书名、作者、链接、来源、简短公开简介/你自己的摘要，不要抓取或保存小说正文。"
            )
        );
    assert!(
        Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(
            "Save the public metadata to the knowledge base. Do not scrape or save the full text."
        )
    );
}

#[test]
fn qidian_researcher_fetch_requires_knowledge_followup() {
    let query = "请搜索起点中文网当前可公开访问的免费玄幻小说，找出前10部，把公开元数据放进知识库，然后原创写一部新的玄幻小说。";
    let messages = vec![
            Message::user(query.to_string()),
            Message::tool_result(
                "call_qidian",
                r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://www.qidian.com/rank/chn21/
result_summary:
- 1. 星河之主 | public metadata: 玄幻·烽仙 | source: https://www.qidian.com/rank/chn21/
- 2. 苟在武道世界成圣 | public metadata: metadata not visible in fetched source | source: https://www.qidian.com/rank/chn21/
fetched_result: {"backend":"browser_snapshot_fallback_low_quality_static","url":"https://www.qidian.com/rank/chn21/"}"#,
            )
            .with_tool_name("delegate"),
        ];

    assert!(Reasoner::<CaptureProvider>::query_requests_knowledge_persistence(query));
    assert!(
        Reasoner::<CaptureProvider>::latest_lookup_result_for_followup_execution(&messages)
            .is_some()
    );
    assert!(!Reasoner::<CaptureProvider>::should_prioritize_followup_execution(query, &messages));
    let gap = Reasoner::<CaptureProvider>::collection_evidence_gap_for_query(
        query,
        &Reasoner::<CaptureProvider>::latest_lookup_result_for_followup_execution(&messages)
            .expect("lookup result"),
    )
    .expect("two records should not satisfy a top ten request");
    assert_eq!(gap.observed, 2);
    assert_eq!(gap.requested, 10);
    assert_eq!(
        Reasoner::<CaptureProvider>::followup_execution_source_url(
            query,
            &Reasoner::<CaptureProvider>::latest_lookup_result_for_followup_execution(&messages)
                .expect("lookup result")
        )
        .as_deref(),
        Some("https://www.qidian.com/rank/chn21/")
    );
}

#[test]
fn verification_challenge_detection_does_not_discard_ranked_evidence() {
    let content = r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/rank
result_summary:
- 1. Alpha | public metadata: category | source: https://example.com/rank
- 2. Beta | public metadata: category | source: https://example.com/rank
- 3. Gamma | public metadata: category | source: https://example.com/rank
fetched_result: {"backend":"browser_snapshot_fallback_low_quality_static","content":"请稍候 ... but the page also contains public ranked records"}"#;

    assert!(!Reasoner::<CaptureProvider>::content_contains_verification_challenge(content));
}

#[test]
fn blocked_researcher_lookup_triggers_browser_escalation_candidate() {
    let query = "搜索起点中文网玄幻小说免费榜前十，把公开元数据存到知识库。";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result(
            "call_researcher",
            r#"status: blocked
worker: researcher
executed_tool: web_fetch
source_url: https://www.qidian.com/free/all/chanId21/
blockers: fetched source did not provide enough verified evidence to answer
fetched_result: {"content_quality":"challenge","content":"security verification challenge"}"#,
        )
        .with_tool_name("delegate"),
    ];

    let blocked =
        Reasoner::<CaptureProvider>::latest_blocked_lookup_result_requiring_browser(&messages)
            .expect("blocked lookup should be browser-relevant");
    let (_, role, args) =
        Reasoner::<CaptureProvider>::browser_escalation_delegate_call(1, query, &blocked);

    assert_eq!(role, "delegate");
    assert_eq!(
        args.get("role").and_then(|value| value.as_str()),
        Some("browser")
    );
    assert!(args
        .get("task")
        .and_then(|value| value.as_str())
        .is_some_and(|task| task.contains("Candidate URL")));
}

#[test]
fn weak_lookup_evidence_recovery_uses_generic_hardness_decision() {
    let query = "搜索公开来源前十条记录，把公开元数据存到知识库。";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result(
            "call_researcher",
            r#"status: blocked
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/rank
blockers: fetched source did not provide enough verified evidence to answer
fetched_result: {"content_quality":"challenge","content":"security verification challenge"}"#,
        )
        .with_tool_name("delegate"),
    ];
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "delegate" });
    let reasoner = Reasoner::new(
        provider,
        ReasonerConfig::default(),
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let result =
        Reasoner::<CaptureProvider>::latest_blocked_lookup_result_requiring_browser(&messages)
            .expect("blocked lookup");

    assert_eq!(
        reasoner.lookup_recovery_action_for_result(&messages, query, &result, 2, 8),
        benshu_hardness::RecoveryAction::SwitchObservationSurface
    );
}

#[test]
fn weak_lookup_evidence_recovery_stops_after_observation_marker() {
    let query = "搜索公开来源前十条记录，把公开元数据存到知识库。";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result(
            "call_researcher",
            r#"status: blocked
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/rank
blockers: fetched source did not provide enough verified evidence to answer
fetched_result: {"content_quality":"challenge","content":"security verification challenge"}"#,
        )
        .with_tool_name("delegate"),
        Message::system("BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY".to_string()),
    ];
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool { name: "delegate" });
    let reasoner = Reasoner::new(
        provider,
        ReasonerConfig::default(),
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );
    let result =
        Reasoner::<CaptureProvider>::latest_blocked_lookup_result_requiring_browser(&messages)
            .expect("blocked lookup");

    assert_eq!(
        reasoner.lookup_recovery_action_for_result(&messages, query, &result, 6, 8),
        benshu_hardness::RecoveryAction::EmitBlocker
    );
}

#[test]
fn structured_browser_result_can_drive_knowledge_create() {
    let query = "搜索起点中文网玄幻小说免费榜前十，把公开元数据存到知识库。";
    let result = r#"status: completed
worker: browser
executed_tool: browser_browse
source_url: https://www.qidian.com/free/all/chanId21/
result_summary:
- 1. 甲 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/
- 2. 乙 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/
- 3. 丙 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/
- 4. 丁 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/
- 5. 戊 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/
- 6. 己 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/
- 7. 庚 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/
- 8. 辛 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/
- 9. 壬 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/
- 10. 癸 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/free/all/chanId21/"#;
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result("call_browser", result).with_tool_name("delegate"),
    ];

    let evidence =
        Reasoner::<CaptureProvider>::latest_structured_lookup_result_for_knowledge_create(
            &messages, query,
        )
        .expect("ten ranked browser items should be enough evidence");
    let (_, tool, args) =
        Reasoner::<CaptureProvider>::knowledge_create_delegate_call(1, query, &evidence);

    assert_eq!(tool, "delegate");
    assert_eq!(
        args.get("role").and_then(|value| value.as_str()),
        Some("knowledge")
    );
    assert!(args
        .get("task")
        .and_then(|value| value.as_str())
        .is_some_and(|task| task.contains("保存到知识库") && task.contains("已验证公开证据摘要")));
}

#[test]
fn source_content_requests_reject_metadata_surrogate_knowledge_create() {
    let query = "搜索公开资料前十，把这些资料内容存到知识库，再根据知识库写长文。";
    let result = r#"status: completed
worker: browser
executed_tool: browser_browse
source_url: https://example.com/free/
result_summary:
- 1. 甲 | public metadata: category | source: https://example.com/a
- 2. 乙 | public metadata: category | source: https://example.com/b
- 3. 丙 | public metadata: category | source: https://example.com/c
- 4. 丁 | public metadata: category | source: https://example.com/d
- 5. 戊 | public metadata: category | source: https://example.com/e
- 6. 己 | public metadata: category | source: https://example.com/f
- 7. 庚 | public metadata: category | source: https://example.com/g
- 8. 辛 | public metadata: category | source: https://example.com/h
- 9. 壬 | public metadata: category | source: https://example.com/i
- 10. 癸 | public metadata: category | source: https://example.com/j

evidence_scope: public_metadata_surrogate_not_full_source_content
content_policy_note: full source content was not imported"#;
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result("call_browser", result).with_tool_name("delegate"),
    ];

    assert!(
        Reasoner::<CaptureProvider>::latest_structured_lookup_result_for_knowledge_create(
            &messages, query,
        )
        .is_none()
    );
    assert!(
        Reasoner::<CaptureProvider>::latest_metadata_surrogate_lookup_for_requested_source_content(
            &messages, query,
        )
        .is_some()
    );
    let blocker = Reasoner::<CaptureProvider>::metadata_surrogate_depth_blocker(query, result);
    assert!(blocker.contains("没有取得用户要求导入知识库的源正文或可下载内容"));
}

#[test]
fn public_metadata_result_summary_without_body_rejects_source_content_import() {
    let query = "搜索一个热门的玄幻小说，找到可以读取或下载的正文内容，把它存入知识库作为素材。";
    let result = r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://books.example.com/
search_query: 热门的玄幻小说 下载
result_summary:
- 1. 完整榜单 | public metadata: 月票榜 | source: https://books.example.com/
- 2. 理性的残响 | public metadata: 十七岁的主角成为锚点。作者某某，玄幻连载8.6万字，免费 | source: https://books.example.com/book/1
- 3. 武极天下 | public metadata: 玄幻完结1002万字 | source: https://books.example.com/book/2"#;
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result("call_researcher", result).with_tool_name("delegate"),
    ];

    assert!(
        !Reasoner::<CaptureProvider>::lookup_result_satisfies_requested_knowledge_depth(
            query, result,
        )
    );
    assert!(
        Reasoner::<CaptureProvider>::latest_structured_lookup_result_for_knowledge_create(
            &messages, query,
        )
        .is_none()
    );
    assert!(
        Reasoner::<CaptureProvider>::latest_metadata_surrogate_lookup_for_requested_source_content(
            &messages, query,
        )
        .is_some()
    );
}

#[test]
fn collection_index_page_does_not_satisfy_requested_source_content() {
    let query =
        "搜索一部公网可下载的热门作品，把可以读取到的正文或有效素材收进知识库，再写新作品。";
    let result = r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/genre/fantasy/
fetched_result: {
  "content": "List of fantasy works\nNovel list\nLatest Release Most Popular Completed\nGenre Category Sort Filter"
}"#;

    assert!(
        !Reasoner::<CaptureProvider>::lookup_result_satisfies_requested_knowledge_depth(
            query, result,
        )
    );
}

#[test]
fn effective_material_request_still_rejects_metadata_when_source_body_was_requested() {
    let query =
        "搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库，再写新作品。";
    let result = r#"status: completed
worker: researcher
executed_tool: web_search
lookup_strategy: search_index_evidence_fallback
observed_item_records: 1
requested_item_records: 1
evidence_scope: public_metadata_surrogate_not_full_source_content
result_summary:
- 1. Example Xuanhuan | public metadata: xuanhuan novel summary, source: https://example.com/book.txt

search_result_preview:
[{"title":"Example Xuanhuan","url":"https://example.com/book.txt","snippet":"Plain text: https://example.com/book.txt"}]"#;
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result("call_researcher", result).with_tool_name("delegate"),
    ];

    assert!(
        !Reasoner::<CaptureProvider>::lookup_result_satisfies_requested_knowledge_depth(
            query, result,
        )
    );
    assert!(
        Reasoner::<CaptureProvider>::latest_structured_lookup_result_for_knowledge_create(
            &messages, query,
        )
        .is_none()
    );
    assert!(
        Reasoner::<CaptureProvider>::latest_metadata_surrogate_lookup_for_requested_source_content(
            &messages, query,
        )
        .is_some()
    );
}

#[test]
fn explicit_summary_permission_accepts_structured_metadata_surrogate() {
    let query = "搜索一部热门玄幻小说，把公开摘要也可以存到知识库作为素材，再写新作品。";
    let result = r#"status: completed
worker: researcher
executed_tool: web_search
lookup_strategy: search_index_evidence_fallback
observed_item_records: 1
requested_item_records: 1
evidence_scope: public_metadata_surrogate_not_full_source_content
result_summary:
- 1. Example Xuanhuan | public metadata: xuanhuan novel summary, source: https://example.com/book

search_result_preview:
[{"title":"Example Xuanhuan","url":"https://example.com/book","snippet":"Public summary"}]"#;
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result("call_researcher", result).with_tool_name("delegate"),
    ];

    assert!(
        Reasoner::<CaptureProvider>::lookup_result_satisfies_requested_knowledge_depth(
            query, result,
        )
    );
    assert!(
        Reasoner::<CaptureProvider>::latest_structured_lookup_result_for_knowledge_create(
            &messages, query,
        )
        .is_some()
    );
}

#[test]
fn imported_collection_page_receipt_does_not_satisfy_source_content_depth() {
    let query =
        "搜索一部公网可下载的热门作品，把可以读取到的正文或有效素材收进知识库，再写新作品。";
    let result = r#"status: completed
worker: knowledge
executed_tool: knowledge_import_url
result:
runtime_effect: knowledge.imported
collection: references
title: The Most Popular Fantasy on Example (670 books)
source_url: https://example.com/list/show/35857.popular_fantasy"#;

    assert!(
        !Reasoner::<CaptureProvider>::lookup_result_satisfies_requested_knowledge_depth(
            query, result,
        )
    );
}

#[test]
fn imported_plain_text_receipt_satisfies_source_content_depth() {
    let query =
        "搜索一部公网可下载的热门作品，把可以读取到的正文或有效素材收进知识库，再写新作品。";
    let result = r#"status: completed
worker: knowledge
executed_tool: knowledge_import_url
result:
runtime_effect: knowledge.imported
collection: references
title: Example Public Domain Fantasy
source_url: https://example.com/books/example-fantasy.txt"#;

    assert!(
        Reasoner::<CaptureProvider>::lookup_result_satisfies_requested_knowledge_depth(
            query, result,
        )
    );
}

#[test]
fn fetched_source_body_satisfies_generic_material_alignment() {
    let query =
        "搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库，再写新作品。";
    let source_body = r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://www.gutenberg.org/ebooks/67143.txt.utf-8
search_query: 热门玄幻小说 下载
fetched_result:
{
  "content": "The Project Gutenberg eBook of A Romance. This romance novel is readable full text."
}

search_result_preview:
{"query":"热门玄幻小说 下载"}"#;

    assert!(
        Reasoner::<CaptureProvider>::lookup_result_satisfies_requested_material_alignment(
            query,
            source_body,
        )
    );

    let empty_body = r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://www.gutenberg.org/ebooks/example.txt
fetched_result:
{
  "content": ""
}"#;

    assert!(
        !Reasoner::<CaptureProvider>::lookup_result_satisfies_requested_material_alignment(
            query, empty_body,
        )
    );
}

#[test]
fn collection_evidence_gate_parses_generic_chinese_and_english_counts() {
    let chinese_query = "查找前十二篇公开资料，把这些内容存到知识库，再写报告。";
    let english_query =
        "Find up to twelve public documents, import their content, then write a report.";
    let result = r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/list
result_summary:
- 1. Alpha | public metadata: category | source: https://example.com/a
- 2. Beta | public metadata: category | source: https://example.com/b"#;

    let chinese_gap =
        Reasoner::<CaptureProvider>::collection_evidence_gap_for_query(chinese_query, result)
            .expect("Chinese count should require more item-level records");
    assert_eq!(chinese_gap.requested, 12);
    assert_eq!(chinese_gap.observed, 2);

    let english_gap =
        Reasoner::<CaptureProvider>::collection_evidence_gap_for_query(english_query, result)
            .expect("English count should require more item-level records");
    assert_eq!(english_gap.requested, 12);
    assert_eq!(english_gap.observed, 2);
}

#[test]
fn collection_evidence_gate_does_not_apply_to_single_source_tasks() {
    let query = "把这个网页存到知识库，然后根据它写一个txt。";
    let result = r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/article
result_summary:
- 1. Article | public metadata: source page | source: https://example.com/article"#;

    assert!(
        Reasoner::<CaptureProvider>::collection_evidence_gap_for_query(query, result).is_none()
    );
}

#[test]
fn collection_gap_recovery_prioritizes_observation_tools_over_repeat_search() {
    let tools = vec![
        "web_search".to_string(),
        "web_fetch".to_string(),
        "browser_browse".to_string(),
    ];

    assert_eq!(
        Reasoner::<CaptureProvider>::prioritize_observation_tools_after_collection_gap(tools),
        vec!["browser_browse".to_string(), "web_fetch".to_string()]
    );
}

#[test]
fn post_import_delivery_blocks_incomplete_generic_collection_before_synthesis() {
    let query =
        "Find the top 10 public documents, save them to the knowledge base, then write a report.";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result(
            "call_researcher",
            r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/list
result_summary:
- 1. Alpha | public metadata: category | source: https://example.com/a"#,
        )
        .with_tool_name("delegate"),
        Message::tool_result(
            "call_knowledge",
            r#"status: completed
worker: knowledge
executed_tool: knowledge_import_url
result:
Imported web knowledge into collection 'references' at path 'web/example'. Source URL: https://example.com/list"#,
        )
        .with_tool_name("delegate"),
    ];

    let synthesized =
        Reasoner::<CaptureProvider>::synthesize_post_import_delivery(query, &messages)
            .expect("incomplete collection should produce a blocker");
    assert!(synthesized.contains("requires 10 item-level source records"));
    assert!(synthesized.contains("confirms only 1"));
}

#[test]
fn file_artifact_post_import_delivery_continues_to_writer_instead_of_inline_summary() {
    let query = "搜索资料，保存到知识库，根据内容写一部小说，然后保存成txt文档。";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result(
            "call_researcher",
            r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/rank
result_summary:
- 1. Alpha | public metadata: fantasy | source: https://example.com/rank
- 2. Beta | public metadata: fantasy | source: https://example.com/rank
- 3. Gamma | public metadata: fantasy | source: https://example.com/rank"#,
        )
        .with_tool_name("delegate"),
        Message::tool_result(
            "call_knowledge",
            r#"status: completed
worker: knowledge
executed_tool: knowledge_import_url
result:
Imported web knowledge into collection 'references' at path 'web/example'. Source URL: https://example.com/rank"#,
        )
        .with_tool_name("delegate"),
    ];

    assert!(post_import_delivery::query_requests_file_artifact(query));
    assert!(
        Reasoner::<CaptureProvider>::synthesize_post_import_delivery(query, &messages).is_none()
    );
}

#[test]
fn chinese_store_to_knowledge_and_create_novel_is_post_import_delivery() {
    let query = "搜索起点中文网所有玄幻小说免费榜前十，把他们存到知识库中，然后根据这些小说自己创造一个50万字的小说，并保存成txt文档。";

    assert!(post_import_delivery::query_requests_post_import_delivery(
        query
    ));
    assert!(post_import_delivery::query_requests_file_artifact(query));
}

#[test]
fn large_generated_text_routes_to_file_artifact_even_without_explicit_save() {
    let query = "搜索起点中文网前十免费的玄幻小说并且把这些小说内容存到知识库里，进行推理之后写一个50万字的玄幻小说";

    assert!(post_import_delivery::query_requests_post_import_delivery(
        query
    ));
    assert!(post_import_delivery::query_requests_file_artifact(query));
}

#[test]
fn artifact_mutation_detects_revision_of_existing_written_unit() {
    let query = "请继续处理第二章，按照检查结果修订它，补全摘要、关键事实和连续性更新";

    assert!(Reasoner::<CaptureProvider>::query_requests_artifact_mutation(query));
}

#[test]
fn artifact_written_effect_detects_json_tool_receipt() {
    let result =
        r#"{"success":true,"runtime_effect":"artifact.written","artifact_path":"/tmp/chapter.md"}"#;

    assert!(Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(result));
}

#[test]
fn artifact_written_effect_detects_chinese_saved_file_receipt() {
    let result = "已保存写作/文件产物检查点。 - 章节：第 1 章：Chapter 1 - 字数/单位：本次 375 / 累计 375 - 文件：/home/user/benshu/data/generated/novels/烬余之刻/chapters/0001_chapter-1.md - 项目：/home/user/benshu/data/generated/novels/烬余之刻 - 审查：通过";

    assert!(Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(result));
    assert!(
        Reasoner::<CaptureProvider>::tool_result_satisfies_artifact_request("请写第一章", result)
    );
}

#[test]
fn governed_exported_project_satisfies_saved_file_artifact() {
    let query = "请先完成故事设定，并写第一章，正文保存成文件";
    let result = "status: completed\nworker: writer\nexecuted_tool: novel_studio\nworkflow_driver: writing.longform_fiction\nproject_path: /home/user/benshu/data/generated/novels/万劫归墟录-3\nruntime_effect: artifact.exported\nchapters_completed: 1\nchapters_planned: 1\nstate: {\"approved_chapters\":1}";

    assert!(Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(result));
    assert!(Reasoner::<CaptureProvider>::tool_result_satisfies_artifact_request(query, result));
}

#[test]
fn requested_turn_txt_checkpoint_satisfies_one_chapter_export_request() {
    let query = "先只写第一章，每章不少于120字，每轮最多一章，完成后导出 Windows 可读的 txt。";
    let result = "status: completed\nworker: writer\nexecuted_tool: novel_studio\nworkflow_driver: writing.longform_fiction\nproject_path: /home/user/benshu/data/generated/novels/旧王朝钥匙书\nexport_path: /home/user/benshu/data/generated/novels/旧王朝钥匙书/exports/current.txt\noutput_path: /home/user/benshu/data/generated/novels/旧王朝钥匙书/exports/current.txt\nformat: txt\nmedia_type: text/plain\nruntime_effect: artifact.written\ncompletion_scope: requested_turn\nproject_complete: false\nturn_complete: true\nunit_count: 630\ntotal_units: 630\nchapters_completed: 1\nchapters_planned: 1\nstate: {\"approved_chapters\":1,\"approved_units\":630}";

    assert!(Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(result));
    assert!(Reasoner::<CaptureProvider>::tool_result_satisfies_artifact_request(query, result));
}

#[test]
fn artifact_written_effect_ignores_saved_process_report_path() {
    let result = "已保存状态报告。 - 文件：/home/user/benshu/data/generated/tasks/abc/status_report.md - 下一步：继续执行";

    assert!(!Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(result));
}

#[test]
fn artifact_written_effect_ignores_read_only_next_action_advice() {
    let result = r#"{
  "success": true,
  "read_only": true,
  "next_actions": [
    {
      "action": "revise_chapter",
      "requires": ["project_path", "chapter_number", "content"],
      "runtime_effect": "artifact.written"
    }
  ],
  "next_step_hint": "Reading state alone is not completion."
}"#;

    assert!(!Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(result));
}

#[test]
fn governed_artifact_checkpoint_detects_incomplete_writing_project_state() {
    let result = r#"请求已执行完成。工具 `novel_studio` 的结果如下：{
  "success": true,
  "runtime_effect": "artifact.checkpointed",
  "completion_scope": "checkpoint",
  "stage": "contract",
  "next_action": "run_next_chapter",
  "project_path": "/tmp/novel/project",
  "state": {
    "target_units": 500000,
    "approved_units": 0,
    "chapters": 0,
    "exports": 0
  },
  "writing_policy": {
    "workflow": ["source_intake", "contract", "planner", "writer", "export"]
  }
}"#;

    assert!(Reasoner::<CaptureProvider>::tool_result_has_governed_artifact_checkpoint(result));
    assert!(!Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(result));
}

#[test]
fn read_only_delegate_result_does_not_claim_artifact_completion() {
    let messages = vec![
        Message::user("请继续完成第三章并保存进项目。".to_string()),
        Message::tool_result(
            "call_delegate",
            r#"请求已执行完成。工具 `novel_studio` 的结果如下：{
  "success": true,
  "read_only": true,
  "next_actions": [
    {
      "action": "revise_chapter",
      "requires": ["project_path", "chapter_number", "content"],
      "runtime_effect": "artifact.written"
    }
  ],
  "next_step_hint": "Reading the truth ledger alone is not completion."
}"#,
        )
        .with_tool_name("delegate"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("还没有产生可验证的本地产物写入回执"));
    assert!(!synthesized.contains("已完成委派执行"));
    assert!(!synthesized.contains("请求已执行完成"));
}

#[test]
fn observation_recovery_failure_uses_lookup_evidence_not_unrelated_domain_template() {
    let messages = vec![
        Message::user("搜索一个科幻星际类型小说，尝试入知识库".to_string()),
        Message::tool_result(
            "call_fetch",
            r#"{"url":"https://example.com/sci-fi","title":"Interstellar fiction list","content":"candidate source"}"#,
        )
        .with_tool_name("web_fetch"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages);
    assert!(synthesized.is_none());

    let recovery = Reasoner::<CaptureProvider>::synthesize_incomplete_tool_delivery_for_recovery(
        &messages,
        "搜索一个科幻星际类型小说，尝试入知识库",
    )
    .expect("recovery message");
    assert!(recovery.contains("status: incomplete"));
    assert!(recovery.contains("executed_tool: web_fetch"));
    assert!(recovery.contains("source_url: https://example.com/sci-fi"));
    assert!(recovery.contains("没有产生知识库写入回执"));
    assert!(!recovery.contains("价格"));
    assert!(!recovery.contains("行情"));

    let delegated = vec![
        Message::user("搜索一个科幻星际类型小说，尝试入知识库".to_string()),
        Message::tool_result("call_delegate", recovery).with_tool_name("delegate"),
    ];
    assert!(
        Reasoner::<CaptureProvider>::latest_lookup_result_for_followup_execution(&delegated)
            .is_some(),
        "incomplete observation recovery must still preserve enough evidence for knowledge follow-up"
    );
}

#[test]
fn governed_fiction_tool_surface_filters_generic_writing_studio() {
    fn def(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("{name} tool"),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    let mut tools = vec![
        def("writing_studio"),
        def("novel_studio"),
        def("write_file"),
    ];

    let removed = Reasoner::<CaptureProvider>::apply_task_specific_tool_surface_filter(
        &mut tools,
        Some("搜索一个科幻星际类型小说，尝试入知识库，根据这个的基础来写小说 50万字"),
    );
    let names = tools.into_iter().map(|tool| tool.name).collect::<Vec<_>>();

    assert_eq!(removed, 2);
    assert!(names.contains(&"novel_studio".to_string()));
    assert!(!names.contains(&"writing_studio".to_string()));
    assert!(!names.contains(&"write_file".to_string()));
}

#[tokio::test]
async fn imported_material_recovery_keeps_fetch_document_available_for_writing() {
    let provider = Arc::new(CaptureProvider::new());
    let tools = ToolSet::new();
    tools.add(StaticTool {
        name: "novel_studio",
    });
    tools.add(StaticTool {
        name: "writing_studio",
    });
    tools.add(StaticTool {
        name: "fetch_document",
    });
    tools.add(StaticTool {
        name: "tiered_search",
    });
    tools.add(StaticTool { name: "write_file" });

    let mut config = ReasonerConfig::default();
    config.model = "capture-model".to_string();

    let reasoner = Reasoner::new(
        provider,
        config,
        tools,
        None,
        Arc::new(GlobalTacticalOrchestrator::passthrough()),
    );

    let available = reasoner.available_execution_tools_for_query(
        "根据知识库素材写一部50万字小说。\n\nKnowledge import receipt:\ncollection: references\npath: web/example/source",
    );

    assert!(available.contains(&"fetch_document".to_string()));
    assert!(available.contains(&"tiered_search".to_string()));
    assert!(available.contains(&"novel_studio".to_string()));
    assert!(!available.contains(&"writing_studio".to_string()));
    assert!(!available.contains(&"write_file".to_string()));
    assert_eq!(
        available.first().map(String::as_str),
        Some("fetch_document")
    );
}

#[test]
fn artifact_write_receipt_satisfies_execution_recovery() {
    let query = "请继续处理第二章，修订它并补全摘要、关键事实和连续性更新";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result(
            "call_delegate",
            r#"请求已执行完成。工具 `novel_studio` 的结果如下：{
  "success": true,
  "runtime_effect": "artifact.written",
  "artifact_path": "/tmp/novel/chapters/0002.md",
  "audit": {"passed": true}
}"#,
        )
        .with_tool_name("delegate"),
    ];

    assert!(
        Reasoner::<CaptureProvider>::latest_successful_result_satisfies_execution_request(
            &messages, query
        )
    );
}

#[test]
fn requested_txt_artifact_is_not_satisfied_by_markdown_chapter_receipt() {
    let query = "请写一部完整小说并保存成 txt 文档";
    let markdown_chapter = r#"{
  "success": true,
  "runtime_effect": "artifact.written",
  "artifact_path": "/tmp/novel/chapters/0001.md",
  "chapter": {"number": 1}
}"#;
    let txt_export = r#"{
  "success": true,
  "runtime_effect": "artifact.written",
  "runtime_effects": ["artifact.written", "artifact.exported", "artifact.txt"],
  "artifact_path": "/tmp/novel/exports/final.txt",
  "output_path": "/tmp/novel/exports/final.txt",
  "format": "txt"
}"#;

    assert!(
        !Reasoner::<CaptureProvider>::tool_result_satisfies_artifact_request(
            query,
            markdown_chapter
        )
    );
    assert!(Reasoner::<CaptureProvider>::tool_result_satisfies_artifact_request(query, txt_export));
}

#[test]
fn requested_txt_artifact_is_satisfied_by_continuous_completion_receipt() {
    let query = "请写一部完整小说并保存成 txt 文档";
    let continuous_result = "status: completed\nworker: writer\nexecuted_tool: write_file\ncontinuous_task_id: 7c1c5fd9-ea42-45f5-8d52-916fd0f6caa0\ncontinuous_task_status: Completed\npath: /tmp/novel/exports/final.txt\nruntime_effect: artifact.written\nruntime_effect: artifact.txt\nmedia_type: text/plain\nsteps_completed: 12\nsteps_planned: 12\nresult:\nCheckpointed 12 steps and wrote 43268 bytes to /tmp/novel/exports/final.txt";

    assert!(
        Reasoner::<CaptureProvider>::tool_result_satisfies_artifact_request(
            query,
            continuous_result
        )
    );
}

#[test]
fn blocked_continuous_artifact_receipt_does_not_satisfy_request() {
    let query = "请写一部完整小说并保存成 txt 文档";
    let blocked_result = "status: blocked\nworker: writer\nexecuted_tool: write_file\ncontinuous_task_id: 7c1c5fd9-ea42-45f5-8d52-916fd0f6caa0\ncontinuous_task_status: Failed { reason: \"longform artifact step 2 drifted primary subject\" }\npath: /tmp/novel/exports/final.txt\nruntime_effect: artifact.written\nruntime_effect: artifact.txt\nmedia_type: text/plain\nsteps_completed: 1\nsteps_planned: 200\nresult:\npartial checkpoint exists but final artifact is incomplete";

    assert!(
        !Reasoner::<CaptureProvider>::tool_result_satisfies_artifact_request(query, blocked_result)
    );
}

#[test]
fn artifact_verified_receipt_satisfies_verification_recovery() {
    let query = "请检查第三章是否已经保存进项目，确保当前章节存在";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result(
            "call_novel_studio",
            r#"{
  "success": true,
  "runtime_effect": "artifact.verified",
  "artifact_path": "/tmp/novel/chapters/0003.md",
  "read_only": true
}"#,
        )
        .with_tool_name("novel_studio"),
    ];

    assert!(
        Reasoner::<CaptureProvider>::latest_successful_result_satisfies_execution_request(
            &messages, query
        )
    );
}

#[test]
fn json_runtime_effect_receipt_is_durable_effect() {
    let messages = vec![
        Message::user("检查第三章是否已经保存进项目".to_string()),
        Message::tool_result(
            "call_novel_studio",
            r#"{"success":true,"runtime_effect":"artifact.verified","artifact_path":"/tmp/novel/chapters/0003.md"}"#,
        )
        .with_tool_name("novel_studio"),
    ];

    let durable =
        Reasoner::<CaptureProvider>::latest_successful_durable_effect_tool_result(&messages);

    assert!(durable.is_some());
}

#[test]
fn satisfied_durable_artifact_suppresses_pseudo_tool_recovery() {
    let messages = vec![
        Message::user("写第1章并保存".to_string()),
        Message::tool_result(
            "call_novel_studio",
            r#"{"success":true,"runtime_effect":"artifact.written","artifact_path":"/tmp/novel/chapters/0001.md","bytes":4096}"#,
        )
        .with_tool_name("novel_studio"),
    ];

    assert!(
        Reasoner::<CaptureProvider>::should_finalize_instead_of_recovering_pseudo_tool(
            &messages,
            r#"<|tool_call>{"name":"novel_studio","arguments":{"action":"revise_draft","chapter_number":1}}</tool_call>"#
        )
    );
}

#[test]
fn partial_scaled_artifact_still_allows_pseudo_tool_recovery() {
    let messages = vec![
        Message::user("写50万字小说并保存成txt".to_string()),
        Message::tool_result(
            "call_novel_studio",
            r#"{"success":true,"runtime_effect":"artifact.written","artifact_path":"/tmp/novel/chapters/0001.md","chars":4096}"#,
        )
        .with_tool_name("novel_studio"),
    ];

    assert!(
        !Reasoner::<CaptureProvider>::should_finalize_instead_of_recovering_pseudo_tool(
            &messages,
            r#"<|tool_call>{"name":"novel_studio","arguments":{"action":"run_next_chapter","chapter_number":2}}</tool_call>"#
        )
    );
}

#[test]
fn process_report_write_is_not_durable_effect() {
    let messages = vec![
        Message::user("写一部长篇小说并保存成 txt".to_string()),
        Message::tool_result(
            "call_write",
            "runtime_effect: artifact.written\npath: /workspace/tasks/status_report.txt\nbytes: 297\n\nSuccessfully wrote 297 bytes\nblockers: need to fetch source content",
        )
        .with_tool_name("write_file"),
    ];

    let durable =
        Reasoner::<CaptureProvider>::latest_successful_durable_effect_tool_result(&messages);

    assert!(durable.is_none());
    assert!(
        !Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(&messages[1].text())
    );
}

#[test]
fn bare_process_report_write_is_not_durable_effect() {
    let messages = vec![
        Message::user("写一部长篇小说并保存成 txt".to_string()),
        Message::tool_result(
            "call_write",
            "runtime_effect: artifact.written\npath: status_report.txt\nbytes: 297\n\nSuccessfully wrote 297 bytes\nblockers: need to fetch source content",
        )
        .with_tool_name("write_file"),
    ];

    let durable =
        Reasoner::<CaptureProvider>::latest_successful_durable_effect_tool_result(&messages);

    assert!(durable.is_none());
    assert!(
        !Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(&messages[1].text())
    );
}

#[test]
fn recovery_notes_write_is_not_durable_effect() {
    let messages = vec![
        Message::user("写一部长篇小说并保存成 txt".to_string()),
        Message::tool_result(
            "call_write",
            "runtime_effect: artifact.written\npath: data/recovery_notes.txt\nbytes: 141\n\nSuccessfully wrote 141 bytes",
        )
        .with_tool_name("write_file"),
    ];

    let durable =
        Reasoner::<CaptureProvider>::latest_successful_durable_effect_tool_result(&messages);

    assert!(durable.is_none());
    assert!(
        !Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(&messages[1].text())
    );
}

#[test]
fn blocker_error_file_write_is_not_durable_effect() {
    let messages = vec![
        Message::user("写一部长篇小说并保存成 txt".to_string()),
        Message::tool_result(
            "call_write",
            "runtime_effect: artifact.written\npath: data/generated/tasks/abc/error_report.txt\nbytes: 227\n\nSuccessfully wrote 227 bytes",
        )
        .with_tool_name("write_file"),
    ];

    let durable =
        Reasoner::<CaptureProvider>::latest_successful_durable_effect_tool_result(&messages);

    assert!(durable.is_none());
    assert!(
        !Reasoner::<CaptureProvider>::tool_result_has_artifact_written_effect(&messages[1].text())
    );
}

#[test]
fn model_channel_markers_are_stripped_from_final_text() {
    let text = "runtime_effect_receipt_tool: novel_studio\n\n<|channel>thought\n<channel|>status: completed";

    let cleaned = Reasoner::<CaptureProvider>::strip_model_channel_markers(text);

    assert!(!cleaned.contains("<|channel>"));
    assert!(!cleaned.contains("<channel|>"));
    assert!(cleaned.contains("status: completed"));
}

#[test]
fn internal_channel_only_model_output_is_not_deliverable() {
    let text = "<|channel>thought\n<channel|>";

    assert!(Reasoner::<CaptureProvider>::model_output_is_empty_or_non_deliverable(text));
}

#[test]
fn final_channel_model_output_is_deliverable() {
    let text = "<|channel>final\n<channel|>status: blocked\nblockers: runtime unavailable";

    assert!(!Reasoner::<CaptureProvider>::model_output_is_empty_or_non_deliverable(text));
}

#[test]
fn reflexion_missing_response_critique_is_treated_as_empty_output() {
    assert!(
        Reasoner::<CaptureProvider>::reflexion_critique_reports_missing_response(
            "<|channel>thought\n<channel|> The response is empty. It contains no content to critique."
        )
    );
    assert!(
        Reasoner::<CaptureProvider>::reflexion_critique_reports_missing_response(
            "The response itself is missing, so there is no last response to review."
        )
    );
}

#[test]
fn reflexion_substantive_critique_is_not_empty_output() {
    assert!(
        !Reasoner::<CaptureProvider>::reflexion_critique_reports_missing_response(
            "Missing source evidence for the second claim."
        )
    );
}

#[test]
fn artifact_task_context_only_tools_trigger_worker_escalation() {
    let query = "继续完成第三章，检查项目状态，确保保存进项目并更新连续性";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result("call_shared_board", "Shared board is empty.")
            .with_tool_name("shared_board"),
        Message::tool_result("call_search_history", "Search matches from memory.")
            .with_tool_name("search_history"),
    ];

    assert!(
        Reasoner::<CaptureProvider>::recent_context_only_artifact_progress_stalled(
            &messages, query
        )
    );
}

#[test]
fn read_only_artifact_followup_does_not_trigger_mutation_escalation() {
    let query = "请总结一下刚才生成的第一章内容，并告诉我主角是谁、保存的 txt 路径在哪里。";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result("call_search_history", "No relevant history found.")
            .with_tool_name("search_history"),
        Message::tool_result("call_search_history", "No relevant history found.")
            .with_tool_name("search_history"),
    ];

    assert!(!Reasoner::<CaptureProvider>::query_requests_artifact_mutation(query));
    assert!(
        !Reasoner::<CaptureProvider>::recent_context_only_artifact_progress_stalled(
            &messages, query
        )
    );
}

#[test]
fn artifact_task_context_only_escalation_stops_after_write_receipt() {
    let query = "继续完成第三章，检查项目状态，确保保存进项目并更新连续性";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result("call_shared_board", "Shared board is empty.")
            .with_tool_name("shared_board"),
        Message::tool_result(
            "call_delegate",
            r#"status: completed
worker: writer
runtime_effect: artifact.written
path: /tmp/novel/chapter-3.md"#,
        )
        .with_tool_name("delegate"),
    ];

    assert!(
        !Reasoner::<CaptureProvider>::recent_context_only_artifact_progress_stalled(
            &messages, query
        )
    );
}

#[test]
fn post_import_delivery_accepts_browser_lookup_evidence() {
    let query =
        "搜索起点中文网前十免费的玄幻小说，把公开元数据存到知识库，然后原创写一部新的玄幻小说。";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result(
            "call_browser",
            r#"status: completed
worker: browser
executed_tool: browser_browse
source_url: https://www.qidian.com/free/chanId21/
result_summary:
- 1. 夜无疆 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/book/1/
- 2. 青山 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/book/2/
- 3. 星河 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/book/3/
- 4. 长夜 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/book/4/
- 5. 云台 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/book/5/
- 6. 山河 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/book/6/
- 7. 问剑 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/book/7/
- 8. 归尘 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/book/8/
- 9. 天序 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/book/9/
- 10. 碎星 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/book/10/"#,
        )
        .with_tool_name("delegate"),
        Message::tool_result(
            "call_knowledge",
            r#"status: completed
worker: knowledge
executed_tool: knowledge_import_url
result:
Imported web knowledge into collection 'references' at path 'web/www-qidian-com/document-a89a86c8f6260d74'. Source URL: https://www.qidian.com/free/chanId21/"#,
        )
        .with_tool_name("delegate"),
    ];

    let synthesized =
        Reasoner::<CaptureProvider>::synthesize_post_import_delivery(query, &messages)
            .expect("browser-backed creative post-import delivery");

    assert!(synthesized.contains("已完成搜索和知识库写入"));
    assert!(synthesized.contains("夜无疆"));
}

#[test]
fn synthesize_successful_tool_delivery_defers_researcher_result_until_knowledge_import() {
    let query = "请搜索起点中文网当前可公开访问的免费玄幻小说，找出前10部，把公开元数据放进知识库，然后原创写一部新的玄幻小说。";
    let messages = vec![
            Message::user(query.to_string()),
            Message::tool_result(
                "call_qidian",
                r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://www.qidian.com/rank/chn21/
result_summary:
- 1. 星河之主 | public metadata: 玄幻·烽仙 | source: https://www.qidian.com/rank/chn21/
fetched_result: {"backend":"browser_snapshot_fallback_low_quality_static","url":"https://www.qidian.com/rank/chn21/"}"#,
            )
            .with_tool_name("delegate"),
        ];

    assert!(Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages).is_none());
}

#[test]
fn post_import_delivery_completes_creative_synthesis_after_knowledge_import() {
    let query = "请搜索起点中文网当前可公开访问的免费玄幻小说，找出前10部，把公开元数据放进知识库，然后原创写一部新的玄幻小说。";
    let messages = vec![
        Message::user(query.to_string()),
        Message::tool_result(
            "call_qidian",
            r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://www.qidian.com/rank/recom/chn21/
result_summary:
- 1. 夜无疆 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/rank/recom/chn21/
- 2. 青山 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/rank/recom/chn21/
- 3. 诡秘之主 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/rank/recom/chn21/
- 4. 万古 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/rank/recom/chn21/
- 5. 天门 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/rank/recom/chn21/
- 6. 星火 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/rank/recom/chn21/
- 7. 山海 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/rank/recom/chn21/
- 8. 归墟 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/rank/recom/chn21/
- 9. 烬天 | public metadata: 玄幻·异世大陆 | source: https://www.qidian.com/rank/recom/chn21/
- 10. 苍玄 | public metadata: 玄幻·东方玄幻 | source: https://www.qidian.com/rank/recom/chn21/"#,
        )
        .with_tool_name("delegate"),
        Message::tool_result(
            "call_knowledge",
            r#"status: completed
worker: knowledge
executed_tool: knowledge_import_url
source_url: https://www.qidian.com/rank/recom/chn21/
result:
{
  "ok": true,
  "collection": "references",
  "path": "web/www-qidian-com/document-a89a86c8f6260d74",
  "source_url": "https://www.qidian.com/rank/recom/chn21/"
}"#,
        )
        .with_tool_name("delegate"),
    ];

    let synthesized =
        Reasoner::<CaptureProvider>::synthesize_post_import_delivery(query, &messages)
            .expect("creative post-import delivery");

    assert!(synthesized.contains("已完成搜索和知识库写入"));
    assert!(synthesized.contains("writer worker"));
    assert!(synthesized.contains("夜无疆"));
    assert!(!synthesized.contains("只完成了知识库写入"));
}

#[test]
fn knowledge_persistence_query_survives_worker_task_user_message() {
    let original_query = "请搜索起点中文网当前可公开访问的免费玄幻小说，把公开元数据放进知识库。";
    let messages = vec![
            Message::user(original_query.to_string()),
            Message::user(
                "Search for the top 10 recommended/ranked free fantasy novels currently available on Qidian."
                    .to_string(),
            ),
            Message::tool_result(
                "call_qidian",
                r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://www.qidian.com/rank/chn21/
result_summary:
- 1. 星河之主 | public metadata: 玄幻·烽仙 | source: https://www.qidian.com/rank/chn21/"#,
            )
            .with_tool_name("delegate"),
        ];

    assert_eq!(
        Reasoner::<CaptureProvider>::latest_knowledge_persistence_query(&messages).as_deref(),
        Some(original_query)
    );
    assert!(Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages).is_none());
}

#[test]
fn synthesize_successful_tool_delivery_summarizes_delegate_search_without_raw_json() {
    let messages = vec![
        Message::user("请搜索柳叶刀最新治疗心脏病的论文，给我候选链接。".to_string()),
        Message::tool_result(
            "call_5",
            r#"status: completed
worker: researcher
executed_tool: web_search
result:
{
  "kind":"web_search",
  "results":[
    {
      "title":"The Lancet | Heart disease treatment trial",
      "url":"https://www.thelancet.com/example-paper",
      "snippet":"The Lancet reports heart disease treatment findings."
    }
  ]
}"#,
        )
        .with_tool_name("delegate"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("https://www.thelancet.com/example-paper"));
    assert!(!synthesized.contains("\"kind\""));
    assert!(!synthesized.contains("工具 `delegate` 的结果如下"));
}

#[test]
fn synthesize_delegate_search_summarizes_evidence_bundle_candidates() {
    let messages = vec![
        Message::user("请搜索 browser-use agent browser，给我2个候选链接。".to_string()),
        Message::tool_result(
            "call_evidence",
            r#"status: completed
worker: researcher
executed_tool: web_search
result:
{
  "kind":"web_search",
  "results":[],
  "evidence_bundle":{
    "candidates":[
	      {
	        "title":"browser-use/browser-use",
	        "url":"https://github.com/browser-use/browser-use",
	        "snippet":"Make websites accessible for AI agents."
	      },
	      {
	        "title":"browser-use/agent-browser",
	        "url":"https://github.com/browser-use/agent-browser",
	        "snippet":"Browser automation resources for agents."
	      }
	    ]
	  }
}"#,
        )
        .with_tool_name("delegate"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("browser-use/browser-use"));
    assert!(synthesized.contains("https://github.com/browser-use/browser-use"));
    assert!(synthesized.contains("browser-use/agent-browser"));
    assert!(synthesized.contains("https://github.com/browser-use/agent-browser"));
    assert!(!synthesized.contains("\"evidence_bundle\""));
}

#[test]
fn synthesize_delegate_search_summarizes_compacted_json_like_result() {
    let messages = vec![
            Message::user("请搜索 browser-use agent browser 相关资料。".to_string()),
            Message::tool_result(
                "call_compacted_evidence",
                r#"status: completed
worker: researcher
executed_tool: web_search
result:
{
  "diagnostics": [],
  "evidence_bundle": {
    "candidates": [
      {
        "title": "browser-use/browser-use",
        "url": "https://github.com/browser-use/browser-use",
        "snippet": "Make websites accessible for AI agents [... trimmed repeated specialist result ...]"
      }
    ]
  }
}"#,
            )
            .with_tool_name("delegate"),
        ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("browser-use/browser-use"));
    assert!(synthesized.contains("https://github.com/browser-use/browser-use"));
    assert!(!synthesized.contains("已完成委派执行"));
    assert!(!synthesized.contains("diagnostics"));
}

#[test]
fn synthesize_delegate_web_fetch_prefers_fetched_content_over_url_fields() {
    let messages = vec![
        Message::user("帮我查一下比特币现在的价格，用中文回答并给出来源。".to_string()),
        Message::tool_result(
            "call_fetch",
            r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://www.coindesk.com/price/bitcoin
search_query: current Bitcoin price
fetched_result:
{
  "backend": "http_fetch",
  "content": "Bitcoin price today\nSearch\n# Bitcoin\n$79,285.27\n2.35%\nMarket Cap. #1\nResearch Reports\nFeatured",
  "url": "https://www.coindesk.com/price/bitcoin"
}"#,
        )
        .with_tool_name("delegate"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("$79,285.27"));
    assert!(synthesized.contains("https://www.coindesk.com/price/bitcoin"));
    assert!(!synthesized.contains("已完成委派执行"));
    assert!(!synthesized.contains("Research Reports。 来源"));
}

#[test]
fn tool_result_json_blob_extracts_first_balanced_object() {
    let content = r#"status: completed
result:
{"results":[{"title":"A","url":"https://example.com","snippet":"ok"}]}

---
### NOTICE: First use of skill 'delegate'."#;
    let blob = Reasoner::<CaptureProvider>::tool_result_json_blob(content).expect("json blob");
    assert_eq!(
        blob,
        r#"{"results":[{"title":"A","url":"https://example.com","snippet":"ok"}]}"#
    );
}

#[test]
fn synthesize_successful_tool_delivery_summarizes_blocked_delegate_search_cleanly() {
    let messages = vec![
        Message::user("请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。".to_string()),
        Message::tool_result(
            "call_blocked",
            r#"status: blocked
worker: researcher
blockers: external search was blocked by an anti-bot or challenge page
query: site:thelancet.com lancet "heart disease" treatment 2025 2026"#,
        )
        .with_tool_name("delegate"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("反爬虫"));
    assert!(synthesized.contains("知识库"));
    assert!(!synthesized.contains("site:thelancet.com"));
}

#[test]
fn blocked_plain_search_delivery_does_not_mention_knowledge_base() {
    let messages = vec![
            Message::user(
                "请检查 YouTube 上 agent browser 相关视频来源，不要伪造内容。".to_string(),
            ),
            Message::tool_result(
                "call_blocked",
                r#"status: blocked
worker: researcher
lookup_strategy: structured_source_first
blockers: source returned low-information content (quality=boilerplate_only): https://www.youtube.com/results?search_query=agent+browser"#,
            )
            .with_tool_name("delegate"),
        ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("不能作为可靠搜索结果交付"));
    assert!(!synthesized.contains("知识库"));
    assert!(!synthesized.contains("写入"));
    assert!(!synthesized.contains("入库"));
}

#[test]
fn synthesize_successful_tool_delivery_blocks_delegate_pseudo_tool_leak() {
    let messages = vec![
        Message::user("请让 researcher 查一下最新 BlockBeats 快讯。".to_string()),
        Message::tool_result(
            "call_delegate_pseudo",
            r#"status: completed
worker: researcher
result:
<|tool_call>call:web_search{query: "BlockBeats 最新快讯"}<tool_call|>"#,
        )
        .with_tool_name("delegate"),
    ];

    let synthesized = Reasoner::<CaptureProvider>::synthesize_successful_tool_delivery(&messages)
        .expect("synthesized result");

    assert!(synthesized.contains("未执行的工具调用标签"));
    assert!(synthesized.contains("还没有完成"));
    assert!(!synthesized.contains("已完成委派执行"));
    assert!(!synthesized.contains("请求已执行完成"));
}

#[test]
fn summarize_delegate_delivery_includes_knowledge_import_receipt() {
    let content = "status: completed\nworker: knowledge\nexecuted_tool: knowledge_import_url\nresult:\nImported web knowledge into collection 'references' at path 'web/thelancet-com/paper-1234' with title 'Lancet paper'. Source URL: https://www.thelancet.com/example-paper";

    let summary = Reasoner::<CaptureProvider>::summarize_delegate_delivery(
        "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。",
        content,
        true,
    );

    assert!(summary.contains("完成知识库写入"));
    assert!(summary.contains("references"));
    assert!(summary.contains("web/thelancet-com/paper-1234"));
    assert!(summary.contains("https://www.thelancet.com/example-paper"));
}

#[test]
fn summarize_delegate_delivery_uses_artifact_checkpoint_without_body() {
    let long_body = "正文内容不要进入聊天历史。".repeat(400);
    let content = serde_json::json!({
        "success": true,
        "runtime_effect": "artifact.written",
        "artifact_path": "/tmp/benshu/novels/project/chapters/0001.md",
        "project_path": "/tmp/benshu/novels/project",
        "unit_count": 3200,
        "total_units": 3200,
        "target_units": 500000,
        "chapter": {
            "number": 1,
            "title": "风起云门",
            "summary": "主角在边城发现灵潮异常，并决定离开旧门派调查。"
        },
        "quality_gate": {"passed": true},
        "content": long_body
    })
    .to_string();

    let summary = Reasoner::<CaptureProvider>::summarize_delegate_delivery(
        "写一部50万字玄幻小说并保存成文件。",
        &format!(
            "status: completed\nworker: writer\nexecuted_tool: novel_studio\nresult:\n{content}"
        ),
        true,
    );

    assert!(summary.contains("产物检查点"));
    assert!(summary.contains("第 1 章"));
    assert!(summary.contains("3200"));
    assert!(summary.contains("500000"));
    assert!(summary.contains("/tmp/benshu/novels/project/chapters/0001.md"));
    assert!(summary.contains("通过"));
    assert!(summary.contains("主角在边城发现灵潮异常"));
    assert!(!summary.contains("正文内容不要进入聊天历史"));
    assert!(!summary.contains("content"));
}

#[test]
fn summarize_delegate_delivery_stops_when_lookup_has_no_relevant_source_url() {
    let content = r#"status: completed
worker: researcher
executed_tool: web_search
result:
{
  "kind":"web_search",
  "results":[
    {
      "title":"Support article",
      "url":"https://support.google.com/search",
      "snippet":"generic help page"
    }
  ]
}"#;

    let summary = Reasoner::<CaptureProvider>::summarize_delegate_delivery(
        "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。",
        content,
        true,
    );

    assert!(summary.contains("初步检索"));
    assert!(summary.contains("不能安全写入知识库"));
}

#[test]
fn summarize_delegate_delivery_does_not_present_incomplete_lookup_as_answer() {
    let content = r#"status: incomplete
executed_tool: web_search
source_url: https://en.wikipedia.org/?curid=159894

I ran the lookup/fetch, but this step did not produce a local artifact write receipt yet, so it cannot be treated as completed.

Current evidence: [
  {
    "title": "Shipping Forecast",
    "url": "https://en.wikipedia.org/?curid=159894",
    "snippet": "The Shipping Forecast is a BBC Radio broadcast of weather reports and forecasts for the seas around the British Isles."
  }
]"#;

    let summary = Reasoner::<CaptureProvider>::summarize_delegate_delivery(
        "搜索一下今天北京天气怎样",
        content,
        true,
    );

    assert!(summary.contains("不完整") || summary.contains("未验证"));
    assert!(summary.contains("不能把它当作可靠答案"));
    assert!(!summary.contains("Shipping Forecast"));
    assert!(!summary.contains("当前最相关结果"));
}

#[test]
fn summarize_delegate_delivery_blocks_cloudflare_fetch_before_knowledge_import() {
    let content = r#"status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://www.thelancet.com/journals/lancet/onlinefirst
search_result:
{"kind":"web_search"}

fetched_result:
{
  "backend":"browser_snapshot_fallback_blocked",
  "url":"https://www.thelancet.com/journals/lancet/onlinefirst",
  "content":"请稍候… www.thelancet.com 正在进行安全验证 Enable JavaScript and cookies to continue Ray ID: 1234567890"
}"#;

    let summary = Reasoner::<CaptureProvider>::summarize_delegate_delivery(
        "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。",
        content,
        true,
    );

    assert!(summary.contains("安全验证") || summary.contains("反爬"));
    assert!(summary.contains("不能安全写入知识库"));
    assert!(summary.contains("thelancet.com"));
}

#[test]
fn tool_search_result_indicates_external_lookup_for_latest_info_routes() {
    let content = r#"{
  "matches": [
    {"name":"web_search"},
    {"name":"web_fetch"},
    {"name":"browser_browse"}
  ]
}"#;

    assert!(Reasoner::<CaptureProvider>::tool_search_result_indicates_external_lookup(content));
}

#[test]
fn tool_result_is_blocked_detects_structured_blocker_status() {
    assert!(Reasoner::<CaptureProvider>::tool_result_is_blocked(
        "status: blocked\nworker: researcher\nblockers: external search was blocked"
    ));
    assert!(!Reasoner::<CaptureProvider>::tool_result_is_blocked(
        "status: completed\nworker: researcher\nexecuted_tool: web_search"
    ));
}

#[test]
fn extract_latest_parsed_attachment_summary_returns_injected_summary() {
    let messages = vec![Message::user(Content::Parts(vec![
            ContentPart::Text {
                text: "请描述这张图片".to_string(),
            },
            ContentPart::Text {
                text: "\n[Parsed image Attachment via local_sensory_vlm]\nsource: file:///tmp/demo.png\n一只橙色猫坐在窗台上。\nparser_mode: visual".to_string(),
            },
            ContentPart::Image {
                source: ImageSource::Url {
                    url: "file:///tmp/demo.png".to_string(),
                },
            },
        ]))];

    let summary = Reasoner::<CaptureProvider>::extract_latest_parsed_attachment_summary(&messages)
        .expect("parsed summary");

    assert_eq!(summary, "一只橙色猫坐在窗台上。");
}
