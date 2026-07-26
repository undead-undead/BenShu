use super::*;
use benshu_brain::agent::multi_agent::{MultiAgent, WorkerBlueprint};
use benshu_brain::agent::protocol::{AgentEvent, ChatOutcome, TaskOwnership};
use benshu_brain::error::Result as BrainResult;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

struct PseudoThenRealWorker {
    role: AgentRole,
    calls: AtomicUsize,
    events_tx: broadcast::Sender<AgentEvent>,
}

impl PseudoThenRealWorker {
    fn new(role: AgentRole) -> Self {
        let (events_tx, _) = broadcast::channel(4);
        Self {
            role,
            calls: AtomicUsize::new(0),
            events_tx,
        }
    }
}

#[test]
fn delegated_worker_runtime_failure_text_is_not_treated_as_success() {
    assert!(DelegateTool::delegated_worker_result_is_runtime_failure(
        "status: failed\nblockers: missing source evidence"
    ));
    assert!(DelegateTool::delegated_worker_result_is_runtime_failure(
        "Error executing tool 'delegate': Tool not found: delegate"
    ));
    assert!(DelegateTool::delegated_worker_result_is_runtime_failure(
        "Runtime notice: 1 planned tool call(s) did not produce a matching tool result"
    ));
    assert!(!DelegateTool::delegated_worker_result_is_runtime_failure(
        "status: completed\nworker: researcher\nexecuted_tool: web_search\nresult: usable evidence"
    ));
}

#[test]
fn delegated_worker_tool_boundary_failure_can_be_returned_as_blocker() {
    let error = "Runtime tool error in `delegate`: tool is not equipped for this agent. Available tools right now: browser_browse, web_fetch, web_search.";
    let blocker = DelegateTool::delegated_worker_runtime_failure_blocker("researcher", error)
        .expect("tool boundary failure should become a returnable blocker");

    assert!(blocker.contains("status: blocked"));
    assert!(blocker.contains("worker: researcher"));
    assert!(blocker.contains("available_tools: browser_browse, web_fetch, web_search"));
    assert!(blocker.contains("runtime_error_preview:"));
}

#[test]
fn delegated_worker_structured_tool_contract_error_returns_blocker() {
    let result = r#"请求已执行完成。工具 `novel_studio` 的结果如下：{
  "action": "write_draft",
  "error_kind": "missing_required_content",
  "example_shape": {"content": "<full text to save>"},
  "next_step_hint": "Generate the actual body text first, then call this action again."
}"#;

    assert!(DelegateTool::delegated_worker_result_is_runtime_failure(
        result
    ));
    let blocker = DelegateTool::delegated_worker_runtime_failure_blocker("writer", result)
        .expect("structured tool contract error should become a returnable blocker");

    assert!(blocker.contains("status: blocked"));
    assert!(blocker.contains("worker: writer"));
    assert!(blocker.contains("structured tool contract error"));
    assert!(blocker.contains("runtime_error_preview:"));
}

#[test]
fn delegated_worker_not_found_observation_is_not_runtime_failure() {
    let result = r#"{
  "alternative_projects": [],
  "error": "chapter 3 not found in selected project",
  "error_kind": "chapter_not_found",
  "recoverable": true,
  "success": false,
  "next_step_hint": "Continue by composing from the selected project's latest available chapter."
}"#;

    assert!(!DelegateTool::delegated_worker_result_is_runtime_failure(
        result
    ));
}

#[test]
fn artifact_checkpoint_receipt_is_detected_from_tool_end_summary() {
    let summary = r#"writer.novel_studio finished success=true duration_ms=13 preview={
  "artifact_path": "/workspace/out/chapter.md",
  "runtime_effect": "artifact.written"
}"#;

    assert!(DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
    assert!(DelegateTool::checkpoint_summary_has_artifact_progress_receipt(summary));
}

#[test]
fn artifact_checkpointed_progress_is_resumable_but_not_final_written() {
    let summary = r#"writer.novel_studio finished success=true duration_ms=13 preview={
  "artifact_path": "/workspace/out/chapter.md",
  "runtime_effect": "artifact.checkpointed",
  "completion_scope": "checkpoint",
  "total_units": 2400,
  "target_units": 500000,
  "target_reached": false
}"#;

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
    assert!(DelegateTool::checkpoint_summary_has_artifact_progress_receipt(summary));
}

#[test]
fn artifact_written_with_unmet_target_is_progress_not_final_written() {
    let summary = r#"Worker `writer` returned delegated result. Preview: 请求已执行完成。工具 `novel_studio` 的结果如下：{
  "artifact_path": "/workspace/novel/chapters/0001.md",
  "runtime_effect": "artifact.written",
  "success": true,
  "completion_scope": "checkpoint",
  "total_units": 399,
  "target_units": 500000,
  "target_reached": false,
  "audit": {
    "warnings": ["Chapter 1 is far below the chapter target: 399 of 8000 units."]
  }
}"#;

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
    assert!(DelegateTool::checkpoint_summary_has_artifact_progress_receipt(summary));
}

#[test]
fn writing_unit_targets_distinguish_chapter_scope_from_total_scope() {
    let task = "请写两章玄幻小说，每章不少于1200字，并导出txt。";

    assert_eq!(
        DelegateTool::requested_chapter_unit_target_chars(task),
        Some(2500)
    );
    assert_eq!(DelegateTool::requested_total_text_target_chars(task), None);

    let ordinal_task = "第一章内容需不少于 1200 字，请直接输出第一章正文。";
    assert_eq!(
        DelegateTool::requested_chapter_unit_target_chars(ordinal_task),
        Some(2500)
    );
    assert_eq!(
        DelegateTool::requested_total_text_target_chars(ordinal_task),
        None
    );

    let total_task = "请写一部50000字的玄幻小说，每章不少于1500字。";
    assert_eq!(
        DelegateTool::requested_total_text_target_chars(total_task),
        Some(50000)
    );
    assert_eq!(
        DelegateTool::requested_chapter_unit_target_chars(total_task),
        Some(2500)
    );

    let structured_contract = "\
用户已确认创作合同。\n\
总目标字数：500000\n\
每章目标字数：3000\n\
正文保存为 txt。";
    assert_eq!(
        DelegateTool::requested_total_text_target_chars(structured_contract),
        Some(500000)
    );
    assert_eq!(
        DelegateTool::requested_chapter_unit_target_chars(structured_contract),
        Some(2500)
    );
    assert_eq!(
        DelegateTool::requested_chapter_count_with_step_target(structured_contract, 3000),
        200
    );

    let continuation_task = "第一章完成后，书名不要三个字重复；请在同一个项目里继续写第二章。";
    assert_eq!(
        DelegateTool::requested_start_chapter(continuation_task),
        Some(2)
    );
    assert_eq!(
        DelegateTool::requested_total_text_target_chars(continuation_task),
        None
    );

    let wrapped_continuation = "\
Full user request: 接着刚才的小说继续写下一章，后面每章2500字。\n\n\
Previous worker receipt:\nstate: {\"approved_units\":4618,\"chapter_unit_target\":4000,\"target_units\":4000}";
    assert_eq!(
        DelegateTool::requested_chapter_unit_target_chars(wrapped_continuation),
        Some(2500)
    );
    assert_eq!(
        DelegateTool::requested_total_text_target_chars(wrapped_continuation),
        None
    );
}

#[test]
fn bare_artifact_path_is_not_a_final_write_receipt() {
    let summary = r#"Worker `writer` returned delegated result. Preview: {
  "artifact_path": "/workspace/novel/chapters/0001.md",
  "success": true
}"#;

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
}

#[test]
fn read_only_checkpoint_hint_is_not_artifact_written_receipt() {
    let summary = r#"writer.novel_studio finished success=true duration_ms=5 preview={
  "read_only": true,
  "next_actions": [{"runtime_effect": "artifact.written"}]
}"#;

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
    assert!(!DelegateTool::checkpoint_summary_has_artifact_progress_receipt(summary));
}

#[test]
fn needs_revision_checkpoint_is_not_artifact_written_receipt() {
    let summary = r#"writer.novel_studio finished success=true duration_ms=13 preview={
  "artifact_path": "/workspace/out/chapter.md",
  "runtime_effect": "artifact.needs_revision",
  "status": "needs_revision",
  "quality_gate": {"passed": false}
}"#;

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
}

#[test]
fn review_checkpoint_is_not_artifact_written_receipt() {
    let summary = r#"writer.novel_studio finished success=true duration_ms=7 preview={
  "artifact_path": "/workspace/novel/reviews/chapter-0001-audit-0001.md",
  "review_path": "/workspace/novel/reviews/chapter-0001-audit-0001.md",
  "runtime_effect": "artifact.written",
  "review": {"verdict": "needs_revision"}
}"#;

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
}

#[test]
fn process_report_checkpoint_is_not_artifact_written_receipt() {
    let summary = "writer.write_file finished success=true duration_ms=4 preview=runtime_effect: artifact.written path: /workspace/tasks/status_report.txt bytes: 297 Successfully wrote 297 bytes blockers: need to fetch source content";

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
}

#[test]
fn bare_process_report_checkpoint_is_not_artifact_written_receipt() {
    let summary = "writer.write_file finished success=true duration_ms=4 preview=runtime_effect: artifact.written path: status_report.txt bytes: 297 Successfully wrote 297 bytes blockers: need to fetch source content";

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
}

#[test]
fn recovery_notes_checkpoint_is_not_artifact_written_receipt() {
    let summary = "writer.write_file finished success=true duration_ms=4 preview=runtime_effect: artifact.written path: data/recovery_notes.txt bytes: 141 Successfully wrote 141 bytes";

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
}

#[test]
fn blocker_error_file_checkpoint_is_not_artifact_progress() {
    let summary = "writer.write_file finished success=true duration_ms=11 preview=runtime_effect: artifact.written path: data/generated/tasks/137e3586-e1a8-4165-aad5-62af10e59266/error_report.txt bytes: 227 Successfully wrote 227 bytes";

    assert!(!DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
    assert!(!DelegateTool::checkpoint_summary_has_artifact_progress_receipt(summary));
}

#[test]
fn delegated_result_can_carry_child_artifact_receipt() {
    let summary = r#"writer.novel_studio finished success=true duration_ms=13 preview={
  "artifact_path": "/workspace/out/chapter.md",
  "runtime_effect": "artifact.written"
}"#;

    let result = DelegateTool::delegated_result_with_artifact_receipt(
        "writer",
        "Chapter 3 was revised and saved.",
        summary,
    );

    assert!(result.contains("runtime_effect: artifact.written"));
    assert!(result.contains("\"artifact_path\": \"/workspace/out/chapter.md\""));
    assert!(result.contains("Chapter 3 was revised and saved."));
}

#[test]
fn chinese_saved_file_checkpoint_counts_as_artifact_receipt() {
    let summary = "Worker `writer` returned delegated result. Preview: 已保存写作/文件产物检查点。 - 章节：第 4 章：第4章：紫气破阵 - 字数/单位：本次 666 / 累计 2022 - 文件：/home/user/benshu/data/generated/novels/project/chapters/0004_第4章紫气破阵.md - 项目：/home/user/benshu/data/generated/novels/project - 审查：未提供";

    assert!(DelegateTool::checkpoint_summary_has_artifact_written_receipt(summary));
}

#[async_trait]
impl MultiAgent for PseudoThenRealWorker {
    fn role(&self) -> AgentRole {
        self.role.clone()
    }

    async fn handle_message(
        &self,
        _message: benshu_brain::agent::multi_agent::AgentMessage,
    ) -> BrainResult<Option<benshu_brain::agent::multi_agent::AgentMessage>> {
        Ok(None)
    }

    async fn process(&self, input: &str) -> BrainResult<String> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            Ok("<|tool_call>call:web_search{query: \"BlockBeats\"}<tool_call|>".to_string())
        } else if input.contains("REQUIRED RECOVERY STEP") {
            Ok("status: completed\nworker: researcher\nexecuted_tool: web_search\nresult: real tool result".to_string())
        } else {
            Ok("status: blocked\nblockers: recovery contract missing".to_string())
        }
    }

    async fn chat(
        &self,
        _messages: Vec<benshu_brain::agent::message::Message>,
        _session_id: Option<String>,
    ) -> BrainResult<ChatOutcome> {
        Ok(ChatOutcome {
            response: "unused".to_string(),
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(self.role.clone(), None),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        })
    }

    fn agent_identity(
        &self,
    ) -> Option<Arc<parking_lot::RwLock<Option<benshu_brain::agent::agent_identity::AgentIdentity>>>>
    {
        None
    }

    fn events(&self) -> broadcast::Receiver<AgentEvent> {
        self.events_tx.subscribe()
    }

    fn security(&self) -> Option<Arc<dyn benshu_brain::security::SecurityHandler>> {
        None
    }

    fn cancel(&self) {}

    fn ensure_active_token(&self) {}
}

#[tokio::test]
async fn delegate_definition_keeps_worker_surface_compact() {
    let coordinator = Arc::new(Coordinator::new());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("pdf".to_string()),
        agent_path: PathBuf::from("/tmp/pdf"),
        display_name: "PDF".to_string(),
        description: Some("PDF parsing specialist.".to_string()),
        tools: vec!["pdf_parse".to_string()],
        artifact_policy: None,
    });

    let tool = DelegateTool::new(Arc::downgrade(&coordinator));
    let definition = tool.definition().await;

    assert!(definition
        .description
        .contains("Registered specialist count: 1"));
    assert!(definition
        .description
        .contains("Known specialist roles right now: pdf."));
    assert!(definition.description.contains("tool_search"));
    assert!(!definition.description.contains("PDF parsing specialist"));
    assert!(!definition.description.contains("pdf_parse"));
    assert_eq!(
        definition.parameters["properties"]["role"]["enum"],
        serde_json::json!(["pdf", "auto"])
    );
}

#[test]
fn delegate_resolves_capability_aliases_to_registered_workers() {
    let coordinator = Arc::new(Coordinator::new());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("document".to_string()),
        agent_path: PathBuf::from("/tmp/document"),
        display_name: "Document".to_string(),
        description: Some("Document understanding specialist.".to_string()),
        tools: vec!["document_understand".to_string()],
        artifact_policy: None,
    });
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("knowledge".to_string()),
        agent_path: PathBuf::from("/tmp/knowledge"),
        display_name: "Knowledge".to_string(),
        description: Some("Knowledge ingestion specialist.".to_string()),
        tools: vec!["knowledge_import_url".to_string()],
        artifact_policy: None,
    });

    let document_role = DelegateTool::resolve_target_role(&coordinator, "document_understanding");
    let knowledge_role = DelegateTool::resolve_target_role(&coordinator, "knowledge_import_url");

    assert_eq!(document_role.name(), "document");
    assert_eq!(knowledge_role.name(), "knowledge");
}

#[test]
fn delegate_auto_uses_worker_policy_index_before_static_fallback() {
    let coordinator = Arc::new(Coordinator::new());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("researcher".to_string()),
        agent_path: PathBuf::from("/tmp/researcher"),
        display_name: "Researcher".to_string(),
        description: Some("General lookup specialist.".to_string()),
        tools: vec!["web_search".to_string()],
        artifact_policy: None,
    });
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("medical_researcher".to_string()),
        agent_path: PathBuf::from("/tmp/medical_researcher"),
        display_name: "Medical Researcher".to_string(),
        description: Some("Medical evidence specialist.".to_string()),
        tools: vec!["web_search".to_string(), "knowledge_import_url".to_string()],
        artifact_policy: Some(serde_json::json!({
            "handles": [{
                "artifact": "medical_evidence_packet",
                "triggers": ["医学论文", "heart disease", "治疗心脏病"],
                "intents": ["knowledge import", "evidence review"]
            }]
        })),
    });

    let role = DelegateTool::resolve_target_role_for_task(
        &coordinator,
        "auto",
        "查找最近治疗心脏病的医学论文，保存证据到知识库，然后写论文",
    );

    assert_eq!(role.name(), "medical_researcher");
}

#[test]
fn delegate_explicit_role_is_not_overridden_by_task_content() {
    let coordinator = Arc::new(Coordinator::new());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("researcher".to_string()),
        agent_path: PathBuf::from("/tmp/researcher"),
        display_name: "Researcher".to_string(),
        description: Some("Lookup specialist.".to_string()),
        tools: vec!["web_search".to_string(), "web_fetch".to_string()],
        artifact_policy: None,
    });
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("writer".to_string()),
        agent_path: PathBuf::from("/tmp/writer"),
        display_name: "Writer".to_string(),
        description: Some("Long-form writing specialist.".to_string()),
        tools: vec!["writing".to_string()],
        artifact_policy: Some(serde_json::json!({
            "handles": [{
                "artifact": "longform_fiction",
                "triggers": ["小说", "fiction", "50万字"]
            }]
        })),
    });

    let role = DelegateTool::resolve_target_role_for_task(
        &coordinator,
        "researcher",
        "搜索一个科幻星际类型小说，尝试入知识库，根据这个的基础来写小说 50万字",
    );

    assert_eq!(role.name(), "researcher");
}

#[test]
fn delegate_explicit_writer_role_is_not_overridden_by_acquisition_terms() {
    let coordinator = Arc::new(Coordinator::new());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("researcher".to_string()),
        agent_path: PathBuf::from("/tmp/researcher"),
        display_name: "Researcher".to_string(),
        description: Some("Lookup and source gathering specialist.".to_string()),
        tools: vec!["web_search".to_string(), "web_fetch".to_string()],
        artifact_policy: None,
    });
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("writer".to_string()),
        agent_path: PathBuf::from("/tmp/writer"),
        display_name: "Writer".to_string(),
        description: Some("Long-form writing specialist.".to_string()),
        tools: vec!["writing".to_string()],
        artifact_policy: None,
    });

    let role = DelegateTool::resolve_target_role_for_task(
        &coordinator,
        "writer",
        "Search and import source material into the knowledge base, then write the requested long-form artifact.",
    );

    assert_eq!(role.name(), "writer");
}

#[test]
fn writer_without_acquisition_tools_reports_phase_boundary_before_drafting() {
    let coordinator = Arc::new(Coordinator::new());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("researcher".to_string()),
        agent_path: PathBuf::from("/tmp/researcher"),
        display_name: "Researcher".to_string(),
        description: Some("Lookup and source gathering specialist.".to_string()),
        tools: vec!["web_search".to_string(), "web_fetch".to_string()],
        artifact_policy: None,
    });
    let writer = AgentRole::Custom("writer".to_string());
    let writer_tools = vec!["writing".to_string()];
    let task = "Search public web source material, import it into the knowledge base, then write a long novel and save a txt file.";

    assert!(DelegateTool::role_is_writing_owner(&writer, &writer_tools));
    assert!(!DelegateTool::worker_has_external_acquisition_tools(
        &writer_tools
    ));
    assert!(DelegateTool::task_requires_external_acquisition_before_artifact(task));
    assert!(!DelegateTool::task_has_verified_acquisition_evidence(task));
    let suggested = DelegateTool::suggested_external_acquisition_role(&coordinator, &writer);
    let blocker = DelegateTool::artifact_owner_phase_boundary_result(&writer, suggested, task);

    assert!(blocker.contains("error_kind: phase_boundary"));
    assert!(blocker.contains("suggested_role: researcher"));
}

#[test]
fn worker_contract_recovery_does_not_trigger_acquisition_phase_boundary() {
    let task = "Continue the same delegated task after a worker tool-contract recovery. The previous worker attempt reached an equipped tool but supplied incomplete arguments. If content exists only as a URL or knowledge receipt, retrieve it first. Previous contract detail: {\"error_kind\":\"missing_required_content\",\"example_shape\":{\"action\":\"write_draft\",\"content\":\"<full text to save>\"}}";

    assert!(!DelegateTool::task_requires_external_acquisition_before_artifact(task));
}

#[test]
fn frontstage_tool_recovery_uses_original_user_request_for_phase_boundary() {
    let pure_creation = "Execute this routed user task as the specialist because the frontstage model did not emit a tool call after an explicit execution-required prompt. Preserve the full original request and all downstream actions. If the task includes lookup/source discovery before another action, perform the lookup phase first. Full user request: 请从零创作一部新的玄幻小说，共10章。现在先写第1章。";
    let acquisition = "Execute this routed user task as the specialist because the frontstage model did not emit a tool call after an explicit execution-required prompt. Preserve the full original request and all downstream actions. If the task includes lookup/source discovery before another action, perform the lookup phase first. Full user request: 搜索公网素材并导入知识库，然后写一篇报告。";

    assert!(!DelegateTool::task_requires_external_acquisition_before_artifact(pure_creation));
    assert!(DelegateTool::task_requires_external_acquisition_before_artifact(acquisition));
}

#[test]
fn web_novel_creation_does_not_trigger_external_acquisition_boundary() {
    let task = "Create a detailed outline and the first chapter for a 'grassroots-to-legend' (草根逆袭) fantasy web novel. Save the chapter as a text artifact.";

    assert!(!DelegateTool::task_requires_external_acquisition_before_artifact(task));
}

#[test]
fn delegate_writer_request_with_existing_evidence_stays_writer() {
    let coordinator = Arc::new(Coordinator::new());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("researcher".to_string()),
        agent_path: PathBuf::from("/tmp/researcher"),
        display_name: "Researcher".to_string(),
        description: Some("Lookup and source gathering specialist.".to_string()),
        tools: vec!["web_search".to_string(), "web_fetch".to_string()],
        artifact_policy: None,
    });
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("writer".to_string()),
        agent_path: PathBuf::from("/tmp/writer"),
        display_name: "Writer".to_string(),
        description: Some("Long-form writing specialist.".to_string()),
        tools: vec!["writing".to_string()],
        artifact_policy: None,
    });

    let role = DelegateTool::resolve_target_role_for_task(
        &coordinator,
        "writer",
        "Use the Knowledge import receipt and verified researcher evidence already in this conversation to write the requested artifact.",
    );

    assert_eq!(role.name(), "writer");
}

#[test]
fn writer_phase_boundary_accepts_recently_imported_knowledge_material() {
    let task = "Based on the recently imported knowledge (https://example.org/material), write a new long-form artifact without copying the source.";

    assert!(DelegateTool::task_requires_external_acquisition_before_artifact(task));
    assert!(DelegateTool::task_has_verified_acquisition_evidence(task));
}

#[test]
fn delegate_wraps_worker_task_with_tool_first_contract() {
    let task = DelegateTool::build_worker_execution_contract(
        &AgentRole::Custom("researcher".to_string()),
        &["web_search".to_string(), "web_fetch".to_string()],
        "Search the latest Lancet heart disease treatment paper.",
        Some("Find a recent Lancet heart disease paper and save it to the knowledge base."),
    );

    assert!(task.contains("Delegated Specialist Contract"));
    assert!(task.contains("Constraint preservation contract"));
    assert!(task.contains("source of truth"));
    assert!(task.contains("call `web_search` once"));
    assert!(task.contains("search-result metadata is not completion evidence"));
    assert!(task.contains("Do not invent extra source-use constraints"));
    assert!(task.contains("Preserve only the constraints explicitly present"));
    assert!(task.contains("sources explicitly excluded by the original request"));
    assert!(task.contains("source_urls"));
    assert!(task.contains("Original delegated task"));
    assert!(task.contains("latest Lancet"));
}

#[test]
fn delegate_worker_contract_includes_runtime_policy_bundle() {
    let policy = serde_json::json!({
        "handles": [{
            "artifact": "research_paper",
            "triggers": ["paper"],
            "intents": ["research", "draft"],
            "evidence_hints": ["primary source"],
            "quality_contract": {
                "min_chars": 2000,
                "min_citations": 2,
                "require_title": true
            }
        }]
    });
    let task = DelegateTool::build_worker_execution_contract_with_policy(
        &AgentRole::Custom("researcher".to_string()),
        &["web_search".to_string(), "web_fetch".to_string()],
        Some(&policy),
        "Find a paper and draft a grounded summary.",
        None,
    );

    assert!(task.contains("Runtime policy bundle"));
    assert!(task.contains("phase=Delegation"));
    assert!(task.contains("research_paper"));
    assert!(task.contains("min_citations=2"));
}

#[test]
fn delegate_browser_contract_uses_real_browser_browse_tool_name() {
    let task = DelegateTool::build_worker_execution_contract(
        &AgentRole::Custom("researcher".to_string()),
        &[
            "web_search".to_string(),
            "web_fetch".to_string(),
            "browser_browse".to_string(),
        ],
        "Find observable public list evidence from a web page.",
        None,
    );

    assert!(task.contains("Browser contract"));
    assert!(task.contains("call `browser_browse` exactly as the real tool name"));
}

#[test]
fn delegate_writing_contract_keeps_written_artifacts_on_writer() {
    let task = DelegateTool::build_worker_execution_contract(
        &AgentRole::Custom("writer".to_string()),
        &["writing".to_string()],
        "写一篇短篇小说并保存为 txt。",
        Some("请创建一个短篇玄幻小说测试稿，由 writer worker 完成并保存为本地 txt 文件。"),
    );

    assert!(task.contains("Writing artifact contract"));
    assert!(task.contains("Do not hand articles, fiction, papers, essays, reports, drafts, or TXT/Markdown prose files to coder"));
    assert!(task.contains("write the content with `write_file`"));
    assert!(task.contains("use `writing_studio`"));
    assert!(task.contains("writing ledger"));
    assert!(task.contains("use `novel_studio`"));
    assert!(task.contains(
        "A requested saved file is not complete until a writing/file tool reports the path"
    ));
}

#[test]
fn delegate_fast_path_recoverable_browser_blocker_falls_back_to_worker_loop() {
    let result = "status: blocked\nworker: researcher\nexecuted_tool: browser_browse\nblockers: browser search failed: Windows CDP bridge returned no relevant parsable results; static_error=anti-bot";

    assert!(DelegateTool::fast_path_blocker_should_fall_back(
        &AgentRole::Custom("researcher".to_string()),
        "Search the public web for downloadable free fantasy novels.",
        result
    ));
}

#[test]
fn delegate_constraint_surface_keeps_original_request_for_fast_paths() {
    let task = DelegateTool::task_with_constraint_source(
        "Find a list of the top 10 popular fantasy novels.",
        Some("搜索起点玄幻小说把可以下载的免费玄幻小说下载前10部，之后放到知识库。"),
    );

    assert!(task.contains("Original user request"));
    assert!(task.contains("免费"));
    assert!(task.contains("下载"));
    assert!(task.contains("知识库"));
}

#[test]
fn delegate_constraint_surface_moves_embedded_original_request_to_front() {
    let original = "搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。";
    let task = DelegateTool::task_with_constraint_source(
        &format!(
            "Search for a popular downloadable fantasy source.\n\nOriginal user request:\n{original}"
        ),
        Some(original),
    );

    assert!(task.starts_with("Original user request:\n搜索一部公网可下载"));
    assert_eq!(task.matches(original).count(), 1);
    assert!(task.contains("\n\nDelegated task:\nSearch for a popular downloadable fantasy source."));
}

#[test]
fn delegate_worker_contract_strips_model_added_source_use_constraints() {
    let task = DelegateTool::build_worker_execution_contract(
        &AgentRole::Custom("researcher".to_string()),
        &["web_search".to_string(), "web_fetch".to_string()],
        "Find up to 10 downloadable public-web texts. If the content is too large or copyrighted, find summaries or excerpts instead.",
        Some("请在公网查找可下载的免费文本内容并保存到知识库。"),
    );

    assert!(task.contains("Find up to 10 downloadable public-web texts."));
    assert!(!task.to_ascii_lowercase().contains("copyrighted"));
    assert!(!task.to_ascii_lowercase().contains("excerpts instead"));
    assert!(task.contains("Original user request"));
}

#[test]
fn delegate_worker_contract_blocks_weaker_substitutes_for_original_constraints() {
    let task = DelegateTool::build_worker_execution_contract(
        &AgentRole::Custom("researcher".to_string()),
        &["web_search".to_string(), "web_fetch".to_string()],
        "Find summaries or metadata for source material if direct content is hard to fetch.",
        Some("搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。"),
    );

    assert!(task.contains("source of truth"));
    assert!(task.contains("weaker substitute"));
    assert!(task.contains("status: blocked"));
    assert!(task.contains("正文"));
    assert!(task.contains("知识库"));
}

#[test]
fn delegate_blocked_worker_result_is_returnable_not_runtime_failure() {
    let result =
        "status: blocked\nworker: researcher\nblockers: no concrete source URL was verified";

    assert!(!DelegateTool::delegated_worker_result_is_runtime_failure(
        result
    ));
    assert!(DelegateTool::delegated_worker_result_is_runtime_failure(
        "status: failed\nworker: researcher\nblockers: runtime error"
    ));
}

#[test]
fn delegate_detects_unexecuted_pseudo_tool_tags() {
    assert!(DelegateTool::contains_unexecuted_pseudo_tool_call(
        "<|tool_call>call:web_search{query: \"x\"}<tool_call|>"
    ));
    assert!(!DelegateTool::contains_unexecuted_pseudo_tool_call(
        "status: completed\nresult: ok"
    ));
}

#[test]
fn delegate_compacts_github_structured_fetch_results() {
    let payload = serde_json::json!({
        "url": "https://api.github.com/search/repositories?q=agent-browser&per_page=5&sort=stars",
        "content": serde_json::json!({
            "total_count": 2,
            "items": [
                {
                    "full_name": "cline/cline",
                    "html_url": "https://github.com/cline/cline",
                    "stargazers_count": 61000,
                    "description": "Autonomous coding agent in your IDE."
                },
                {
                    "full_name": "reworkd/AgentGPT",
                    "html_url": "https://github.com/reworkd/AgentGPT",
                    "stargazers_count": 35000,
                    "description": "Autonomous AI agents in your browser."
                }
            ]
        }).to_string(),
    })
    .to_string();

    let compact = DelegateTool::compact_structured_fetch_result(
        "Search GitHub for agent-browser",
        &payload,
        2,
    )
    .unwrap();
    assert!(compact.contains("result_summary"));
    assert!(compact.contains("https://github.com/cline/cline"));
    assert!(compact.contains("https://github.com/reworkd/AgentGPT"));
    assert!(!compact.contains("archive_url"));
}

#[test]
fn delegate_compacts_academic_structured_fetch_results() {
    let payload = serde_json::json!({
            "url": "https://api.crossref.org/works?query=lancet+heart+treatment",
            "content": serde_json::json!({
                "message": {
                    "total-results": 1,
                    "items": [
                        {
                            "title": ["Secondary prevention of cardiovascular disease: is it time for the polypill to be standard treatment?"],
                            "container-title": ["The Lancet Regional Health - Europe"],
                            "DOI": "10.1016/j.lanepe.2025.101384",
                            "URL": "https://www.thelancet.com/journals/lanepe/article/example",
                            "published-print": { "date-parts": [[2025, 8]] }
                        }
                    ]
                }
            }).to_string(),
        })
        .to_string();

    let compact = DelegateTool::compact_structured_fetch_result(
        "Search recent Lancet papers about heart disease treatment.",
        &payload,
        2,
    )
    .unwrap();

    assert!(compact.contains("result_summary"));
    assert!(compact.contains("polypill"));
    assert!(compact.contains("10.1016/j.lanepe.2025.101384"));
}

#[test]
fn delegate_compacts_pubmed_esummary_records() {
    let payload = serde_json::json!({
        "url": "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=123&retmode=json",
        "content": serde_json::json!({
            "result": {
                "uids": ["123"],
                "123": {
                    "uid": "123",
                    "pubdate": "2026 May",
                    "source": "Lancet",
                    "fulljournalname": "The Lancet",
                    "title": "Cardiovascular treatment after myocardial infarction: randomized trial.",
                    "pubtype": ["Journal Article", "Randomized Controlled Trial"],
                    "articleids": [
                        {"idtype": "pubmed", "value": "123"},
                        {"idtype": "doi", "value": "10.1016/example"}
                    ]
                }
            }
        }).to_string(),
    })
    .to_string();

    let compact = DelegateTool::compact_structured_fetch_result(
        "Find recent Lancet cardiovascular treatment papers",
        &payload,
        2,
    )
    .unwrap();

    assert!(compact.contains("result_summary"));
    assert!(compact.contains("Cardiovascular treatment"));
    assert!(compact.contains("10.1016/example"));

    let followups = DelegateTool::structured_lookup_followup_urls(&payload, 2);
    assert!(followups
        .iter()
        .any(|url| url == "https://pubmed.ncbi.nlm.nih.gov/123/"));
    assert!(followups
        .iter()
        .any(|url| url == "https://doi.org/10.1016/example"));
}

#[test]
fn delegate_marks_youtube_shell_fetch_as_blocker() {
    let payload = serde_json::json!({
            "url": "https://www.youtube.com/results?search_query=agent+browser",
            "content_quality": "boilerplate_only",
            "content": "AboutPressCopyrightContact usCreatorsAdvertiseDevelopersTermsPrivacyPolicy & SafetyHow YouTube worksTest new features",
        })
        .to_string();

    let blocker = DelegateTool::fetched_result_blocker(&payload).unwrap();
    assert!(blocker.contains("low-information"));
    assert!(!DelegateTool::fetched_result_looks_usable(&payload));
}

#[test]
fn delegate_recovery_contract_requires_real_tool_execution() {
    let task = DelegateTool::build_worker_pseudo_tool_recovery_contract(
        &AgentRole::Custom("researcher".to_string()),
        &["web_search".to_string()],
        "Search for BlockBeats news.",
        None,
        "<|tool_call>call:web_search{query: \"BlockBeats\"}<tool_call|>",
    );

    assert!(task.contains("REQUIRED RECOVERY STEP"));
    assert!(task.contains("actually call the matching real tool"));
    assert!(task.contains("Do not repeat `<|tool_call>` tags"));
    assert!(task.contains("web_search"));
}

#[tokio::test]
async fn delegate_retries_worker_pseudo_tool_tag_before_returning() {
    let coordinator = Arc::new(Coordinator::new());
    let role = AgentRole::Custom("mock_researcher".to_string());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: role.clone(),
        agent_path: PathBuf::from("/tmp/researcher"),
        display_name: "Researcher".to_string(),
        description: Some("Research specialist.".to_string()),
        tools: vec!["web_search".to_string()],
        artifact_policy: None,
    });
    coordinator.register(Arc::new(PseudoThenRealWorker::new(role)));

    let tool = DelegateTool::new(Arc::downgrade(&coordinator));
    let result = tool
        .call(r#"{"role":"mock_researcher","task":"Search BlockBeats latest news."}"#)
        .await
        .expect("delegate should recover");

    assert!(result.contains("executed_tool: web_search"));
    assert!(result.contains("real tool result"));
    assert!(!result.contains("<|tool_call>"));
}

#[test]
fn delegate_extracts_first_url_for_knowledge_fast_path() {
    let url = DelegateTool::first_url(
        "Import this source into the knowledge base: https://example.com/paper.",
    );

    assert_eq!(url.as_deref(), Some("https://example.com/paper"));
}

#[test]
fn delegate_compacts_lookup_query_for_lancet_research() {
    let query = DelegateTool::compact_lookup_query(
            "Search for the latest medical papers published in The Lancet regarding heart disease treatment. Focus on 2024, 2025, and 2026. Include links and DOIs.",
        );

    assert!(query.contains("site:thelancet.com"));
    assert!(query.contains("lancet"));
    assert!(query.to_ascii_lowercase().contains("heart"));
    assert!(query.contains("2025"));
    assert!(query.contains("2026"));
    assert!(query.contains("doi"));
}

#[test]
fn delegate_builds_academic_lookup_query_variants() {
    let variants = DelegateTool::lookup_query_variants(
        "请搜索柳叶刀最新治疗心脏病的论文，并给我 DOI、PubMed 或开放全文来源。",
    );

    assert!(variants.len() >= 2);
    assert!(variants.iter().any(|query| query.contains("pubmed")));
    assert!(variants.iter().any(|query| query.contains("doi")
        || query.contains("full text")
        || query.contains("open access")));
    assert!(variants.first().is_some_and(|query| {
        query.contains("pubmed") || query.contains("doi") || query.contains("full text")
    }));
}

#[test]
fn delegate_builds_site_and_artifact_lookup_query_variants() {
    let variants = DelegateTool::lookup_query_variants(
        "请帮我查 GitHub 上某个仓库的 README、issue 和最近提交。",
    );

    assert!(!variants.is_empty());
    assert!(variants.iter().any(|query| query.contains("github")
        && (query.contains("repository") || query.contains("readme"))));
}

#[test]
fn delegate_treats_novel_rankings_as_collection_not_data_records() {
    let task = "搜索起点中文网当前可公开访问的免费玄幻小说，找出排名前10部，把书名、作者、链接、来源、公开元数据和简介摘要保存进知识库。";

    assert!(DelegateTool::task_requests_collection_or_ranking(task));
    assert!(!DelegateTool::task_requests_data_or_records(task));

    let variants = DelegateTool::lookup_query_variants(task);
    assert!(variants.first().is_some_and(|query| {
        !query.contains("site:")
            && query.contains("起点")
            && query.contains("玄幻")
            && !query.contains("Search for")
    }));
    assert!(variants
        .iter()
        .any(|query| query.contains("site:qidian.com")));
    assert!(variants
        .iter()
        .any(|query| query.contains("ranking") || query.contains("novel")));
    assert!(!variants.iter().any(|query| {
        query
            .split_whitespace()
            .any(|token| token.eq_ignore_ascii_case("repo"))
    }));
    assert!(!variants.iter().any(|query| {
        let lowered = query.to_ascii_lowercase();
        lowered.contains("official results records data") || lowered.contains("开奖记录")
    }));
}

#[test]
fn delegate_preserves_explicit_search_phrases_before_generated_hints() {
    let variants = DelegateTool::lookup_query_variants(
            "Step 1: Search for \"起点中文网 2026 热门玄幻小说排行榜\" or \"起点中文网 免费玄幻小说推荐\" to find a current list.",
        );

    assert_eq!(
        variants.first().map(String::as_str),
        Some("起点中文网 2026 热门玄幻小说排行榜")
    );
    assert!(variants
        .iter()
        .any(|query| query == "起点中文网 免费玄幻小说推荐"));
}

#[test]
fn delegate_prioritizes_short_cjk_query_inside_english_task() {
    let variants = DelegateTool::lookup_query_variants(
            "Search for the top 10 recommended or trending free fantasy (玄幻) novels on Qidian (起点中文网) as of April 30, 2026.",
        );

    assert!(variants.first().is_some_and(|query| {
        !query.contains("site:")
            && query.contains("玄幻")
            && query.contains("起点中文网")
            && query.contains("推荐")
            && !query.contains("Search for")
    }));
    assert!(variants
        .iter()
        .any(|query| query.contains("site:qidian.com")));
}

#[test]
fn delegate_treats_mixed_english_qidian_metadata_task_as_verified_collection() {
    let task = "Search for the top 10 most recommended or trending free fantasy (玄幻) novels currently available on Qidian (起点中文网) as of May 1, 2026. For each novel, find the following metadata: Title, Author, URL, Source, and a brief summary/description. Ensure the sources are real and verifiable. Do not scrape the full text.";

    assert!(DelegateTool::task_requests_collection_or_ranking(task));
    assert!(!DelegateTool::task_requests_data_or_records(task));
    assert!(DelegateTool::task_requires_verified_fetch_result(task));
}

#[test]
fn delegate_qidian_collection_variants_include_broad_cjk_query() {
    let task = "Search for the top 10 recommended or trending fantasy (玄幻) novels currently available on Qidian (起点中文网) that have publicly accessible metadata. Specifically, look for lists, rankings, or recommendation pages. For each of the 10 novels, retrieve: Title, Author, Link, Source, and a short summary/metadata. Note: Do not scrape the full text. If a source is blocked or inaccessible, report it and find an alternative. The goal is to build a knowledge base of these 10 novels to analyze trends for a creative task. Date: 2026-05-01.";

    let variants = DelegateTool::lookup_query_variants(task);

    assert!(
        variants.iter().any(|query| query.contains("玄幻")
            && query.contains("起点")
            && query.contains("推荐")
            && !query.contains("site:")),
        "variants should include a broad CJK query without site lock, got: {variants:?}"
    );
}

#[test]
fn delegate_accepts_directory_candidates_for_collection_tasks() {
    let payload = serde_json::json!({
        "results": [
            {
                "title": "起点中文网 玄幻小说排行榜 免费小说推荐",
                "url": "https://www.qidian.com/rank/yuepiao/chanId21/",
                "snippet": "玄幻 小说 排行榜 书名 作者 简介 免费 阅读 推荐"
            }
        ]
    })
    .to_string();

    assert!(DelegateTool::search_output_has_usable_candidates(
        "搜索起点中文网免费玄幻小说排名前10部。",
        &payload
    ));
}

#[test]
fn delegate_builds_media_lookup_query_variants_without_domain_bucket() {
    let variants =
        DelegateTool::lookup_query_variants("请帮我查一个 YouTube 视频的字幕和讲解重点。");

    assert!(!variants.is_empty());
    assert!(variants.iter().any(|query| query.contains("youtube")
        || query.contains("video")
        || query.contains("caption")));
}

#[test]
fn delegate_expands_site_hints_using_site_policy() {
    let intent = DelegateTool::build_lookup_intent(
        "请搜索柳叶刀最新治疗心脏病的论文，并给我 DOI、PubMed 或开放全文来源。",
    );

    assert!(intent
        .site_hints
        .iter()
        .any(|hint| hint == "site:pubmed.ncbi.nlm.nih.gov"));
    assert!(intent
        .site_hints
        .iter()
        .any(|hint| hint == "site:api.crossref.org"));
}

#[test]
fn delegate_candidate_score_prefers_policy_alternative_sources() {
    let task = "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。";
    let preferred = DelegateTool::candidate_score(
        task,
        "https://pubmed.ncbi.nlm.nih.gov/12345678/",
        "PubMed record",
        "study abstract doi",
    );
    let publisher = DelegateTool::candidate_score(
        task,
        "https://www.thelancet.com/collections/cardiology-vascular-medicine",
        "The Lancet collection",
        "publisher collection page",
    );

    assert!(preferred > publisher);
}

#[test]
fn delegate_candidate_score_prefers_specific_general_data_sources() {
    let homepage_score = DelegateTool::candidate_score(
        "查找最近2个月中国福利彩票每期开奖数据并保存进知识库。",
        "http://www.cwl.gov.cn/",
        "中国福彩网_公益福彩_中国福利彩票官方网站",
        "提供双色球、福彩3D、七乐彩、开奖公告和其他福彩数据。",
    );
    let specific_score = DelegateTool::candidate_score(
        "查找最近2个月中国福利彩票每期开奖数据并保存进知识库。",
        "http://www.cwl.gov.cn/kjxx/",
        "中国福利彩票 开奖信息 开奖公告",
        "双色球、福彩3D、七乐彩开奖记录和开奖号码数据。",
    );

    assert!(homepage_score <= 0);
    assert!(specific_score > homepage_score);
    assert!(specific_score > 0);
}

#[test]
fn delegate_data_lookup_variants_include_record_terms() {
    let variants = DelegateTool::lookup_query_variants(
            "Search for the winning numbers of China Welfare Lottery 中国福利彩票 for the past 2 months.",
        );

    assert!(variants
        .iter()
        .any(|query| query.contains("winning numbers")
            || query.contains("开奖结果")
            || query.contains("开奖记录")));
    assert!(variants
        .iter()
        .any(|query| query.contains("official") || query.contains("官方")));
    assert!(!variants
        .iter()
        .any(|query| query.split_whitespace().next() == Some("result")));
}

#[test]
fn delegate_compacts_chinese_data_lookup_queries() {
    let variants = DelegateTool::lookup_query_variants(
        "查找2个月内中国福利彩票的每期开奖号码，放进知识库，然后预测下一期的开奖号码。",
    );

    assert!(variants
        .iter()
        .any(|query| query.contains("中国福利彩票") || query.contains("福利彩票")));
    assert!(variants.iter().any(|query| query.contains("开奖号码")));
    assert!(variants
        .iter()
        .any(|query| query.contains("site:cwl.gov.cn")));
    assert!(!variants
        .iter()
        .any(|query| query.starts_with("numbers ") || query.contains(" Search ")));
    assert!(!variants
        .iter()
        .any(|query| query.contains("放进知识库，然后预测下一期")));
}

#[test]
fn delegate_rejects_generic_result_dictionary_for_data_tasks() {
    let score = DelegateTool::candidate_score(
        "查找最近2个月中国福利彩票每期开奖数据并保存进知识库。",
        "https://dictionary.cambridge.org/us/dictionary/english/result",
        "RESULT | definition in the Cambridge English Dictionary",
        "RESULT meaning: something that happens because of something else.",
    );

    assert!(score <= 0);
}

#[test]
fn delegate_uses_homepage_only_as_data_discovery_source() {
    let task = "查找最近2个月中国福利彩票每期开奖数据并保存进知识库。";
    let search_result = serde_json::json!({
        "results": [
            {
                "title": "中国福彩网_公益福彩_中国福利彩票官方网站",
                "url": "https://www.cwl.gov.cn/",
                "snippet": "提供双色球、福彩3D、七乐彩、开奖公告和其他福彩数据。"
            }
        ]
    })
    .to_string();

    assert!(!DelegateTool::search_output_has_usable_candidates(
        task,
        &search_result
    ));
    assert_eq!(
        DelegateTool::best_discovery_fetch_urls(task, &search_result, 1),
        vec!["https://www.cwl.gov.cn/".to_string()]
    );
}

#[test]
fn delegate_recovers_fragmented_local_model_tool_args() {
    let args = DelegateTool::parse_delegate_args(
            r#"{"2026). The goal is to find a list of recent draw dates":null,"Welfare Lottery 7-star":null,"and their corresponding winning numbers. Please provide the data in a structured format.<|\"|>":null,"etc.)":null,"game types (e.g.":null,"role:<|\"|>researcher<|\"|>":null,"task:<|\"|>Search for the winning numbers of the China Welfare Lottery (中国福利彩票) for the last 2 months (approximately from late February 2026 to April 29":null}"#,
        )
        .unwrap();

    assert_eq!(args.role, "researcher");
    assert!(args.task.contains("China Welfare Lottery"));
    assert!(args.task.contains("winning numbers"));
    assert!(args.task.contains("structured format"));
}

#[test]
fn delegate_rejects_stale_results_for_recent_data_tasks() {
    let score = DelegateTool::candidate_score(
        "查找最近2个月中国福利彩票每期开奖数据并保存进知识库。",
        "http://example.com/lottery/2022/results",
        "开奖公告 福彩数据",
        "2022 年双色球、快乐8 开奖号码数据。",
    );

    assert!(score <= 0);
}

#[test]
fn delegate_rejects_recent_but_non_data_candidates_for_data_tasks() {
    let score = DelegateTool::candidate_score(
        "查找最近2个月中国福利彩票每期开奖数据并保存进知识库。",
        "https://www.zhihu.com/topic/19586942",
        "中华人民共和国 - 知乎",
        "2026年3月31日 中国和巴基斯坦提出关于恢复海湾和中东地区和平稳定的五点倡议。",
    );

    assert!(score <= 0);
}

#[test]
fn delegate_rejects_generic_numbers_app_for_lottery_data_tasks() {
    let score = DelegateTool::candidate_score(
            "Search for the winning numbers of China Welfare Lottery 中国福利彩票 for the past 2 months.",
            "https://support.apple.com/numbers",
            "Numbers - Official Apple Support",
            "Let Numbers do the math. Create formulas that perform calculations or manipulate data.",
        );

    assert!(score <= 0);
}

#[test]
fn delegate_rejects_social_or_login_redirects_for_data_records() {
    let score = DelegateTool::candidate_score(
        "查找2个月内中国福利彩票的每期开奖号码。",
        "http://mp.weixin.qq.com/s?src=11&timestamp=1777472532",
        "中国福利彩票“双色球”第2026047期开奖公告",
        "喜中一等奖1注奖2026047期双色球，22小时前。",
    );

    assert!(score <= 0);
}

#[test]
fn delegate_rejects_social_or_login_redirects_as_data_discovery() {
    let score = DelegateTool::candidate_discovery_score(
        "查找2个月内中国福利彩票的每期开奖号码。",
        "https://www.zhihu.com/org/zhong-guo-fu-li-cai-piao-49",
        "中国福利彩票",
        "开奖公告、开奖号码和历史记录。",
    );

    assert_eq!(score, 0);
}

#[test]
fn delegate_prefers_record_collection_pages_over_news_portals_for_data_tasks() {
    let task = "查找过去2个月内中国福利彩票双色球每期开奖号码，包括期号、开奖日期和中奖号码。";
    let history_page = DelegateTool::candidate_score(
        task,
        "https://cp.ip138.com/quanguo/",
        "全国彩票 开奖结果 全国彩票开奖查询 全国彩票开奖公告",
        "双色球 第2026046期 开奖结果 历史开奖号码 号码走势图 04-26",
    );
    let portal_page = DelegateTool::candidate_score(
        task,
        "https://www.zhcw.com/sy/tt/index.shtml",
        "中彩网_彩票行业垂直门户_更快开奖结果_中国彩票网",
        "双色球一等奖12注621万元，奖池15.59亿元",
    );

    assert!(history_page > portal_page);
    assert!(portal_page <= 0);
}

#[test]
fn delegate_followup_urls_skip_homepage_for_data_tasks() {
    let search_result = serde_json::json!({
        "results": [
            {
                "title": "Example Data Portal",
                "url": "https://example.com/",
                "snippet": "Official data records and results"
            },
            {
                "title": "Example Data Records",
                "url": "https://example.com/records/",
                "snippet": "Official data records and results"
            }
        ]
    })
    .to_string();

    let urls = DelegateTool::best_followup_fetch_urls(
        "Find recent official data records and results.",
        &search_result,
        5,
    );

    assert_eq!(urls, vec!["https://example.com/records/"]);
}

#[test]
fn delegate_collection_followup_urls_diversify_hosts() {
    let search_result = serde_json::json!({
        "results": [
            {
                "title": "起点中文网 玄幻 排行榜",
                "url": "https://www.qidian.com/rank/recom/chn21/",
                "snippet": "起点中文网玄幻小说推荐榜单"
            },
            {
                "title": "起点中文网 玄幻 免费",
                "url": "https://www.qidian.com/free/chanId21/",
                "snippet": "起点中文网免费玄幻小说"
            },
            {
                "title": "公开玄幻小说榜单整理",
                "url": "https://example.org/qidian-fantasy-ranking",
                "snippet": "整理起点中文网玄幻小说榜单书名作者链接"
            },
            {
                "title": "玄幻小说推荐榜",
                "url": "https://another.example/rank",
                "snippet": "公开可访问的玄幻小说推荐列表"
            }
        ]
    })
    .to_string();

    let urls = DelegateTool::best_followup_fetch_urls(
        "搜索起点中文网当前可公开访问的免费玄幻小说，找出排名/推荐前10部。",
        &search_result,
        3,
    );

    assert_eq!(urls.len(), 2);
    assert_eq!(
        urls.iter().filter(|url| url.contains("qidian.com")).count(),
        1
    );
    assert!(urls.iter().any(|url| url.contains("example.org")));
    assert!(!urls.iter().any(|url| url.contains("another.example")));
}

#[test]
fn delegate_collection_followup_rejects_unrelated_fallback_candidates() {
    let search_result = serde_json::json!({
        "results": [
            {
                "title": "Peak District - Wikipedia",
                "url": "https://en.wikipedia.org/wiki/Peak_District",
                "snippet": "Upland area in England"
            }
        ]
    })
    .to_string();

    let urls = DelegateTool::best_followup_fetch_urls(
        "Search for the top 10 recommended free fantasy Xuanhuan novels on Qidian 起点中文网.",
        &search_result,
        3,
    );

    assert!(urls.is_empty());
}

#[test]
fn delegate_collection_followup_rejects_app_store_noise_for_named_source_task() {
    let search_result = serde_json::json!({
        "results": [
            {
                "title": "McAfee Security: VPN Antivirus - Apps on Google Play",
                "url": "https://play.google.com/store/apps/details?id=com.wsandroid.suite&hl=en-US",
                "snippet": "Download this Android app from Google Play."
            },
            {
                "title": "起点中文网 玄幻小说排行榜",
                "url": "https://www.qidian.com/rank/recom/chn21/",
                "snippet": "起点中文网玄幻小说推荐榜单，公开书名、作者、分类和榜单信息。"
            },
            {
                "title": "起点中文网 免费玄幻小说榜单",
                "url": "https://www.qidian.com/free/chanId21/",
                "snippet": "起点中文网免费玄幻小说列表，公开书名、作者、分类和免费阅读状态。"
            }
        ]
    })
    .to_string();

    let urls = DelegateTool::best_followup_fetch_urls(
        "搜索起点中文网当前可公开访问的免费玄幻小说，找出排名/推荐前10部，保存进知识库。",
        &search_result,
        3,
    );

    assert!(!urls.iter().any(|url| url.contains("play.google.com")));
    assert!(urls.iter().any(|url| url.contains("qidian.com")));
}

#[test]
fn delegate_candidate_score_rejects_app_store_noise_for_non_app_data_task() {
    let score = DelegateTool::candidate_score(
        "搜索起点中文网当前可公开访问的免费玄幻小说，找出排名/推荐前10部，保存进知识库。",
        "https://play.google.com/store/apps/details?id=com.wsandroid.suite&hl=en-US",
        "McAfee Security: VPN Antivirus - Apps on Google Play",
        "Download this Android app from Google Play.",
    );

    assert!(score <= 0);
}

#[test]
fn delegate_detects_direct_url_candidates_for_collection_followup() {
    let search_result = serde_json::json!({
        "results": [
            {
                "title": "https://www.qidian.com/xuanhuan/",
                "url": "https://www.qidian.com/xuanhuan/",
                "snippet": "Directory seed",
                "source": "direct_url"
            }
        ]
    })
    .to_string();

    assert!(DelegateTool::search_output_has_direct_url_candidates(
        &search_result
    ));
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_embedded_records_complete_collection_metadata() {
    let records = (0..10)
        .map(|index| {
            serde_json::json!({
                "title": format!("玄幻书{}", index + 1),
                "url": format!("https://m.example.com/book/{}/", 1000 + index),
                "metadata": "author: test / category: 玄幻 / price: 0 / summary: public metadata"
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "kind": "browser_browse",
        "action": "extract_links",
        "result": {
            "records": records
        }
    });

    let completion = DelegateTool::try_browser_payload_record_collection(
        "搜索前十免费玄幻小说，保存内容后推理写小说",
        "https://m.example.com/free/",
        Some(&payload),
    )
    .expect("completion");

    assert!(completion.contains("status: completed"));
    assert!(completion.contains("direct_site_embedded_record_collection"));
    assert!(completion.contains("玄幻书10"));
}

#[test]
fn delegate_collection_summary_accepts_repeated_title_metadata_blocks() {
    let content = r#"
精品公开书单
共62本书
星海第一术士
青石灯|玄幻|连载中
少年在废墟学院中发现旧纪元术式。
加入书架 免费试读
云渊问道
白衣客|玄幻|连载中
宗门倾覆后，少女以残卷重开天门。
加入书架 免费试读
归墟灯火
折竹声|玄幻|完结
群山尽头的古灯照见诸神遗骨。
加入书架 免费试读
"#;
    let payload = serde_json::json!({
        "url": "https://example.com/booklist/detail/1",
        "content": content,
        "content_quality": "actionable"
    })
    .to_string();

    let summary = DelegateTool::compact_collection_fetch_summary(
        "搜索前3部免费玄幻小说，保存内容后推理写小说",
        "https://example.com/booklist/detail/1",
        &payload,
    )
    .expect("summary");

    assert!(summary.contains("星海第一术士"));
    assert!(summary.contains("云渊问道"));
    assert!(summary.contains("归墟灯火"));
    assert_eq!(DelegateTool::verified_ranked_metadata_count(&summary), 3);
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_payload_links_reads_structured_result_wrapper() {
    let payload = serde_json::json!({
        "kind": "browser_browse",
        "action": "extract_links",
        "result": {
            "links": [
                {"text": "Free", "url": "https://m.example.com/free/"},
                {"text": "Rank", "url": "https://m.example.com/rank/"}
            ]
        }
    });

    let links = DelegateTool::browser_payload_links(Some(&payload));

    assert_eq!(links.len(), 2);
    assert!(links.iter().any(|link| {
        link.get("url").and_then(|value| value.as_str()) == Some("https://m.example.com/free/")
    }));
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_item_links_reject_filter_navigation() {
    let task = "搜索免费玄幻小说前十部";

    assert!(!DelegateTool::link_looks_like_collection_item(
        task,
        "https://www.example.com/free/all/update1/",
        "三日内"
    ));
    assert!(!DelegateTool::link_looks_like_collection_item(
        task,
        "https://www.example.com/free/all/chanId80/",
        "古代言情"
    ));
    assert!(DelegateTool::link_looks_like_collection_item(
        task,
        "https://www.example.com/book/1048357493/",
        "星河铸骨"
    ));
    assert!(!DelegateTool::link_looks_like_collection_item(
        task,
        "https://www.example.com/chapter/1048357493/901386471/",
        "第1章 星火初燃"
    ));
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_item_links_reject_policy_navigation_noise() {
    let task = "搜索免费玄幻小说前十部";

    assert!(!DelegateTool::link_looks_like_collection_item(
        task,
        "https://www.example.com/about/intro",
        "关于我们"
    ));
    assert_eq!(
        DelegateTool::candidate_score(
            task,
            "https://www.example.com/about/intro",
            "网站介绍",
            "about this site"
        ),
        0
    );

    let payload = serde_json::json!({
        "results": [
            {
                "title": "网站介绍",
                "url": "https://www.example.com/about/intro",
                "snippet": "site introduction"
            },
            {
                "title": "星河铸骨",
                "url": "https://www.example.com/book/1048357493/",
                "snippet": "免费 玄幻 作者 作品详情"
            }
        ]
    })
    .to_string();

    let urls = DelegateTool::best_followup_fetch_urls(task, &payload, 2);

    assert_eq!(urls, vec!["https://www.example.com/book/1048357493/"]);
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_filter_navigation_requires_task_overlap() {
    let task = "搜索免费玄幻小说前十部";

    assert!(
        DelegateTool::link_is_filter_navigation_without_task_overlap(
            task,
            "古代言情",
            "https://www.example.com/free/all/chanId80/"
        )
    );
    assert!(
        DelegateTool::link_is_filter_navigation_without_task_overlap(
            task,
            "短篇已签约",
            "https://www.example.com/free/all/size2-sign1/"
        )
    );
    assert!(
        DelegateTool::link_is_filter_navigation_without_task_overlap(
            task,
            "杀手",
            "https://www.example.com/free/all/sign2-tag%E6%9D%80%E6%89%8B/"
        )
    );
    assert!(
        !DelegateTool::link_is_filter_navigation_without_task_overlap(
            task,
            "玄幻",
            "https://www.example.com/free/all/chanId21/"
        )
    );
    assert!(
        !DelegateTool::link_is_filter_navigation_without_task_overlap(
            task,
            "星河铸骨",
            "https://www.example.com/book/1048357493/"
        )
    );
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_detail_metadata_rejects_collection_index_pages() {
    let task = "搜索免费玄幻小说前十部并保存内容";

    assert!(!DelegateTool::browser_detail_metadata_satisfies_item(
        task,
        "免费小说大全_小说免费在线阅读 / 玄幻721722 / • 作品分类"
    ));
    assert!(DelegateTool::browser_detail_metadata_satisfies_item(
        task,
        "作者：某某 / 简介：少年踏入异界修行 / 分类：玄幻"
    ));
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_site_seeds_follow_declared_site_roots_and_policy_index_paths() {
    let urls = DelegateTool::browser_site_seed_urls("搜索起点中文网免费玄幻小说前10部");

    assert!(urls.contains(&"https://www.qidian.com/free/".to_string()));
    assert!(urls.contains(&"https://qidian.com/free/".to_string()));
    assert!(urls.contains(&"https://m.qidian.com/free/".to_string()));
    assert!(urls.contains(&"https://www.qidian.com/".to_string()));
    assert!(urls.contains(&"https://qidian.com/".to_string()));
    assert!(urls.contains(&"https://m.qidian.com/".to_string()));
    assert_eq!(
        urls.first().map(String::as_str),
        Some("https://m.qidian.com/free/")
    );
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_site_seeds_skip_static_api_policy_hosts() {
    let urls = DelegateTool::browser_site_seed_urls(
        "查找柳叶刀最近治疗心脏病论文，优先使用 PubMed Crossref OpenAlex。",
    );

    assert!(urls.iter().any(|url| url.contains("thelancet.com")));
    assert!(!urls.iter().any(|url| url.contains("api.crossref.org")));
    assert!(!urls.iter().any(|url| url.contains("api.openalex.org")));
    assert!(!urls
        .iter()
        .any(|url| url.contains("pubmed.ncbi.nlm.nih.gov")));
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_queries_strip_static_api_site_filters() {
    let query = DelegateTool::strip_static_only_site_filters_for_browser(
            "site:thelancet.com site:api.crossref.org site:api.openalex.org site:pubmed.ncbi.nlm.nih.gov site:pmc.ncbi.nlm.nih.gov site:doi.org heart treatment trial",
        );

    assert!(query.contains("site:thelancet.com"));
    assert!(query.contains("heart treatment trial"));
    assert!(!query.contains("api.crossref.org"));
    assert!(!query.contains("api.openalex.org"));
    assert!(!query.contains("pubmed.ncbi.nlm.nih.gov"));
    assert!(!query.contains("pmc.ncbi.nlm.nih.gov"));
    assert!(!query.contains("doi.org"));
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_queries_keep_only_one_dynamic_site_filter() {
    let query = DelegateTool::strip_static_only_site_filters_for_browser(
        "site:example.com site:news.example.org treatment trial",
    );

    assert!(query.contains("site:example.com"));
    assert!(!query.contains("site:news.example.org"));
    assert!(query.contains("treatment trial"));
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_site_link_score_uses_observed_overlap_without_path_facet_bonus() {
    let task = "搜索起点中文网免费玄幻小说前10部";
    let free_genre = DelegateTool::browser_site_link_score(
        task,
        "https://www.qidian.com/free/",
        "玄幻",
        "https://www.qidian.com/free/chanId21/",
    );
    let unrelated = DelegateTool::browser_site_link_score(
        task,
        "https://www.qidian.com/",
        "女生频道",
        "https://www.qidian.com/mm/",
    );
    let promotional = DelegateTool::browser_site_link_score(
        task,
        "https://www.qidian.com/book/1048574060/",
        "游戏中心",
        "https://game.example.com/CpGameHome/Index/navigation2017/gameId/771/serverId/154/",
    );

    assert!(free_genre > 0);
    assert!(free_genre > unrelated);
    assert_eq!(unrelated, 0);
    assert_eq!(promotional, 0);
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_link_collection_can_use_observed_collection_page() {
    let task = "搜索示例站免费玄幻小说前10部，并把公开元数据保存进知识库";
    let payload = serde_json::json!({
        "content": "免费 玄幻 排行 本页展示公开可访问的玄幻小说条目",
        "links": (1..=10).map(|index| {
            serde_json::json!({
                "text": format!("示例玄幻作品{index}"),
                "url": format!("https://www.example.com/book/{index}/")
            })
        }).collect::<Vec<_>>()
    });

    let completion = DelegateTool::try_browser_payload_link_collection(
        task,
        "https://www.example.com/free/fantasy/",
        Some(&payload),
    )
    .expect("link collection should complete from item-level links");

    assert!(completion.contains("status: completed"), "{completion}");
    assert!(completion.contains("lookup_strategy: direct_site_link_collection"));
    assert_eq!(
        DelegateTool::verified_ranked_metadata_count(&completion),
        10
    );
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_policy_can_filter_question_style_paths() {
    let task = "搜索示例站免费玄幻小说前10部";

    assert!(DelegateTool::url_looks_like_non_content_navigation(
        task,
        "https://www.example.com/ask/how-to-find-free-books"
    ));
    assert!(DelegateTool::url_looks_like_non_content_navigation(
        task,
        "https://www.example.com/questions/how-to-find-free-books"
    ));
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_lookup_query_preserves_availability_constraints() {
    let task = "搜索起点玄幻小说把可以下载的免费玄幻小说下载前10部";

    let queries = DelegateTool::browser_lookup_query_variants(task);
    let first = queries.first().cloned().unwrap_or_default();
    let joined = queries.join("\n");

    assert!(first.contains("免费"), "{first}");
    assert!(first.contains("下载"), "{first}");
    assert!(joined.contains("免费"), "{joined}");
    assert!(joined.contains("下载"), "{joined}");
    assert!(joined.contains("玄幻"), "{joined}");
    assert!(joined.contains("小说"), "{joined}");
    assert!(!first.contains("把可"), "{first}");
    assert!(!first.contains("索起"), "{first}");
}

#[test]
fn delegate_cjk_lookup_query_drops_workflow_fragments() {
    let task = "搜索起点玄幻小说把可以下载的免费玄幻小说下载前10部，之后放到知识库，对知识库里的小说进行推理";

    let joined = DelegateTool::lookup_query_variants(task).join("\n");

    assert!(joined.contains("免费"), "{joined}");
    assert!(joined.contains("下载"), "{joined}");
    assert!(joined.contains("玄幻"), "{joined}");
    assert!(!joined.contains("把可"), "{joined}");
    assert!(!joined.contains("可以下"), "{joined}");
    assert!(!joined.contains("前1"), "{joined}");
    assert!(
        !joined.split_whitespace().any(|term| term == "幻小说"),
        "{joined}"
    );
}

#[test]
fn delegate_lookup_query_filters_internal_policy_identifiers() {
    let task = "Search public ranking records for fantasy novels free download collection ranking ranked_collection verify_items import_metadata item_records source_page title_author_link knowledge_import ingest site:example.com";

    let joined = DelegateTool::lookup_query_variants(task).join("\n");

    assert!(!joined.contains("ranked_collection"), "{joined}");
    assert!(!joined.contains("verify_items"), "{joined}");
    assert!(!joined.contains("import_metadata"), "{joined}");
    assert!(!joined.contains("item_records"), "{joined}");
    assert!(!joined.contains("source_page"), "{joined}");
    assert!(!joined.contains("title_author_link"), "{joined}");
    assert!(!joined.contains("knowledge_import"), "{joined}");
    assert!(!joined.contains(" ingest "), "{joined}");
    assert!(joined.contains("fantasy"), "{joined}");
    assert!(joined.contains("free"), "{joined}");
    assert!(joined.contains("download"), "{joined}");
}

#[test]
fn delegate_collection_fetch_requires_item_level_evidence() {
    let task = "搜索起点中文网免费玄幻小说前10部";
    let irrelevant_full_text_page = serde_json::json!({
        "url": "https://www.thefreedictionary.com/full",
        "content_quality": "actionable",
        "content": "Full - definition of full by The Free Dictionary. Dictionary, Encyclopedia and Thesaurus. Free online dictionary."
    })
    .to_string();
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &irrelevant_full_text_page
    ));

    let directory_shell = serde_json::json!({
        "url": "https://www.qidian.com/rank/recom/chn21/",
        "content": "玄幻 排行 免费 首页 登录 注册 更多",
        "links": [
            {"text": "首页", "url": "https://www.qidian.com/"},
            {"text": "排行", "url": "https://www.qidian.com/rank/"},
            {"text": "免费", "url": "https://www.qidian.com/free/"}
        ]
    })
    .to_string();
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &directory_shell
    ));

    let item_links = serde_json::json!({
            "url": "https://www.qidian.com/rank/recom/chn21/",
            "content": "玄幻榜单 本页列出公开可访问的多部玄幻小说条目，每个条目包含书名、作者、链接和公开简介摘要，用于构成榜单级证据，而不是入口目录页。",
            "links": (1..=10).map(|index| {
                serde_json::json!({
                    "text": format!("原创玄幻书名{index}"),
                    "url": format!("https://www.qidian.com/book/{index}/")
                })
            }).collect::<Vec<_>>()
        })
        .to_string();
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &item_links
    ));

    let browser_snapshot = serde_json::json!({
        "url": "https://www.example.com/rank/recom/chn21/",
        "content_quality": "actionable",
        "content": "\
    推荐榜本周作品推荐票数排行\n\
    •\nNO.1\n夜无疆\n辰东|玄幻·东方玄幻|连载\n最新更新 第693章\n\
    •\n2\n青山\n会说话的肘子|玄幻·东方玄幻|连载\n\
    •\n3\n诡秘之主\n爱潜水的乌贼|玄幻·异世大陆|完本\n\
    •\n4\n元始法则\n飞天鱼|玄幻·异世大陆|连载\n\
    •\n5\n大道之上\n宅猪|玄幻·东方玄幻|连载\n\
    •\n6\n宿命之环\n爱潜水的乌贼|玄幻·异世大陆|完本\n\
    •\n7\n万相之王\n天蚕土豆|玄幻·东方玄幻|连载\n\
    •\n8\n神印王座2皓月当空\n唐家三少|玄幻·异世大陆|完本\n\
    •\n9\n人道大圣\n莫默|玄幻·东方玄幻|完本\n\
    •\n10\n赤心巡天\n情何以甚|仙侠·古典仙侠|完本"
    })
    .to_string();
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &browser_snapshot
    ));

    let matching_free_snapshot = serde_json::json!({
        "url": "https://www.example.com/rank/free/fantasy/",
        "content_quality": "actionable",
        "content": "\
    免费榜本周免费玄幻作品排行\n\
    •\nNO.1\n夜无疆\n辰东|玄幻·东方玄幻|免费阅读\n最新更新 第693章\n\
    •\n2\n青山\n会说话的肘子|玄幻·东方玄幻|免费阅读\n\
    •\n3\n诡秘之主\n爱潜水的乌贼|玄幻·异世大陆|免费阅读\n\
    •\n4\n元始法则\n飞天鱼|玄幻·异世大陆|免费阅读\n\
    •\n5\n大道之上\n宅猪|玄幻·东方玄幻|免费阅读\n\
    •\n6\n宿命之环\n爱潜水的乌贼|玄幻·异世大陆|免费阅读\n\
    •\n7\n万相之王\n天蚕土豆|玄幻·东方玄幻|免费阅读\n\
    •\n8\n神印王座2皓月当空\n唐家三少|玄幻·异世大陆|免费阅读\n\
    •\n9\n人道大圣\n莫默|玄幻·东方玄幻|免费阅读\n\
    •\n10\n大荒剑主\n无名作者|玄幻·东方玄幻|免费阅读"
    })
    .to_string();
    assert!(DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &matching_free_snapshot
    ));
}

#[test]
fn delegate_source_material_fetch_must_match_downstream_intent_before_ingest() {
    let task = "搜索一个科幻星际类型小说，尝试入知识库，根据这个的基础来写小说 50万字";
    let unrelated_source = serde_json::json!({
        "url": "https://www.gutenberg.org/ebooks/26908.txt.utf-8",
        "content_quality": "actionable",
        "content": "The Project Gutenberg eBook of Conversations on Chemistry, V. 1-2. Author: Mrs. Marcet. This volume explains chemistry lessons and experiments."
    })
    .to_string();
    assert!(DelegateTool::task_requires_verified_fetch_result(task));
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &unrelated_source
    ));

    let matching_source = serde_json::json!({
        "url": "https://www.example.org/books/interstellar-science-fiction-novel.txt",
        "content_quality": "actionable",
        "content": "A science fiction interstellar novel about deep space travel, alien civilizations, starships, and cosmic worldbuilding."
    })
    .to_string();
    assert!(DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &matching_source
    ));

    let non_narrative_source = serde_json::json!({
        "url": "https://www.gutenberg.org/ebooks/77131.txt.utf-8",
        "content_quality": "actionable",
        "content": "The Project Gutenberg eBook of Drug themes in science fiction. Author: Robert Silverberg. This essay surveys drug themes in science fiction and offers criticism, commentary, and bibliography."
    })
    .to_string();
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &non_narrative_source
    ));

    let generic_wrong_genre_source = serde_json::json!({
        "url": "https://en.wikipedia.org/wiki/Erotic_fiction",
        "content_quality": "actionable",
        "content": "Erotic fiction is a genre of fiction and includes novels, stories, literary history, and sexual fantasy."
    })
    .to_string();
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        "Search for popular, downloadable fantasy (Xuanhuan) web novels online as source material.",
        &generic_wrong_genre_source
    ));
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        "搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。",
        &generic_wrong_genre_source
    ));

    let generic_fantasy_romance_source = serde_json::json!({
        "url": "https://www.gutenberg.org/ebooks/56177.txt.utf-8",
        "content_quality": "actionable",
        "content": "The Project Gutenberg eBook of The Island of Fantasy: A Romance. This is a fantasy romance novel with adventure and island mystery."
    })
    .to_string();
    let xuanhuan_source_task = "Search for a popular, downloadable fantasy (Xuanhuan) novel online. Identify usable source material.\n\nOriginal user request:\n搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库，然后基于素材写全新的玄幻小说。";
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        xuanhuan_source_task,
        &generic_fantasy_romance_source
    ));
    let completion = DelegateTool::format_research_fetch_completion(
        xuanhuan_source_task,
        "https://www.gutenberg.org/ebooks/56177.txt.utf-8",
        Some("热门玄幻小说 下载"),
        None,
        r#"{"results":[]}"#,
        &generic_fantasy_romance_source,
    );
    assert!(
        completion.starts_with("status: blocked\nworker: researcher"),
        "{completion}"
    );

    let generic_fantasy_novel_source = serde_json::json!({
        "url": "https://www.gutenberg.org/ebooks/67143.txt.utf-8",
        "content_quality": "actionable",
        "content": "The Project Gutenberg eBook of Fantasy: A Novel. This eBook is for the use of anyone anywhere in the United States at no cost. Fantasy is an English novel."
    })
    .to_string();
    let runtime_style_task = "Search for a popular, downloadable fantasy (Xuanhuan) novel online. Find and extract its text or key creative materials (plot, settings, character archetypes) to be used as reference material. Provide a summary of the findings or the text itself.\n\n完整用户请求（必须保留查找之后的后续阶段，不能只完成查找片段）：搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说，不能简单复制素材内容，要求情节完善、角色名字不漂移、总长度超过50万字，并保存成txt文件。";
    assert!(DelegateTool::task_requests_narrative_source_material(
        runtime_style_task
    ));
    assert!(DelegateTool::task_requires_verified_fetch_result(
        runtime_style_task
    ));
    assert!(!DelegateTool::task_requests_data_or_records(
        runtime_style_task
    ));
    assert!(!DelegateTool::task_requests_collection_or_ranking(
        runtime_style_task
    ));
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        runtime_style_task,
        &generic_fantasy_novel_source
    ));
    let runtime_completion = DelegateTool::format_research_fetch_completion(
        runtime_style_task,
        "https://www.gutenberg.org/ebooks/67143.txt.utf-8",
        Some("热门玄幻小说 下载"),
        None,
        r#"{"results":[]}"#,
        &generic_fantasy_novel_source,
    );
    assert!(
        runtime_completion.starts_with("status: blocked\nworker: researcher"),
        "{runtime_completion}"
    );

    let runtime_gutenberg_romance_source = serde_json::json!({
        "url": "https://www.gutenberg.org/ebooks/56177.txt.utf-8",
        "content_quality": "actionable",
        "content": "The Project Gutenberg eBook of The Island of Fantasy: A Romance. Title: The Island of Fantasy: A Romance. Language: English. CHAPTER I. A MIND DISEASED. Maurice Roylands sits in Roylands Grange and speaks with Rector Carriston in an English romance."
    })
    .to_string();
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        runtime_style_task,
        &runtime_gutenberg_romance_source
    ));
    let runtime_romance_completion = DelegateTool::format_research_fetch_completion(
        runtime_style_task,
        "https://www.gutenberg.org/ebooks/56177.txt.utf-8",
        Some("热门玄幻小说 下载"),
        None,
        r#"{"results":[]}"#,
        &runtime_gutenberg_romance_source,
    );
    assert!(
        runtime_romance_completion.starts_with("status: blocked\nworker: researcher"),
        "{runtime_romance_completion}"
    );
}

#[test]
fn delegate_time_sensitive_lookup_blocks_misaligned_fetch() {
    let task = "Search for the current weather in Beijing for May 13, 2026. Please provide the temperature, weather conditions, and the source.";
    let unrelated_fetch = serde_json::json!({
        "url": "https://en.wikipedia.org/?curid=9455579",
        "content_quality": "actionable",
        "content": "Fuzhou Changle International Airport is an airport serving Fuzhou, Fujian, China. The page includes history, airlines, destinations, and climate."
    })
    .to_string();
    assert!(DelegateTool::task_requests_time_sensitive_lookup(task));
    assert!(DelegateTool::task_requires_verified_fetch_result(task));
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &unrelated_fetch
    ));

    let completion = DelegateTool::format_research_fetch_completion(
        task,
        "https://en.wikipedia.org/?curid=9455579",
        Some("Beijing weather May 13 2026"),
        None,
        r#"{"results":[]}"#,
        &unrelated_fetch,
    );
    assert!(
        completion.starts_with("status: blocked\nworker: researcher"),
        "{completion}"
    );

    let aligned_fetch = serde_json::json!({
        "url": "https://example.test/weather/beijing",
        "content_quality": "actionable",
        "content": "Beijing weather forecast for May 13, 2026: cloudy, temperature 22 C to 31 C, light wind. Source updated today."
    })
    .to_string();
    assert!(DelegateTool::fetched_result_looks_usable_for_task(
        task,
        &aligned_fetch
    ));
}

#[test]
fn delegate_fast_path_demotes_completed_fetch_when_source_contract_fails() {
    let task = "Original user request:\n搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说。\n\nDelegated task:\nSearch for a popular, downloadable fantasy (Xuanhuan) novel online as source material.";
    let fetched = serde_json::json!({
        "url": "https://www.gutenberg.org/ebooks/56177.txt.utf-8",
        "content_quality": "actionable",
        "content": "The Project Gutenberg eBook of The Island of Fantasy: A Romance. CHAPTER I. Maurice Roylands speaks with Rector Carriston in an English romance."
    })
    .to_string();
    let completed = format!(
        "status: completed\nworker: researcher\nexecuted_tool: web_fetch\nsource_url: https://www.gutenberg.org/ebooks/56177.txt.utf-8\nsearch_query: 热门玄幻小说 novel fantasy download\nfetched_result:\n{}\n\nsearch_result_preview:\n{{\"results\":[]}}",
        fetched
    );

    let guarded = DelegateTool::guard_fast_path_completion_against_source_contract(
        &AgentRole::Custom("researcher".to_string()),
        task,
        completed,
    );

    assert!(
        guarded.starts_with("status: blocked\nworker: researcher"),
        "{guarded}"
    );
    assert!(guarded.contains("source_material_mismatch"), "{guarded}");
    assert!(
        DelegateTool::fast_path_blocker_should_fall_back(
            &AgentRole::Custom("researcher".to_string()),
            task,
            &guarded
        ),
        "{guarded}"
    );
}

#[test]
fn delegate_knowledge_import_requires_aligned_source_body_for_downstream_material() {
    let simple_import =
        "Import this concrete source URL into the knowledge base exactly once. URL: https://example.org/book.txt";
    assert!(
        !DelegateTool::knowledge_import_requires_source_alignment_evidence(simple_import),
        "{simple_import}"
    );
    assert!(DelegateTool::knowledge_import_source_alignment_blocker(simple_import).is_none());

    let url_only_downstream_import = "Import this concrete source URL into the knowledge base exactly once. Do not run another lookup. URL: https://www.gutenberg.org/ebooks/67143.txt.utf-8\n\n完整用户请求（必须保留查找之后的后续阶段，不能只完成查找片段）：搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说。";
    assert!(
        DelegateTool::knowledge_import_requires_source_alignment_evidence(
            url_only_downstream_import
        )
    );
    assert!(
        DelegateTool::knowledge_import_source_alignment_blocker(url_only_downstream_import)
            .is_some()
    );

    let mismatched_evidence_import = "Import this concrete source URL into the knowledge base exactly once. URL: https://www.gutenberg.org/ebooks/67143.txt.utf-8\n\nfetched_result:\n{\"url\":\"https://www.gutenberg.org/ebooks/67143.txt.utf-8\",\"content_quality\":\"actionable\",\"content\":\"The Project Gutenberg eBook of Fantasy: A Novel. This is an English fantasy novel.\"}\n\n完整用户请求：搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说。";
    assert!(
        DelegateTool::knowledge_import_source_alignment_blocker(mismatched_evidence_import)
            .is_some()
    );

    let aligned_evidence_import = "Import this concrete source URL into the knowledge base exactly once. URL: https://example.org/xuanhuan.txt\n\nfetched_result:\n{\"url\":\"https://example.org/xuanhuan.txt\",\"content_quality\":\"actionable\",\"content\":\"第一章 少年从边荒醒来，玄幻大陆灵脉复苏，宗门、妖族与古老神碑交织成新的修炼故事。正文持续展开人物命运与境界突破。\"}\n\n完整用户请求：搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说。";
    assert!(
        DelegateTool::knowledge_import_source_alignment_blocker(aligned_evidence_import).is_none()
    );
}

#[test]
fn delegate_lookup_variants_preserve_cjk_scifi_interstellar_intent() {
    let variants = DelegateTool::lookup_query_variants("搜索一个科幻星际类型小说，尝试入知识库");
    let joined = variants.join("\n").to_ascii_lowercase();

    assert!(
        joined.contains("science fiction") || joined.contains("科幻"),
        "{joined}"
    );
    assert!(
        joined.contains("interstellar") || joined.contains("星际"),
        "{joined}"
    );
}

#[test]
fn delegate_collection_summary_requires_metadata_when_user_asks_for_content_details() {
    let task = "搜索起点中文网前十免费的玄幻小说内容并保存进知识库";
    let listing_without_details = serde_json::json!({
        "url": "https://www.example.com/free/fantasy/",
        "content_quality": "actionable",
        "content": "\
    免费榜玄幻作品排行\n\
    •\nNO.1\n作品一\n\
    •\n2\n作品二\n\
    •\n3\n作品三\n\
    •\n4\n作品四\n\
    •\n5\n作品五\n\
    •\n6\n作品六\n\
    •\n7\n作品七\n\
    •\n8\n作品八\n\
    •\n9\n作品九\n\
    •\n10\n作品十"
    })
    .to_string();

    assert!(DelegateTool::compact_collection_fetch_summary(
        task,
        "https://www.example.com/free/fantasy/",
        &listing_without_details
    )
    .is_none());
}

#[test]
fn delegate_collection_intent_rejects_conflicting_ranking_facets() {
    let free_task = "搜索起点中文网所有玄幻小说免费榜前十";
    let recommendation_payload = serde_json::json!({
        "url": "https://www.example.com/rank/recom/fantasy/",
        "content_quality": "actionable",
        "content": "\
    推荐榜本周作品推荐票数排行\n\
    •\nNO.1\n夜无疆\n辰东|玄幻·东方玄幻|连载\n\
    •\n2\n青山\n会说话的肘子|玄幻·东方玄幻|连载\n\
    •\n3\n诡秘之主\n爱潜水的乌贼|玄幻·异世大陆|完本\n\
    •\n4\n元始法则\n飞天鱼|玄幻·异世大陆|连载\n\
    •\n5\n大道之上\n宅猪|玄幻·东方玄幻|连载\n\
    •\n6\n宿命之环\n爱潜水的乌贼|玄幻·异世大陆|完本\n\
    •\n7\n万相之王\n天蚕土豆|玄幻·东方玄幻|连载\n\
    •\n8\n神印王座2皓月当空\n唐家三少|玄幻·异世大陆|完本\n\
    •\n9\n人道大圣\n莫默|玄幻·东方玄幻|完本\n\
    •\n10\n赤心巡天\n情何以甚|仙侠·古典仙侠|完本"
    })
    .to_string();

    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        free_task,
        &recommendation_payload
    ));
    assert!(DelegateTool::collection_intent_alignment_blocker(
        free_task,
        "https://www.example.com/rank/recom/fantasy/",
        &recommendation_payload,
    )
    .is_some_and(|reason| reason.contains("source intent mismatch")));

    let recommendation_task = "搜索某站玄幻小说推荐榜前十";
    let free_payload = serde_json::json!({
        "url": "https://www.example.com/rank/free/fantasy/",
        "content_quality": "actionable",
        "content": "\
    免费榜本周免费玄幻作品排行\n\
    •\nNO.1\n夜无疆\n辰东|玄幻·东方玄幻|免费阅读\n\
    •\n2\n青山\n会说话的肘子|玄幻·东方玄幻|免费阅读\n\
    •\n3\n诡秘之主\n爱潜水的乌贼|玄幻·异世大陆|免费阅读\n\
    •\n4\n元始法则\n飞天鱼|玄幻·异世大陆|免费阅读\n\
    •\n5\n大道之上\n宅猪|玄幻·东方玄幻|免费阅读\n\
    •\n6\n宿命之环\n爱潜水的乌贼|玄幻·异世大陆|免费阅读\n\
    •\n7\n万相之王\n天蚕土豆|玄幻·东方玄幻|免费阅读\n\
    •\n8\n神印王座2皓月当空\n唐家三少|玄幻·异世大陆|免费阅读\n\
    •\n9\n人道大圣\n莫默|玄幻·东方玄幻|免费阅读\n\
    •\n10\n赤心巡天\n情何以甚|仙侠·古典仙侠|免费阅读"
    })
    .to_string();

    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        recommendation_task,
        &free_payload
    ));
}

#[test]
fn delegate_collection_fetch_rejects_unstructured_free_directory_page() {
    let payload = serde_json::json!({
        "url": "https://www.qidian.com/free/",
        "content_quality": "actionable",
        "content": "\
    限时免费小说\n\
    玄幻721722 奇幻159241 都市374244\n\
    •\n华娱：我来修个仙\n壶虎狐|都市·娱乐明星|连载中\n\
    •\n从日常技艺开始肝出个长生\n落选艺术生|玄幻·东方玄幻|连载中\n\
    •\n重生从换亲上门开始\n洒家李狗蛋|都市·都市生活|连载中",
        "links": []
    })
    .to_string();

    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        "搜索起点中文网所有玄幻小说免费榜前十",
        &payload
    ));
    assert!(DelegateTool::compact_collection_fetch_summary(
        "搜索起点中文网所有玄幻小说免费榜前十",
        "https://www.qidian.com/free/",
        &payload
    )
    .is_none());
}

#[test]
fn delegate_search_index_fallback_marks_metadata_scope() {
    let search_result = serde_json::json!({
        "results": (1..=10).map(|index| {
            serde_json::json!({
                "title": format!("《公开玄幻样本{index}》"),
                "url": format!("https://example.com/book/{index}/"),
                "snippet": format!("免费阅读 玄幻 作者{index} 公开简介摘要")
            })
        }).collect::<Vec<_>>()
    })
    .to_string();

    let result = DelegateTool::format_search_index_collection_completion(
        "搜索某站免费玄幻小说前十，给出书名作者链接和公开简介摘要",
        Some("某站 免费 玄幻 小说 前十"),
        &search_result,
    )
    .expect("search index fallback");

    assert!(result.starts_with("status: completed\nworker: researcher"));
    assert!(result.contains("lookup_strategy: search_index_evidence_fallback"));
    assert!(result.contains("evidence_scope: search_index_metadata_not_page_content"));
    assert!(result.contains("observed_item_records: 10"));
}

#[test]
fn delegate_search_index_fallback_blocks_full_content_import() {
    let search_result = serde_json::json!({
        "results": (1..=10).map(|index| {
            serde_json::json!({
                "title": format!("《公开玄幻样本{index}》"),
                "url": format!("https://example.com/book/{index}/"),
                "snippet": format!("免费阅读 玄幻 作者{index} 公开简介摘要")
            })
        }).collect::<Vec<_>>()
    })
    .to_string();

    let result = DelegateTool::format_search_index_collection_completion(
        "搜索某站免费玄幻小说前十，并把小说内容存到知识库",
        Some("某站 免费 玄幻 小说 前十"),
        &search_result,
    )
    .expect("search index fallback");

    assert!(result.starts_with("status: blocked\nworker: researcher"));
    assert!(result.contains("search index evidence only provides public metadata"));
    assert!(result.contains("not importable full content"));
}

#[test]
fn delegate_search_index_full_content_surrogate_allows_transformative_followup() {
    let search_result = serde_json::json!({
        "results": (1..=10).map(|index| {
            serde_json::json!({
                "title": format!("《公开玄幻样本{index}》"),
                "url": format!("https://example.com/book/{index}/"),
                "snippet": format!("免费阅读 玄幻 作者{index} 公开简介摘要")
            })
        }).collect::<Vec<_>>()
    })
    .to_string();

    let result = DelegateTool::format_search_index_collection_completion(
        "搜索某站免费玄幻小说前十，把这些小说内容存到知识库里，进行推理之后写一个50万字的玄幻小说",
        Some("某站 免费 玄幻 小说 前十"),
        &search_result,
    )
    .expect("search index fallback");

    assert!(result.starts_with("status: completed\nworker: researcher"));
    assert!(result.contains("evidence_scope: public_metadata_surrogate_not_full_source_content"));
    assert!(result.contains("full source content was not imported"));
}

#[test]
fn delegate_source_content_observed_fetch_is_not_blocked() {
    let payload = serde_json::json!({
        "url": "https://example.com/rank",
        "content": "NO.1\n夜无疆\n2\n青山\n3\n诡秘之主",
        "content_quality": "actionable",
        "orchestration_decision": {
            "can_finalize_answer": true
        },
        "verification_followup": {
            "answer_readiness": "source_content_observed"
        }
    })
    .to_string();

    assert!(!DelegateTool::fetched_result_requires_more_evidence(
        &payload
    ));
}

#[test]
fn delegate_collection_summary_does_not_treat_next_title_as_metadata() {
    let task = "搜索起点中文网玄幻小说月票榜前10部，放进知识库";
    let payload = serde_json::json!({
        "url": "https://www.qidian.com/rank/chn21/",
        "content_quality": "actionable",
        "content": "\
    月票榜更多\n\
    •\nNO.1\n星河之主\n16536月票\n玄幻·烽仙\n\
    •\n2\n苟在武道世界成圣\n12342\n\
    •\n3\n夜无疆\n6666\n"
    })
    .to_string();

    let summary = DelegateTool::compact_collection_fetch_summary(
        task,
        "https://www.qidian.com/rank/chn21/",
        &payload,
    )
    .expect("collection summary");

    assert!(summary.contains("星河之主 | public metadata: 玄幻·烽仙"));
    assert!(summary.contains("苟在武道世界成圣 | public metadata: metadata not visible"));
    assert!(!summary.contains("苟在武道世界成圣 | public metadata: 夜无疆"));
}

#[test]
fn delegate_fetch_completion_prioritizes_fetched_evidence_before_search_preview() {
    let result = DelegateTool::format_research_fetch_completion(
        "Search for example rank",
        "https://example.com/rank",
        Some("site:example.com 玄幻 排行"),
        None,
        &"search noise ".repeat(400),
        r#"{"url":"https://example.com/rank","content":"书名1 作者1 简介1"}"#,
    );

    let fetched_index = result
        .find("fetched_result:")
        .expect("fetched result should be present");
    let search_index = result
        .find("search_result_preview:")
        .expect("search preview should be present");
    assert!(fetched_index < search_index);
    assert!(result.contains("search_query: site:example.com 玄幻 排行"));
    assert!(result.len() < 2_000);
}

#[test]
fn delegate_empty_fetch_payload_requires_more_evidence() {
    let payload = serde_json::json!({
        "content": "",
        "content_quality": "empty",
        "orchestration_decision": {
            "can_finalize_answer": false
        },
        "verification_followup": {
            "answer_readiness": "verification_pending"
        }
    })
    .to_string();

    assert!(DelegateTool::fetched_result_requires_more_evidence(
        &payload
    ));
    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        "Search for the top 10 novels.",
        &payload
    ));
}

#[test]
fn delegate_empty_fetch_completion_is_blocked_not_completed() {
    let empty_fetch = serde_json::json!({
        "url": "https://example.com/rank",
        "content": "",
        "content_quality": "empty",
        "orchestration_decision": {
            "can_finalize_answer": false
        },
        "verification_followup": {
            "answer_readiness": "verification_pending"
        }
    })
    .to_string();

    let result = DelegateTool::format_research_fetch_completion(
        "Search for the top 10 novels.",
        "https://example.com/rank",
        Some("example rank"),
        None,
        r#"{"results":[]}"#,
        &empty_fetch,
    );

    assert!(result.starts_with("status: blocked\nworker: researcher"));
    assert!(result.contains("blockers: source returned low-information content"));
    assert!(!result.starts_with("status: completed"));
}

#[test]
fn delegate_static_followup_skips_browser_only_hosts_for_verified_source_tasks() {
    let task = "搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库，然后基于素材写作";
    let results = serde_json::json!({
        "results": [
            {
                "title": "热门玄幻小说推荐",
                "url": "https://mp.weixin.qq.com/s/example",
                "snippet": "玄幻 小说 下载 正文"
            },
            {
                "title": "玄幻小说正文阅读",
                "url": "https://example.org/xuanhuan/full-text",
                "snippet": "玄幻 小说 正文 可读取"
            }
        ]
    })
    .to_string();

    let urls = DelegateTool::best_followup_fetch_urls(task, &results, 5);

    assert_eq!(urls, vec!["https://example.org/xuanhuan/full-text"]);
}

#[test]
fn delegate_narrative_source_requires_cjk_material_term_when_requested() {
    let task = "Original user request:\n搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。\n\nDelegated task:\nSearch for popular fantasy novels available online for downloading.";
    let evidence = "The Project Gutenberg eBook of The Island of Fantasy: A Romance. Language: English. CHAPTER I. A MIND DISEASED.";

    assert!(!DelegateTool::fetch_payload_matches_cjk_narrative_material_terms(task, evidence));
    assert!(!DelegateTool::fetch_payload_matches_requested_material_type(task, evidence));
}

#[test]
fn delegate_recognizes_worker_status_blocks() {
    assert!(DelegateTool::looks_like_worker_status_block(
        "status: completed\nworker: researcher\nexecuted_tool: web_fetch"
    ));
    assert!(!DelegateTool::looks_like_worker_blocker_status(
        "status: completed\nworker: researcher\nexecuted_tool: web_fetch"
    ));
    assert!(DelegateTool::looks_like_worker_blocker_status(
        "status: blocked\nworker: researcher\nblockers: no evidence"
    ));
    assert!(!DelegateTool::looks_like_worker_status_block(
        "{\"status\":\"completed\"}"
    ));
}

#[test]
fn delegate_uses_structured_page_links_as_followup_candidates() {
    let fetch_payload = serde_json::json!({
        "url": "https://example.com/",
        "content": "Example portal",
        "links": [
            {"text": "Home", "url": "https://example.com/"},
            {"text": "Official data records", "url": "https://example.com/data/records/"}
        ]
    })
    .to_string();

    let urls = DelegateTool::fetched_result_followup_urls(
        "Find recent official data records and results.",
        &fetch_payload,
        5,
    );

    assert_eq!(urls, vec!["https://example.com/data/records/"]);
}

#[test]
fn delegate_builds_structured_lookup_urls_from_policy_hosts() {
    let urls = DelegateTool::structured_lookup_urls(
        "请搜索柳叶刀最新治疗心脏病的论文，并给我 DOI、PubMed 或开放全文来源。",
    );

    assert!(urls
        .iter()
        .any(|url| url.contains("eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi")));
    assert!(urls
        .iter()
        .any(|url| url.contains("api.crossref.org/works")));
    assert!(urls
        .iter()
        .any(|url| url.contains("api.openalex.org/works")));
}

#[test]
fn delegate_builds_public_data_record_urls_for_lottery_tasks() {
    let urls = DelegateTool::structured_lookup_urls(
        "查找2个月内中国福利彩票的每期开奖号码，放进知识库，然后预测下一期的开奖号码。",
    );

    assert!(urls
        .iter()
        .any(|url| url.contains("cp.ip138.com/shuangseqiu")));
    assert!(urls
        .iter()
        .any(|url| url.contains("caipiao.eastmoney.com/pub/Result/History/ssq")));
    assert!(urls
        .iter()
        .any(|url| url.contains("kaijiang.500.com/index_fc.shtml")));
}

#[test]
fn delegate_rejects_data_page_shell_without_record_values() {
    let payload = serde_json::json!({
            "url": "https://www.cwl.gov.cn/ygkj/wqkjgg/ssq/",
            "content": "开奖公告 往期开奖公告 双色球 快乐8 福彩3D 七乐彩 期号 开奖日期 开奖号码 第至 注：期号格式为 2017041 开始查询",
            "content_quality": "actionable"
        })
        .to_string();

    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        "查找2个月内中国福利彩票的每期开奖号码。",
        &payload
    ));
}

#[test]
fn delegate_rejects_lottery_category_summary_without_number_rows() {
    let payload = serde_json::json!({
            "url": "https://cp.ip138.com/fucai/",
            "content": "双色球 第2026047期开奖结果 04-28 和值 跨度 区间比 单双比 蓝球 125 23 1:2:3 1:3 小 单 福彩3D 第2026109期开奖结果 04-29 佰位 拾位 个位 小 单 大 单 大 单",
            "content_quality": "actionable"
        })
        .to_string();

    assert!(!DelegateTool::fetched_result_looks_usable_for_task(
        "查找2个月内中国福利彩票的每期开奖号码。",
        &payload
    ));
}

#[test]
fn delegate_accepts_data_pages_with_multiple_record_values() {
    let payload = serde_json::json!({
            "url": "https://example.com/lottery/history",
            "content": "期号 开奖日期 开奖号码\n2026047 2026-04-28 02 06 09 18 25 31 + 08\n2026046 2026-04-26 01 07 13 21 27 33 + 12\n2026045 2026-04-23 03 11 16 19 22 30 + 05",
            "content_quality": "actionable"
        })
        .to_string();

    assert!(DelegateTool::fetched_result_looks_usable_for_task(
        "查找2个月内中国福利彩票的每期开奖号码。",
        &payload
    ));
}

#[test]
fn delegate_trims_structured_lookup_query_noise() {
    let query = DelegateTool::structured_lookup_query(
            "Search for the latest Lancet research papers regarding heart disease treatment, find titles summaries source links, save the results into the knowledge base, and report the final saved results in Chinese.",
        );

    let lowered = query.to_ascii_lowercase();
    assert!(lowered.contains("lancet"));
    assert!(lowered.contains("heart"));
    assert!(lowered.contains("treatment"));
    assert!(!lowered.contains("latest"));
    assert!(!lowered.contains("source"));
    assert!(!lowered.contains("links"));
    assert!(!lowered.contains("knowledge"));
    assert!(!lowered.contains("summaries"));
}

#[test]
fn delegate_adds_source_label_terms_for_structured_lookup() {
    let query = DelegateTool::structured_lookup_query("查找柳叶刀最近治疗心脏病论文。");
    let lowered = query.to_ascii_lowercase();

    assert!(lowered.contains("lancet"));
    assert!(!lowered.contains("latest"));
}

#[test]
fn delegate_cjk_lookup_query_preserves_subject_phrase_instead_of_ngrams() {
    let variants = DelegateTool::lookup_query_variants(
        "搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说，不能简单复制素材内容。",
    );
    let primary = variants.first().expect("query variant");

    assert!(primary.contains("玄幻小说"));
    assert!(primary.contains("免费") || primary.contains("下载"));
    assert!(!primary.contains("搜索一 索一部"));
    assert!(!primary.contains("玄幻小 幻小说"));
    assert!(!primary.contains("不能"));
    assert!(!primary.contains("全新"));
    assert!(!primary.contains("复制"));
}

#[test]
fn delegate_lookup_query_uses_original_request_before_delegated_workflow() {
    let task = "Original user request:\n搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说，不能简单复制素材内容，要求情节完善、角色名字不漂移、总长度超过50万字，并保存成txt文件。\n\nDelegated task:\nStep 1: Search for popular, downloadable fantasy (Xuanhuan) novels online.\nStep 2: Access and extract text/content/materials from the found novel.\nStep 3: Store these materials into the knowledge base.\nStep 4: Based on the materials, draft revise plan compose architect audit a long novel.";
    let variants = DelegateTool::lookup_query_variants(task);
    let joined = variants.join("\n");

    assert!(
        joined.contains("玄幻小说") || joined.contains("热门玄幻小说"),
        "{joined}"
    );
    assert!(
        joined.contains("下载") || joined.contains("免费"),
        "{joined}"
    );
    for forbidden in [
        "Step",
        "draft",
        "revise",
        "architect",
        "audit",
        "不能",
        "保存成",
    ] {
        assert!(
            !joined.contains(forbidden),
            "query variants should not include downstream workflow token `{forbidden}`: {joined}"
        );
    }
}

#[cfg(feature = "browser")]
#[test]
fn delegate_browser_lookup_query_prefers_original_cjk_source_intent() {
    let task = "Original user request:\n搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说。\n\nDelegated task:\nSearch for a popular downloadable fantasy Xuanhuan novel online. Access and extract text materials, then draft revise architect audit.";
    let variants = DelegateTool::browser_lookup_query_variants(task);
    let first = variants.first().expect("browser lookup variant");

    assert!(
        first.contains("玄幻小说") || first.contains("热门玄幻小说"),
        "{variants:?}"
    );
    assert!(
        first.contains("下载") || first.contains("免费"),
        "{variants:?}"
    );
    let joined = variants.join("\n");
    for forbidden in [
        "recall",
        "fetch",
        "draft",
        "revise",
        "architect",
        "audit",
        "载的热门",
    ] {
        assert!(
            !joined.contains(forbidden),
            "browser query variants should not include `{forbidden}`: {variants:?}"
        );
    }
}

#[test]
fn browser_recovery_query_uses_user_lookup_clause_not_blocker_metadata() {
    let task = "The prior lookup for this user task could not obtain enough verified observable evidence. Use an observation-capable worker to inspect sources and return observable item-level content or metadata according to the configured runtime policy. User task: 搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说，不能简单复制素材内容，要求情节完善、角色名字不漂移、总长度超过50万字，并保存成txt文件。\n\nPrevious blocker: browser search failed for 不能简单复制素材内容 并保存成txt文件 请你自己判断下一步并继续推进 novel fantasy download draft revise plan compose architect audit: Windows browser search failed.\nquery: 载的热门玄幻小说 下载";
    let variants = DelegateTool::lookup_query_variants(task);
    let primary = variants.first().expect("query variant");

    assert!(
        primary.contains("玄幻小说") || primary.contains("热门玄幻小说"),
        "unexpected primary query: {primary}; all variants: {variants:?}"
    );
    assert!(
        primary.contains("免费") || primary.contains("下载"),
        "unexpected primary query: {primary}; all variants: {variants:?}"
    );
    for forbidden in ["不能", "复制", "保存成", "draft", "audit", "载的热门"] {
        assert!(
            !primary.contains(forbidden),
            "browser query should not include downstream/blocker noise `{forbidden}`: {primary}"
        );
    }
}

#[test]
fn delegate_adds_recent_filters_to_structured_lookup_urls() {
    let urls = DelegateTool::structured_lookup_urls(
        "Search for recent Lancet papers about cardiovascular treatment.",
    );

    assert!(urls.iter().any(|url| {
        url.contains("eutils.ncbi.nlm.nih.gov")
            && url.contains("mindate=")
            && url.contains("datetype=pdat")
    }));
    assert!(urls
        .iter()
        .any(|url| url.contains("api.crossref.org") && url.contains("from-pub-date")));
    assert!(urls
        .iter()
        .any(|url| { url.contains("api.openalex.org") && url.contains("from_publication_date") }));
}

#[test]
fn delegate_builds_multiple_compact_structured_lookup_queries() {
    let queries = DelegateTool::structured_lookup_queries(
            "Search for recent Lancet papers regarding heart disease treatment. Focus on finding titles, abstracts, metadata, and key findings to populate the knowledge base.",
        );

    assert!(queries.len() >= 2);
    assert!(queries
        .iter()
        .any(|query| query.contains("lancet") && query.contains("heart")));
    assert!(!queries.iter().any(|query| {
        let lowered = query.to_ascii_lowercase();
        lowered.contains("focus")
            || lowered.contains("abstracts")
            || lowered.contains("metadata")
            || lowered.contains("populate")
    }));
}

#[test]
fn delegate_lookup_queries_drop_tool_surface_terms_from_worker_prompt() {
    let variants = DelegateTool::lookup_query_variants(
            "Use web_search to find recent 2024-2026 Lancet 柳叶刀 research papers or news regarding heart disease treatment 心脏病治疗. Provide paper knowledge_lookup lookup recall read_saved_knowledge knowledge_management update delete list web_research search fetch summarize_sources academic_paper find_papers fetch_abstract source_summary pdf_document parse extract_text summarize site:thelancet.com open access record 2025 2026 latest\n\n完整用户请求（必须保留查找之后的后续阶段，不能只完成查找片段）：查找柳叶刀关于最近的治疗心脏病的论文，然后存入知识库，根据知识库里心脏病论文的知识，自己写一个相关的治疗心脏病的论文，做成pdf。",
        );

    assert!(variants
        .iter()
        .any(|query| query.contains("lancet") && query.contains("heart")));
    assert!(!variants.iter().any(|query| {
        let lowered = query.to_ascii_lowercase();
        lowered.contains("knowledge_lookup")
            || lowered.contains("web_research")
            || lowered.contains("pdf_document")
            || lowered.contains("summarize_sources")
            || lowered.contains("extract_text")
            || lowered.contains("recall")
            || lowered.contains(" fetch ")
            || lowered.contains("code_repository")
            || lowered.contains("inspect_project")
    }));
}

#[test]
fn delegate_user_task_marker_discards_recovery_prompt_noise() {
    let variants = DelegateTool::lookup_query_variants(
            "The prior lookup for this user task could not obtain enough verified page evidence. Use browser search or browser page access to find item-level public metadata. Do not scrape full copyrighted text. User task: 查找柳叶刀关于最近的治疗心脏病的论文，然后存入知识库，根据知识库里心脏病论文的知识，自己写一个相关的治疗心脏病的论文，做成pdf。",
        );

    let joined = variants.join(" ").to_ascii_lowercase();
    assert!(joined.contains("lancet"));
    assert!(!joined.contains("prior"));
    assert!(!joined.contains("this task"));
    assert!(!joined.contains("browser page"));
    assert!(!joined.contains("copyrighted"));
}

#[test]
fn delegate_structured_discovery_can_seed_followup_without_text_alignment() {
    let payload = serde_json::json!({
            "url": "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&term=lancet+heart+disease+treatment&retmode=json",
            "content": r#"{"esearchresult":{"count":"2","idlist":["41936368","41800000"]}}"#,
            "content_quality": "actionable"
        })
        .to_string();

    assert!(DelegateTool::structured_discovery_result_can_seed_followup(
        "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi",
        &payload
    ));
    assert!(!DelegateTool::fetch_payload_matches_lookup_intent(
        "查找柳叶刀关于最近的治疗心脏病的论文",
        &payload
    ));
    assert!(DelegateTool::structured_lookup_followup_urls(&payload, 1)
        .iter()
        .any(|url| url.contains("esummary.fcgi")));
}

#[test]
fn delegate_pubmed_esearch_followup_prefers_batched_esummary() {
    let payload = serde_json::json!({
            "url": "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&term=lancet+heart+disease+treatment&retmode=json",
            "content": r#"{"esearchresult":{"count":"3","idlist":["41903557","42067280","42000000"]}}"#,
            "content_quality": "actionable"
        })
        .to_string();

    let urls = DelegateTool::structured_lookup_followup_urls(&payload, 3);
    assert!(urls
        .first()
        .map(|url| url.contains("esummary.fcgi") && url.contains("41903557%2C42067280%2C42000000"))
        .unwrap_or(false));
}

#[test]
fn delegate_pubmed_esummary_requires_record_topic_alignment() {
    let mismatch_content = serde_json::json!({
        "result": {
            "uids": ["41903557"],
            "41903557": {
                "uid": "41903557",
                "title": "Survival trends in patients with difficult-to-treat, antibiotic-resistant, Gram-negative infections in the era of next-generation antibiotics in the USA: a retrospective cohort study.",
                "fulljournalname": "The Lancet. Infectious diseases",
                "pubdate": "2026 Mar 25",
                "pubtype": ["Journal Article"],
                "articleids": [{"idtype": "doi", "value": "10.1016/S1473-3099(26)00020-4"}]
            }
        }
    });
    let mismatch_payload = serde_json::json!({
        "url": "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=41903557&retmode=json",
        "content": mismatch_content.to_string(),
        "content_quality": "actionable"
    })
    .to_string();

    assert!(!DelegateTool::fetch_payload_matches_lookup_intent(
        "Search for recent Lancet papers regarding heart disease treatments.",
        &mismatch_payload
    ));

    let aligned_content = serde_json::json!({
        "result": {
            "uids": ["42000000"],
            "42000000": {
                "uid": "42000000",
                "title": "Heart disease treatment outcomes after cardiac therapy in adults: a randomized clinical study.",
                "fulljournalname": "The Lancet",
                "pubdate": "2026 Apr",
                "pubtype": ["Journal Article"],
                "articleids": [{"idtype": "doi", "value": "10.1000/example"}]
            }
        }
    });
    let aligned_payload = serde_json::json!({
        "url": "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=42000000&retmode=json",
        "content": aligned_content.to_string(),
        "content_quality": "actionable"
    })
    .to_string();

    assert!(DelegateTool::fetch_payload_matches_lookup_intent(
        "Search for recent Lancet papers regarding heart disease treatments.",
        &aligned_payload
    ));
}

#[test]
fn delegate_structured_url_with_stable_id_counts_as_specific_academic_record() {
    assert!(DelegateTool::url_is_specific_academic_record(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=41936368&retmode=json"
        ));
    assert!(DelegateTool::url_is_specific_academic_record(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/efetch.fcgi?db=pubmed&id=41936368&retmode=xml"
        ));
    assert!(DelegateTool::url_is_specific_academic_record(
        "https://api.openalex.org/works/W2741809807"
    ));
    assert!(DelegateTool::url_is_specific_academic_record(
        "https://doi.org/10.1016/j.lanepe.2025.101384"
    ));
    assert!(!DelegateTool::url_is_specific_academic_record(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&term=lancet+heart+disease"
        ));
    assert!(!DelegateTool::url_is_specific_academic_record(
        "https://api.openalex.org/works?search=lancet+heart+disease"
    ));
}

#[test]
fn delegate_structured_queries_use_lookup_surface_not_schema_notice() {
    let queries = DelegateTool::structured_lookup_queries(
            "Use web_search to find recent Lancet heart disease treatment papers.\n\n完整用户请求（必须保留查找之后的后续阶段，不能只完成查找片段）：查找柳叶刀关于最近的治疗心脏病的论文，然后存入知识库，根据知识库里心脏病论文的知识，自己写一个相关的治疗心脏病的论文，做成pdf。\n\n---\n### NOTICE: First use of skill 'delegate'.\ninterface DelegateArgs { role: \"researcher\"; task: string; }",
        );

    assert!(queries
        .iter()
        .any(|query| query.contains("lancet") && query.contains("heart")));
    assert!(!queries.iter().any(|query| {
        let lowered = query.to_ascii_lowercase();
        lowered.contains("delegateargs") || lowered.contains("interface")
    }));
}

#[test]
fn delegate_prefers_quoted_structured_lookup_subject() {
    let query = DelegateTool::structured_lookup_query(
            "Search GitHub for high-starred 'agent-browser' related projects. Return 2 real source URLs and specify if web_search or browser was used.",
        );

    assert_eq!(query, "agent-browser");
}

#[test]
fn delegate_treats_video_source_inspection_as_lookup() {
    assert!(DelegateTool::task_requests_lookup(
            "检查 YouTube 上 agent browser 相关视频来源；如果只拿到页脚/低信息页面，请明确说不能作为有效来源。"
        ));
    assert!(DelegateTool::task_prefers_structured_sources(
        "检查 YouTube 上 agent browser 相关视频来源。"
    ));
}

#[test]
fn delegate_only_requires_structured_followup_for_academic_sources() {
    assert!(!DelegateTool::task_requires_structured_followup(
        "Search GitHub for high-starred 'agent-browser' related projects."
    ));
    assert!(DelegateTool::task_requires_structured_followup(
        "搜索柳叶刀最新治疗心脏病的论文，并给我 DOI、PubMed 或开放全文来源。"
    ));
}

#[test]
fn delegate_detects_preferred_academic_candidates() {
    let payload = serde_json::json!({
            "results": [
                {
                    "title": "PubMed record",
                    "url": "https://pubmed.ncbi.nlm.nih.gov/12345678/",
                    "snippet": "abstract"
                },
                {
                    "title": "Publisher fulltext",
                    "url": "https://www.thelancet.com/journals/lancet/article/PIIS0140-6736(25)01665-4/fulltext",
                    "snippet": "publisher page"
                }
            ]
        })
        .to_string();

    assert!(DelegateTool::search_output_has_preferred_academic_candidates(&payload));
}

#[test]
fn delegate_matches_structured_evidence_from_task_terms_without_fixed_domain_families() {
    let payload = serde_json::json!({
            "url": "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=41936368&retmode=json",
            "content": r#"{"result":{"uids":["41936368"],"41936368":{"source":"Lancet","pubdate":"2026 Apr 4","title":"Percutaneous coronary intervention versus coronary artery bypass grafting for unprotected left main stenosis: 10-year final results from the randomised, open-label, non-inferiority NOBLE trial."}}}"#,
            "content_quality": "actionable"
        })
        .to_string();

    assert!(DelegateTool::fetch_payload_matches_lookup_intent(
            "Find recent Lancet papers on heart disease treatment, including coronary artery disease and heart failure.",
            &payload
        ));
}

#[test]
fn delegate_rejects_structured_evidence_with_no_task_term_overlap() {
    let payload = serde_json::json!({
            "url": "https://api.example.test/records/1",
            "content": r#"{"title":"Quantum materials workshop schedule","summary":"A meeting agenda for condensed matter physics."}"#,
            "content_quality": "actionable"
        })
        .to_string();

    assert!(!DelegateTool::fetch_payload_matches_lookup_intent(
        "Find recent public papers about diabetes treatment trials.",
        &payload
    ));
}

#[test]
fn delegate_prefers_article_like_fetch_candidates_over_homepages() {
    let payload = serde_json::json!({
        "results": [
            {
                "title": "The Lancet | The best science for better lives",
                "url": "https://www.thelancet.com/",
                "snippet": "generic homepage"
            },
            {
                "title": "Lancet heart disease therapy study",
                "url": "https://pubmed.ncbi.nlm.nih.gov/12345678/",
                "snippet": "study abstract and doi"
            },
            {
                "title": "Online First - The Lancet",
                "url": "https://www.thelancet.com/journals/lancet/onlinefirst",
                "snippet": "latest articles published online first"
            }
        ]
    })
    .to_string();

    let candidates = DelegateTool::best_followup_fetch_urls(
        "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。",
        &payload,
        2,
    );

    assert_eq!(candidates.len(), 2);
    assert!(!candidates
        .iter()
        .any(|url| url == "https://www.thelancet.com/"));
    assert!(candidates
        .iter()
        .any(|url| url.contains("pubmed.ncbi.nlm.nih.gov") || url.contains("/onlinefirst")));
}

#[test]
fn delegate_recognizes_search_challenge_errors_as_blockers() {
    let error = anyhow::anyhow!("Browser search engine returned an anti-bot challenge page");
    let blocker = DelegateTool::summarize_lookup_blocker(&error);
    assert_eq!(
        blocker,
        Some("external search was blocked by an anti-bot or challenge page")
    );

    let error = anyhow::anyhow!("Edge/Chrome browser search returned no parsable results");
    let blocker = DelegateTool::summarize_lookup_blocker(&error);
    assert_eq!(
        blocker,
        Some("external search returned no reliable parsable results")
    );
}

#[test]
fn delegate_rejects_zero_result_structured_lookup_payloads() {
    let payload = serde_json::json!({
        "url": "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&term=test",
        "content": r#"{"esearchresult":{"count":"0","idlist":[]}}"#
    })
    .to_string();

    assert!(!DelegateTool::fetched_result_looks_usable(&payload));
}

#[test]
fn delegate_expands_pubmed_esearch_into_record_urls() {
    let payload = serde_json::json!({
        "url": "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&term=test",
        "content": r#"{"esearchresult":{"count":"2","idlist":["12345678","87654321"]}}"#
    })
    .to_string();

    let urls = DelegateTool::structured_lookup_followup_urls(&payload, 2);
    assert_eq!(
            urls,
            vec![
                "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=12345678&retmode=json".to_string(),
                "https://pubmed.ncbi.nlm.nih.gov/12345678/".to_string(),
                "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=87654321&retmode=json".to_string(),
                "https://pubmed.ncbi.nlm.nih.gov/87654321/".to_string()
            ]
        );
}

#[test]
fn delegate_prefers_crossref_record_api_before_publisher_pages() {
    let payload = serde_json::json!({
            "url": "https://api.crossref.org/works?query=test",
            "content": r#"{
              "message": {
                "items": [
                  {
                    "DOI": "10.1016/S0140-6736(26)00001-0",
                    "URL": "https://www.thelancet.com/journals/lancet/article/PIIS0140-6736(26)00001-0/fulltext"
                  }
                ]
              }
            }"#
        })
        .to_string();

    let urls = DelegateTool::structured_lookup_followup_urls(&payload, 1);
    assert_eq!(
        urls.first().map(String::as_str),
        Some("https://api.crossref.org/works/10.1016%2FS0140-6736%2826%2900001-0")
    );
    assert!(urls.iter().any(|url| url.contains("thelancet.com")));
}

#[test]
fn delegate_prefers_openalex_record_api_before_landing_page() {
    let payload = serde_json::json!({
        "url": "https://api.openalex.org/works?search=test",
        "content": r#"{
              "results": [
                {
                  "id": "https://openalex.org/W1234567890",
                  "primary_location": {
                    "landing_page_url": "https://example.com/paper",
                    "pdf_url": "https://example.com/paper.pdf"
                  },
                  "doi": "https://doi.org/10.1000/example"
                }
              ]
            }"#
    })
    .to_string();

    let urls = DelegateTool::structured_lookup_followup_urls(&payload, 1);
    assert_eq!(
        urls.first().map(String::as_str),
        Some("https://api.openalex.org/works/W1234567890")
    );
    assert!(urls.iter().any(|url| url == "https://example.com/paper"));
}

#[test]
fn delegate_marks_structured_search_endpoints_as_discovery_only() {
    assert!(DelegateTool::is_structured_discovery_url(
        "https://api.crossref.org/works?query=test"
    ));
    assert!(DelegateTool::is_structured_discovery_url(
        "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&term=test"
    ));
    assert!(!DelegateTool::is_structured_discovery_url(
        "https://pubmed.ncbi.nlm.nih.gov/12345678/"
    ));
}

#[test]
fn delegate_prefers_structured_sources_when_policy_provides_them() {
    assert!(DelegateTool::task_prefers_structured_sources(
        "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库"
    ));
    assert!(DelegateTool::task_prefers_structured_sources(
        "Find GitHub repositories about Rust browser automation and summarize them"
    ));
    assert!(!DelegateTool::task_prefers_structured_sources(
        "Search the public web for the best coffee shops in Shanghai"
    ));
}

#[test]
fn delegate_extracts_explicit_terminal_command_without_backticks() {
    assert_eq!(
        DelegateTool::extract_terminal_command("rg \"needle\" README.md").as_deref(),
        Some("rg \"needle\" README.md")
    );
    assert_eq!(
        DelegateTool::extract_terminal_command("请执行命令 rg \"needle\" README.md").as_deref(),
        Some("rg \"needle\" README.md")
    );
    assert!(DelegateTool::extract_terminal_command("请解释这段话").is_none());
}

#[test]
fn delegate_formats_command_no_match_as_completed_outcome() {
    let output = serde_json::json!({
        "runtime": "bash",
        "working_dir": "/tmp/project",
        "status": 1,
        "raw_status_success": false,
        "success": true,
        "outcome_kind": "no_match",
        "outcome_summary": "no matches found for \"needle\"",
        "stdout": "",
        "stderr": "",
        "evidence_artifacts": []
    })
    .to_string();
    let formatted = DelegateTool::format_command_exec_result(&output);
    assert!(formatted.contains("status: completed"));
    assert!(formatted.contains("outcome: no_match"));
    assert!(formatted.contains("raw_exit_status: 1"));
    assert!(formatted.contains("blockers: none"));
}

#[test]
fn delegate_formats_command_artifact_before_short_preview() {
    let long_stdout = "line\n".repeat(2000);
    let output = serde_json::json!({
        "runtime": "bash",
        "working_dir": "/tmp/project",
        "status": 0,
        "raw_status_success": true,
        "success": true,
        "outcome_kind": "success",
        "outcome_summary": "command completed successfully",
        "stdout": long_stdout,
        "stderr": "",
        "evidence_artifacts": [{
            "uri": "/tmp/project/.benshu/tool-output/example/stdout.txt"
        }]
    })
    .to_string();
    let formatted = DelegateTool::format_command_exec_result(&output);
    let artifact_index = formatted.find("evidence_artifacts:").unwrap();
    let stdout_index = formatted.find("stdout_preview:").unwrap();
    assert!(artifact_index < stdout_index);
    assert!(formatted.contains("stdout/stderr preview truncated"));
    assert!(formatted.len() < 1800);
}

#[test]
fn delegate_routes_local_continuation_away_from_researcher() {
    let task =
        "读取 data/generated/longform/agent-artifact-1.txt，继续续写至少50章节并保存成txt文档";

    assert!(DelegateTool::task_requests_local_file_continuation(task));
    assert!(DelegateTool::should_route_local_continuation_to_writer(
        "researcher",
        task
    ));
    assert!(!DelegateTool::should_route_local_continuation_to_writer(
        "knowledge",
        task
    ));
}

#[test]
fn delegate_recognizes_bare_txt_save_requests_as_file_writes() {
    let task = "请写一篇中文论文，约2000字，保存为txt。";

    assert!(DelegateTool::task_requests_file_write(task));
}

#[test]
fn delegate_recognizes_text_file_save_requests_as_file_writes() {
    let task = "Write a report and save it as a text file.";

    assert!(DelegateTool::task_requests_file_write(task));
}

#[test]
fn local_writing_continuation_with_project_ref_does_not_require_external_acquisition() {
    let task = "Original user request:\n继续写下一章，沿用刚才的世界观和真相文件。\n\nDelegated task:\nContinue the existing artifact.\n\nExisting artifact/work-in-progress context:\n- project_path: /home/user/benshu/data/generated/novels/example-project\n";

    assert_eq!(
        DelegateTool::extract_existing_artifact_project_path(task).as_deref(),
        Some("/home/user/benshu/data/generated/novels/example-project")
    );
    assert!(!DelegateTool::task_requires_external_acquisition_before_artifact(task));
    assert_eq!(DelegateTool::requested_chapter_count(task), 1);
}

#[test]
fn delegate_does_not_rewrite_explicit_researcher_to_writer() {
    let task = "Search for a science fiction interstellar novel that can be used as a knowledge base foundation for a 500,000-word novel with deep worldbuilding and complex plotlines.";

    assert!(DelegateTool::should_route_local_continuation_to_writer(
        "researcher",
        task
    ));
    assert!(
        !DelegateTool::should_rewrite_requested_role_for_local_continuation(
            false,
            "researcher",
            task
        )
    );
    assert!(
        DelegateTool::should_rewrite_requested_role_for_local_continuation(
            true,
            "researcher",
            task
        )
    );
}

#[test]
fn delegate_existing_artifact_revision_does_not_enter_longform_continuation() {
    let task =
        "请继续处理第二章，按照检查结果修订它，补全摘要、关键事实和连续性更新，并更新本地项目文件";

    assert!(DelegateTool::task_requests_existing_artifact_revision(task));
    assert!(!DelegateTool::task_requests_local_file_continuation(task));
}

#[test]
fn delegate_read_file_fast_path_ignores_directory_paths() {
    let cwd = std::env::current_dir().expect("current dir should be available");
    let tempdir = tempfile::Builder::new()
        .prefix("delegate-read-dir-")
        .tempdir_in(cwd)
        .expect("tempdir should be created inside workspace");
    let task = format!(
        "Use `read_file` to inspect project files under {} before continuing.",
        tempdir.path().display()
    );

    let result = DelegateTool::read_local_file_for_delegate(&task, "writer")
        .expect("directory metadata should be readable");

    assert!(result.is_none());
}

#[test]
fn delegate_workspace_boundary_check_allows_missing_child_inside_workspace() {
    let cwd = std::env::current_dir().expect("current dir should be available");
    let candidate = cwd.join("target").join("missing-delegate-read-file.md");

    assert!(DelegateTool::path_is_inside(&cwd, &candidate));
}

#[test]
fn delegate_workspace_boundary_blocker_includes_workspace_root() {
    let cwd = std::env::current_dir().expect("current dir should be available");
    let outside = cwd
        .parent()
        .expect("repo should have a parent")
        .join(".benshu")
        .join("project.md");
    let blocker = DelegateTool::workspace_boundary_blocker("writer", "read_file", &outside, &cwd);

    assert!(blocker.contains("status: blocked"));
    assert!(blocker.contains("workspace_root:"));
    assert!(blocker.contains(&cwd.display().to_string()));
    assert!(blocker.contains("do not infer hidden sibling directories"));
}

#[test]
fn delegate_routes_local_writing_context_lookup_to_writer() {
    let task =
        "请在搜索历史和知识库中检索当前作品第二章的人物名单、世界观设定、关键事实和连续性记录";

    assert!(DelegateTool::task_requests_local_writing_context(task));
    assert!(DelegateTool::should_route_local_continuation_to_writer(
        "researcher",
        task
    ));
}

#[test]
fn delegate_keeps_explicit_web_lookup_on_researcher() {
    let task = "请用浏览器搜索公网资料，整理这个作品的相关网页证据";

    assert!(!DelegateTool::task_requests_local_writing_context(task));
    assert!(!DelegateTool::should_route_local_continuation_to_writer(
        "researcher",
        task
    ));
}

#[test]
fn writing_equipment_key_expands_to_writer_loop_tools() {
    let expanded = DelegateTool::expanded_worker_tool_names(&["writing".to_string()]);

    for expected in [
        "read_file",
        "write_file",
        "knowledge_search",
        "fetch_document",
        "knowledge_manage_document",
        "writing_studio",
        "novel_studio",
    ] {
        assert!(
            expanded.iter().any(|tool| tool == expected),
            "missing expanded writing tool: {expected}; expanded={expanded:?}"
        );
    }
    assert!(
        !expanded.iter().any(|tool| tool == "search_history"),
        "writer package should not expose personal chat-memory recall as a default writing source"
    );
}

#[test]
fn delegate_longform_steps_scale_with_requested_target_size() {
    assert_eq!(
        DelegateTool::requested_chapter_count("生成50万字并保存成txt文档"),
        278
    );
    assert_eq!(
        DelegateTool::requested_chapter_count("write a 500,000-word novel"),
        278
    );
    assert_eq!(
        DelegateTool::requested_chapter_count_with_step_target("生成50万字并保存成txt文档", 8_000),
        63
    );
    assert_eq!(
        DelegateTool::requested_text_target_chars("生成50万字并保存成txt文档"),
        Some(500_000)
    );
    assert_eq!(
        DelegateTool::requested_text_target_chars("write a 500,000-word novel"),
        Some(500_000)
    );
    assert_eq!(
        DelegateTool::requested_text_target_chars("生成五十万字并保存成txt文档"),
        Some(500_000)
    );
    assert_eq!(
        DelegateTool::requested_text_target_chars("生成100万字并保存成txt文档"),
        Some(1_000_000)
    );
    assert_eq!(
        DelegateTool::requested_text_target_chars("生成一百万字并保存成txt文档"),
        Some(1_000_000)
    );
    assert_eq!(
        DelegateTool::requested_text_target_chars("生成一百二十万字并保存成txt文档"),
        Some(1_200_000)
    );
    assert_eq!(
        DelegateTool::requested_text_target_chars("生成两百万字并保存成txt文档"),
        Some(2_000_000)
    );
    assert_eq!(
        DelegateTool::requested_text_target_chars("生成800万字并保存成txt文档"),
        Some(8_000_000)
    );
    assert_eq!(
        DelegateTool::requested_text_target_chars("write a 2.5m-word novel"),
        Some(2_500_000)
    );
}

#[test]
fn delegate_longform_unspecified_size_uses_bounded_default_checkpoints() {
    let count = DelegateTool::requested_chapter_count("请写一部完整的原创玄幻小说并保存成txt文档");

    assert_eq!(
        count,
        DelegateTool::default_unspecified_longform_checkpoints()
    );
    assert_eq!(count, 1);
    assert!(count < 50);
}

#[test]
fn delegate_longform_first_chapter_scope_runs_one_checkpoint() {
    assert_eq!(
        DelegateTool::requested_chapter_count("请先完成故事设定，并写第一章，正文保存成文件"),
        1
    );
    assert_eq!(
        DelegateTool::requested_chapter_count(
            "Create the setting and write the first chapter only."
        ),
        1
    );
}

#[test]
fn delegate_longform_distinguishes_chapter_ordinal_from_quantity() {
    assert_eq!(DelegateTool::requested_chapter_count("继续写第2章"), 1);
    assert_eq!(
        DelegateTool::requested_start_chapter("继续写第2章"),
        Some(2)
    );
    assert_eq!(
        DelegateTool::requested_chapter_count("继续写第7章到第10章"),
        4
    );
    assert_eq!(
        DelegateTool::requested_start_chapter("继续写第7章到第10章"),
        Some(7)
    );
    assert_eq!(DelegateTool::requested_chapter_count("请先写10章"), 10);
    assert_eq!(DelegateTool::requested_start_chapter("请先写10章"), None);
}

#[test]
fn delegate_longform_readback_does_not_enter_writer_driver() {
    let task = "Original user request:\n总结一下前10章内容，并告诉我第三章讲了什么、主角是谁。\n\nDelegated task:\nContinue the same writing task.\n\nExisting artifact/work-in-progress context:\n- project_path: /home/user/benshu/data/generated/novels/example\n";

    assert!(DelegateTool::task_requests_governed_fiction_project(task));
    assert!(!DelegateTool::should_route_writer_fiction_to_novel_studio(
        &["novel_studio".to_string()],
        task
    ));
}

#[test]
fn delegate_longform_path_selection_ignores_tool_error_paths_for_new_artifacts() {
    let task = "Execute this routed user task as the specialist.\n\
        Original user request: 请写一部完整的原创仙侠小说，目标50万字，并保存成 txt 文档。\n\
        [Tool Result: novel_studio] {\"path\":\"/home/user/data/generated/novels/旧标题\",\"error\":\"title conflict\"}";

    let path = DelegateTool::select_longform_artifact_path(task).expect("path");

    assert!(
        path.starts_with("data/generated/tasks/"),
        "new artifact should not reuse incidental tool error path: {path}"
    );
    assert!(
        path.ends_with(".txt"),
        "new longform text artifact should remain a txt path: {path}"
    );
}

#[test]
fn longform_public_artifact_excludes_checkpoint_tail() {
    let output = "### 第一章\n\n正文继续。\n\n连续性记录：只给运行时。\n\n下一步钩子：只给下一步。";

    let public = DelegateTool::longform_public_artifact_output(output);

    assert_eq!(public, "### 第一章\n\n正文继续。");
    assert!(!public.contains("连续性记录"));
    assert!(!public.contains("下一步钩子"));
}

#[test]
fn longform_completed_receipt_declares_requested_format_effect() {
    assert_eq!(
        DelegateTool::artifact_format_runtime_effect_line("/tmp/final.txt"),
        "\nruntime_effect: artifact.txt"
    );
    assert_eq!(
        DelegateTool::artifact_format_runtime_effect_line("/tmp/final.md"),
        "\nruntime_effect: artifact.md"
    );
}

#[test]
fn delegate_longform_retry_prompt_shrinks_after_timeout_or_truncation() {
    assert!(DelegateTool::previous_error_requests_smaller_step(
        "step 2 attempt 1 exceeded its 300s execution budget"
    ));
    assert!(DelegateTool::previous_error_requests_smaller_step(
        "longform artifact step 2 returned too little body content (3 chars, minimum 240); output is likely truncated"
    ));
    assert!(DelegateTool::previous_error_requests_smaller_step(
        "longform artifact step 1 is missing continuity note or next hook; output is likely truncated"
    ));

    let prompt = DelegateContinuousActionHandler::build_step_prompt(
        "继续生成长文档",
        &ContinuousStepRequest {
            task_id: uuid::Uuid::nil(),
            objective: "生成长文档并保存".to_string(),
            worker_role: "writer".to_string(),
            step: ContinuousTaskStep {
                index: 2,
                label: "chapter-draft-2".to_string(),
                instruction: "Continue the longform artifact.".to_string(),
                expected_output: Some("One bounded chunk.".to_string()),
                depends_on: vec![1],
                action: ContinuousStepAction::Model {
                    prompt: "继续正文".to_string(),
                },
            },
            previous_summary: Some("上一章摘要".to_string()),
            recent_checkpoint_summaries: vec!["上一章钩子".to_string()],
            attempt: 1,
            previous_error: Some("step 2 attempt 1 exceeded its 300s execution budget".to_string()),
            contract: None,
        },
    );

    assert!(prompt.contains("恢复型微步骤"));
    assert!(prompt.contains("约 600 到 1000 个中文字符"));
    assert!(prompt.contains("不能只输出标题、目录、元信息、计划、摘要、错误说明或执行器状态"));
    assert!(prompt.contains("最后两个非空段落必须分别以“连续性记录：”和“下一步钩子：”开头"));
}

#[test]
fn delegate_longform_retry_prompt_guides_fresh_title_after_reuse() {
    let prompt = DelegateContinuousActionHandler::build_step_prompt(
        "继续生成长文档",
        &ContinuousStepRequest {
            task_id: uuid::Uuid::nil(),
            objective: "生成长文档并保存".to_string(),
            worker_role: "writer".to_string(),
            step: ContinuousTaskStep {
                index: 1,
                label: "artifact-identity-and-chapter-1".to_string(),
                instruction: "Establish identity and write the first chunk.".to_string(),
                expected_output: Some("Identity plus first bounded chunk.".to_string()),
                depends_on: vec![],
                action: ContinuousStepAction::Model {
                    prompt: "开始正文".to_string(),
                },
            },
            previous_summary: None,
            recent_checkpoint_summaries: Vec::new(),
            attempt: 1,
            previous_error: Some(
                "longform artifact title '万劫归墟' was already used by a prior generated artifact and was not explicitly requested"
                    .to_string(),
            ),
            contract: None,
        },
    );

    assert!(prompt.contains("必须自行创造一个全新的标题"));
    assert!(prompt.contains("禁止再次使用标题“万劫归墟”"));
    assert!(prompt.contains("不要复述被拒绝的标题"));
}

#[test]
fn delegate_empty_continuous_step_recovery_prompt_requires_checkpointable_output() {
    let prompt = DelegateContinuousActionHandler::build_empty_step_recovery_prompt(
        "继续生成长文档",
        &ContinuousStepRequest {
            task_id: uuid::Uuid::nil(),
            objective: "生成长文档并保存".to_string(),
            worker_role: "writer".to_string(),
            step: ContinuousTaskStep {
                index: 2,
                label: "chapter-draft-2".to_string(),
                instruction: "Continue the longform artifact.".to_string(),
                expected_output: Some("One bounded chunk.".to_string()),
                depends_on: vec![1],
                action: ContinuousStepAction::Model {
                    prompt: "继续正文".to_string(),
                },
            },
            previous_summary: Some("上一章摘要".to_string()),
            recent_checkpoint_summaries: vec!["上一章钩子".to_string()],
            attempt: 2,
            previous_error: None,
            contract: Some(ContinuousTaskContract {
                invariants: Vec::new(),
                anchors: vec![ContinuousTaskAnchor {
                    name: "planned_total_steps".to_string(),
                    value: "200".to_string(),
                }],
                completion_criteria: Vec::new(),
                required_events: Vec::new(),
                completion_event: None,
            }),
        },
    );

    assert!(prompt.contains("恢复型输出"));
    assert!(
        prompt.contains("不能返回空文本、标题-only、元信息-only、计划-only、道歉或无法完成说明")
    );
    assert!(prompt.contains("连续性记录"));
    assert!(prompt.contains("下一步钩子"));
    assert!(!prompt.contains("blocker 说明原因"));
}

#[test]
fn delegate_continuous_step_token_budget_follows_step_size_and_retry_state() {
    let base_request = ContinuousStepRequest {
        task_id: uuid::Uuid::nil(),
        objective: "生成长文档并保存".to_string(),
        worker_role: "writer".to_string(),
        step: ContinuousTaskStep {
            index: 2,
            label: "chapter-draft-2".to_string(),
            instruction: "Continue the longform artifact.".to_string(),
            expected_output: Some("One bounded chunk.".to_string()),
            depends_on: vec![1],
            action: ContinuousStepAction::Model {
                prompt: "继续正文".to_string(),
            },
        },
        previous_summary: Some("上一章摘要".to_string()),
        recent_checkpoint_summaries: vec!["上一章钩子".to_string()],
        attempt: 0,
        previous_error: None,
        contract: Some(ContinuousTaskContract {
            invariants: Vec::new(),
            anchors: vec![ContinuousTaskAnchor {
                name: "step_target_chars".to_string(),
                value: "1800".to_string(),
            }],
            completion_criteria: Vec::new(),
            required_events: Vec::new(),
            completion_event: None,
        }),
    };

    assert_eq!(
        DelegateContinuousActionHandler::continuous_step_output_token_budget(&base_request),
        2_620
    );

    let mut retry_request = base_request.clone();
    retry_request.attempt = 1;
    retry_request.previous_error =
        Some("step 2 attempt 1 exceeded its 300s execution budget".to_string());
    assert_eq!(
        DelegateContinuousActionHandler::continuous_step_output_token_budget(&retry_request),
        1_200
    );

    let mut first_request = base_request;
    first_request.step.index = 1;
    first_request.step.label = "artifact-identity-and-chapter-1".to_string();
    assert_eq!(
        DelegateContinuousActionHandler::continuous_step_output_token_budget(&first_request),
        3_100
    );
}

#[test]
fn delegate_large_text_artifact_uses_checkpointed_continuation_without_explicit_path() {
    let task = "根据知识库内容推理后写100万字文本文件并保存成txt文档";

    assert!(DelegateTool::task_requests_file_write(task));
    assert!(DelegateTool::task_requests_checkpointed_text_artifact(task));
    assert!(DelegateTool::task_requests_local_file_continuation(task));

    let path = DelegateTool::default_generated_artifact_path(task)
        .expect("large saved text artifact should receive a generated path");
    assert!(path.starts_with("data/generated/tasks/"));
    assert!(path.ends_with("/agent-artifact-1.txt"));
}

#[test]
fn delegate_small_text_artifact_does_not_enter_checkpointed_longform() {
    let task = "请写一篇不超过300字的原创微型言情故事，并保存成 txt 文档。";

    assert!(DelegateTool::task_requests_file_write(task));
    assert!(!DelegateTool::task_requests_checkpointed_text_artifact(
        task
    ));
    assert!(!DelegateTool::task_requests_local_file_continuation(task));
    assert!(DelegateTool::extract_write_target_path(task).is_none());

    let path = DelegateTool::default_generated_artifact_path(task)
        .expect("saved text artifact should receive a generated path");
    assert!(path.starts_with("data/generated/tasks/"));
    assert!(path.ends_with("/agent-artifact-1.txt"));
}

#[test]
fn delegate_bare_file_extensions_are_not_treated_as_paths() {
    assert!(DelegateTool::looks_like_bare_file_extension(".txt"));
    assert!(DelegateTool::looks_like_bare_file_extension("txt"));
    assert!(DelegateTool::extract_write_target_path("保存成 .txt 文件").is_none());
    assert!(DelegateTool::extract_write_target_path("保存成 story.txt 文件").is_some());
}

#[test]
fn delegate_quality_contract_respects_small_requested_text_budget() {
    let contract =
        DelegateTool::artifact_quality_contract("请写一篇不超过300字的短文并保存成txt文档");

    assert_eq!(
        DelegateTool::requested_text_max_chars("不超过300字"),
        Some(300)
    );
    assert_eq!(
        DelegateTool::requested_text_max_chars(
            "previous attempt was too long (702 characters instead of the requested <300). Write under 300 characters."
        ),
        Some(300)
    );
    assert_eq!(
        DelegateTool::requested_text_max_chars("请控制在五百字以内完成"),
        Some(500)
    );
    assert_eq!(contract.max_chars, Some(300));
    assert!(
        contract.min_chars <= 300,
        "small explicit writing budgets should not be expanded into long artifacts"
    );
}

#[test]
fn delegate_quality_contract_rejects_explicit_length_overrun() {
    let task = "请写一篇不超过300字的短文并保存成txt文档";
    let contract = DelegateTool::artifact_quality_contract(task);
    let content = format!("# 标题\n\n正文\n\n{}", "长".repeat(350));
    let report = DelegateTool::artifact_quality_report_with_contract(task, &content, &contract);

    assert!(!report.passed);
    assert!(report
        .repairable
        .iter()
        .any(|issue| issue.contains("content_depth_above_maximum")));
}

#[test]
fn delegate_revision_prompt_respects_explicit_length_ceiling() {
    let task = "请写一篇不超过300字的短文并保存成txt文档";
    let contract = DelegateTool::artifact_quality_contract(task);
    let report = DelegateTool::artifact_quality_report_with_contract(
        task,
        &format!("# 标题\n\n正文\n{}", "长".repeat(350)),
        &contract,
    );
    let prompt = DelegateTool::build_delegated_file_artifact_revision_prompt(
        task,
        "data/generated/tasks/test/agent-artifact-1.txt",
        "上一版",
        &report,
        1,
    );

    assert!(prompt.contains("不超过 300 个字符"));
    assert!(!prompt.contains("必须包含“自检与修订记录”"));
}

#[test]
fn delegate_fast_path_attempt_budget_is_kept_for_supervised_single_steps() {
    assert!(DelegateTool::fast_path_uses_attempt_budget(false, false));
    assert!(DelegateTool::fast_path_uses_attempt_budget(true, false));
    assert!(!DelegateTool::fast_path_uses_attempt_budget(false, true));
    assert!(!DelegateTool::fast_path_uses_attempt_budget(true, true));
}

#[test]
fn delegate_fast_path_budget_is_policy_configurable() {
    let policy = serde_json::json!({
        "handles": [{
            "triggers": ["public records"],
            "direct_execution_budget_secs": 45
        }]
    });
    assert_eq!(
        SearchPolicy::policy_u64_value_for_task(
            &policy,
            "Find public records",
            "find public records",
            &[
                "delegate_fast_path_budget_secs",
                "worker_direct_execution_budget_secs",
                "direct_execution_budget_secs"
            ],
        ),
        Some(45)
    );
    assert!(!DelegateTool::fast_path_uses_attempt_budget(false, true));
}

#[test]
fn delegate_file_artifact_fast_path_budget_scales_with_requested_units() {
    let task = "请写一篇中文论文，约2000字，保存为txt。";

    assert_eq!(DelegateTool::requested_text_target_chars(task), Some(2000));
    assert!(DelegateTool::task_requests_file_write(task));
    assert!(DelegateTool::delegate_fast_path_budget_secs_for_task(task) >= 300);
}

#[test]
fn delegate_file_artifact_fast_path_budget_has_default_floor() {
    let task = "Write a short report and save it as a text file.";

    assert!(DelegateTool::task_requests_file_write(task));
    assert!(DelegateTool::delegate_fast_path_budget_secs_for_task(task) >= 180);
}

#[test]
fn delegated_artifact_decisions_use_original_request_not_wrapper_vocabulary() {
    let task = r#"Create or continue the requested local file artifact and save it as a text document.
For substantial artifacts such as papers, reports, longform documents, or evidence-driven PDF output, satisfy the quality contract.
Write the artifact at `data/generated/tasks/test/agent-artifact-1.txt`.
Original user request: 搜索公开作品前10部，存入知识库，推理后写一篇玄幻小说，尝试写100万字的任务

Verified researcher evidence:
status: completed
worker: researcher
executed_tool: browser_browse
result_summary:
- 1. 样本A | public metadata: title only | source: https://example.com/a

Knowledge import receipt:
status: completed
worker: knowledge
executed_tool: knowledge_import_url"#;

    assert_eq!(
        DelegateTool::artifact_intent_surface(task),
        "搜索公开作品前10部，存入知识库，推理后写一篇玄幻小说，尝试写100万字的任务"
    );
    assert!(DelegateTool::task_requests_checkpointed_text_artifact(task));

    let coordinator = Arc::new(Coordinator::new());
    let contract = DelegateTool::artifact_quality_contract_for_coordinator(&coordinator, task);
    assert_eq!(contract.artifact_type, "longform_document");
}

#[test]
fn delegate_longform_continuation_generates_requested_chapter_count() {
    let task = "继续刚才那部本地长篇，读取 data/generated/longform/agent-artifact-1.txt，至少完成50章节，保存成txt文档";
    let artifact = DelegateTool::build_longform_continuation_artifact(
        task,
        "# BenShu 长文本生成起稿\n\n书名：《测试长篇》\n主角：测试主角\n",
    );

    assert!(!artifact.contains("## 第二阶段：连续正文草稿"));
    assert!(artifact.contains("测试主角"));
    assert!(!artifact.contains("## 产物身份要求"));
    assert!(!artifact.contains("自行命名"));
    assert!(artifact.contains("不能使用代码内置剧情模板"));
    assert!(artifact.matches("### 第").count() >= 50);
    assert!(!artifact.contains("不应出现的固定示例标题"));
}

#[test]
fn delegate_longform_continuation_is_idempotent_for_previous_batches() {
    let existing = "# BenShu 长文本生成起稿\n\n书名：《测试长篇》\n主角：测试主角\n\n---\n\n# BenShu 长篇续写批次\n\n旧批次内容\n### 第二章 旧章";
    let stripped = DelegateTool::strip_previous_longform_continuation(existing);

    assert!(stripped.contains("书名：《测试长篇》"));
    assert!(!stripped.contains("旧批次内容"));
    assert!(!stripped.contains("### 第二章 旧章"));
}

#[test]
fn delegate_longform_reports_failed_continuous_run_as_failed() {
    let status = ContinuousTaskStatus::Failed {
        reason: "step 4 attempted to rename title".to_string(),
    };

    assert_eq!(
        DelegateTool::continuous_run_result_status(&status),
        "failed"
    );
    assert_eq!(
        DelegateTool::continuous_run_blockers(&status),
        Some("step 4 attempted to rename title")
    );
}

#[test]
fn delegate_longform_reports_paused_continuous_run_as_paused() {
    let status = ContinuousTaskStatus::Paused {
        reason: "model provider service disconnected".to_string(),
    };

    assert_eq!(
        DelegateTool::continuous_run_result_status(&status),
        "paused"
    );
    assert_eq!(
        DelegateTool::continuous_run_blockers(&status),
        Some("model provider service disconnected")
    );
}

#[test]
fn delegate_longform_reports_completed_continuous_run_without_blocker() {
    let status = ContinuousTaskStatus::Completed;

    assert_eq!(
        DelegateTool::continuous_run_result_status(&status),
        "completed"
    );
    assert_eq!(DelegateTool::continuous_run_blockers(&status), None);
}

#[test]
fn delegate_file_artifact_blocks_when_ranked_evidence_is_under_requested_count() {
    let task = r#"Original user request:
搜索某站前10部公开作品，写入知识库，然后基于这些样本写一个txt。

Verified researcher evidence:
status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/rank
result_summary:
- 1. 样本A | public metadata: 类型A | source: https://example.com/rank
- 2. 样本B | public metadata: metadata not visible in fetched source | source: https://example.com/rank

Knowledge import receipt:
status: completed
worker: knowledge
executed_tool: knowledge_import_url"#;

    let blocker = DelegateTool::evidence_quality_blocker_for_file_artifact(task)
        .expect("under-collected ranked evidence should block source-derived artifacts");

    assert!(blocker.contains("blocker_contract: goal_not_satisfied"));
    assert!(blocker.contains("observed_item_records: 1"));
    assert!(blocker.contains("requested_item_records: 10"));
    assert!(blocker.contains("next_action_policy: infer_from_original_goal_and_observed_evidence"));
}

#[test]
fn delegate_file_artifact_guard_ignores_runtime_receipt_collection_word() {
    let task = r#"Create the requested local file artifact and save it as a PDF document.

Original user request:
查找柳叶刀关于最近的治疗心脏病的论文，然后存入知识库，根据知识库里心脏病论文的知识，自己写一个相关的治疗心脏病的论文，做成pdf。

Verified researcher evidence:
status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=42067280&retmode=json

Knowledge import receipt:
status: completed
worker: knowledge
executed_tool: knowledge_import_url
result:
Imported web knowledge into collection 'references' at path 'web/eutils-ncbi-nlm-nih-gov/header-30745b6ab1e0f08d'."#;

    assert!(
        DelegateTool::evidence_quality_blocker_for_file_artifact(task).is_none(),
        "runtime receipt vocabulary must not change the original user collection intent"
    );
}

#[test]
fn delegate_file_artifact_guard_uses_original_goal_for_requested_count() {
    let task = r#"Create the requested local file artifact and save it as a txt document.

Original user request:
搜索某站前10部公开作品，写入知识库，然后基于这些样本写一个txt。

Verified researcher evidence:
status: completed
worker: researcher
executed_tool: web_fetch
source_url: https://example.com/rank
result_summary:
- 1. 样本A | public metadata: 类型A | source: https://example.com/rank

Knowledge import receipt:
Imported web knowledge into collection 'references' at path 'web/example/rank'."#;

    let blocker = DelegateTool::evidence_quality_blocker_for_file_artifact(task)
        .expect("original collection goal should still be guarded");

    assert!(blocker.contains("observed_item_records: 1"));
    assert!(blocker.contains("requested_item_records: 10"));
}

#[test]
fn delegated_research_paper_quality_contract_rejects_thin_pdf_body() {
    let task = "artifact_type: research_paper\n请根据知识库写一篇治疗心脏病的论文，做成 PDF。";
    let content = "标题：治疗心脏病\n\n这是一篇很短的说明。\n\n参考文献\n[1] PubMed.";
    let contract = DelegateTool::artifact_quality_contract(task);

    let report = DelegateTool::artifact_quality_report_with_contract(task, content, &contract);

    assert_eq!(report.artifact_type, "research_paper");
    assert!(!report.passed);
    assert!(report
        .repairable
        .iter()
        .any(|issue| issue.contains("content_depth_below_minimum")));
    assert!(report
        .repairable
        .iter()
        .any(|issue| issue.contains("missing_required_research_sections")));
}

#[test]
fn delegated_research_paper_quality_contract_accepts_structured_grounded_body() {
    let task = "Original user request:\nartifact_type: research_paper\n请根据知识库写一篇治疗心脏病的论文，做成 PDF。\n\nVerified researcher evidence:\n- DOI: 10.1016/example\n- PMID: 123456\n\nKnowledge import receipt:\nstatus: completed";
    let body = "补充论证。".repeat(900);
    let content = format!(
        "# 基于证据的心血管治疗论文\n\n摘要\n{body}\n\n引言\n{body}\n\n方法\n{body}\n\n结果\n{body}\n\n讨论\n{body}\n\n结论\n{body}\n\n参考文献\n[1] DOI: 10.1016/example\n[2] PubMed PMID: 123456\n"
    );

    let contract = DelegateTool::artifact_quality_contract(task);
    let report = DelegateTool::artifact_quality_report_with_contract(task, &content, &contract);

    assert!(
        report.passed,
        "unexpected issues: {:?}",
        report.actionable_issues()
    );
    assert!(report
        .to_tool_result_section()
        .contains("quality_contract: pass"));
}

#[test]
fn delegated_quality_contract_can_come_from_worker_policy() {
    let coordinator = Arc::new(Coordinator::new());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("writer".to_string()),
        agent_path: PathBuf::from("/tmp/writer"),
        display_name: "Writer".to_string(),
        description: Some("Configurable artifact writer.".to_string()),
        tools: vec!["write_file".to_string()],
        artifact_policy: Some(serde_json::json!({
            "quality_contracts": [{
                "artifact": "briefing",
                "triggers": ["简报"],
                "min_chars": 120,
                "min_citations": 1,
                "required_sections": ["标题", "要点", "参考"],
                "require_self_review": true
            }]
        })),
    });

    let contract =
        DelegateTool::artifact_quality_contract_for_coordinator(&coordinator, "写一份简报 txt");
    let weak = "标题\n太短";
    let report =
        DelegateTool::artifact_quality_report_with_contract("写一份简报 txt", weak, &contract);

    assert_eq!(contract.artifact_type, "briefing");
    assert!(!report.passed);
    assert!(report
        .repairable
        .iter()
        .any(|issue| issue.contains("content_depth_below_minimum")));
}

#[test]
fn delegated_quality_contract_keeps_user_explicit_length_ceiling_over_policy() {
    let coordinator = Arc::new(Coordinator::new());
    coordinator.register_worker_blueprint(WorkerBlueprint {
        role: AgentRole::Custom("writer".to_string()),
        agent_path: PathBuf::from("/tmp/writer"),
        display_name: "Writer".to_string(),
        description: Some("Configurable artifact writer.".to_string()),
        tools: vec!["write_file".to_string()],
        artifact_policy: Some(serde_json::json!({
            "quality_contracts": [{
                "artifact": "written_document",
                "default": true,
                "min_chars": 900,
                "required_sections": ["标题", "正文"]
            }]
        })),
    });

    let task = "请写一篇不超过300字的原创微型故事，并保存成 txt 文档";
    let contract = DelegateTool::artifact_quality_contract_for_coordinator(&coordinator, task);
    let overrun = format!("# 标题\n\n正文\n{}", "长".repeat(350));
    let report = DelegateTool::artifact_quality_report_with_contract(task, &overrun, &contract);

    assert_eq!(contract.artifact_type, "written_document");
    assert_eq!(contract.max_chars, Some(300));
    assert!(contract.min_chars <= 300);
    assert!(!report.passed);
    assert!(report
        .repairable
        .iter()
        .any(|issue| issue.contains("content_depth_above_maximum")));
}

#[test]
fn longform_plan_seed_locks_existing_project_identity() {
    let seed = LongformContinuationSeed {
        title: Some("云海试炼".to_string()),
        primary_anchor: Some("林衡".to_string()),
        last_next_hook: Some("林衡必须进入雾门。".to_string()),
        context: Some("角色：林衡；世界规则：灵脉会回应誓言。".to_string()),
    };

    let plan = DelegateTool::build_longform_continuation_plan_with_seed(
        "继续已有长文档并保存成 txt",
        "data/generated/tasks/test/agent-artifact-1.txt",
        Some(seed),
    );

    let contract = plan.contract.expect("contract");
    assert!(contract
        .anchors
        .iter()
        .any(|anchor| { anchor.name == "locked_title" && anchor.value == "云海试炼" }));
    assert!(contract
        .anchors
        .iter()
        .any(|anchor| { anchor.name == "locked_primary_anchor" && anchor.value == "林衡" }));
    assert_eq!(plan.steps[0].label, "chapter-draft-1");
    assert!(!plan.steps[0]
        .expected_output
        .as_deref()
        .unwrap_or_default()
        .contains("document identity block"));
}

#[test]
fn longform_plan_uses_original_user_request_as_objective() {
    let task = "The user wants to write a long novel. The first step is NOT to write the text, but to create a world bible.\n\nOriginal user request:\n请写一部完整的原创仙侠小说，目标50万字，并保存成 txt 文档。要求有作品名、稳定主角和主要配角、正文连续推进。";

    let plan = DelegateTool::build_longform_continuation_plan(
        task,
        "data/generated/tasks/test/agent-artifact-1.txt",
    );

    assert!(plan.objective.contains("目标50万字"));
    assert!(!plan.objective.contains("first step is NOT"));
    assert_eq!(plan.steps.len(), 278);
    let contract = plan.contract.expect("contract");
    assert!(contract.anchors.iter().any(|anchor| {
        anchor.name == "objective" && anchor.value.contains("请写一部完整的原创仙侠小说")
    }));
}

#[test]
fn writer_with_novel_studio_defers_governed_fiction_to_writing_tool() {
    let task = "请写一部完整的原创仙侠小说，目标50万字，并保存成 txt 文档。要求有稳定主角、世界观、连续章节和结局。";
    let tools = vec!["writing".to_string()];
    let writer = AgentRole::Custom("writer".to_string());

    assert!(DelegateTool::task_requests_governed_fiction_project(task));
    assert!(DelegateTool::worker_has_novel_studio_tool(&tools));
    assert!(!DelegateTool::should_use_managed_continuous_fast_path(
        &writer, &tools, task
    ));
    assert!(DelegateTool::task_requests_local_file_continuation(task));
}

#[test]
fn checkpointed_writer_task_is_not_deferred_by_self_review_revision_wording() {
    let task = "Create or continue the requested written artifact and save it as a text document. Write the artifact at `data/generated/tasks/test/agent-artifact-1.txt` with the available writing/file artifact tool. For substantial artifacts, satisfy the generic artifact quality contract: minimum structure, evidence/citation grounding when applicable, sufficient depth, and self-review/revision notes. If the requested output is too large for one model response or exceeds a text tool limit, use the generic checkpointed continuation flow instead of stopping at a starter artifact. Original user request: 搜索一个科幻星际类型小说，尝试入知识库，根据这个的基础来写小说 50万字";

    assert!(DelegateTool::task_requests_existing_artifact_revision(task));
    assert!(DelegateTool::task_requests_checkpointed_text_artifact(task));
    assert!(DelegateTool::task_requests_local_file_continuation(task));
    assert!(DelegateTool::select_longform_artifact_path(task)
        .as_deref()
        .is_some_and(|path| path.ends_with("agent-artifact-1.txt")));
    assert!(!DelegateTool::writer_fast_path_should_defer_existing_revision(task));
}

#[test]
fn governed_fiction_worker_contract_hides_generic_writing_studio_surface() {
    let task = "根据素材写一部50万字的科幻星际小说，保持角色和世界观连续。";
    let tools = vec!["writing".to_string()];
    let role = AgentRole::Custom("writer".to_string());

    let contract = DelegateTool::build_worker_execution_contract(&role, &tools, task, None);

    let tools_line = contract
        .lines()
        .find(|line| line.contains("Available specialist tools:"))
        .expect("tools line");
    assert!(tools_line.contains("novel_studio"));
    assert!(!tools_line.contains("writing_studio"));
    assert!(!tools_line.contains("write_file"));
    assert!(!tools_line.contains("read_file"));
    assert!(!tools_line.contains("list_dir"));
    assert!(!tools_line.contains("edit_file"));
}

#[test]
fn governed_fiction_continuation_hides_generic_file_surface() {
    let task = "继续写第4章。延续上一章的人物、地点、修炼体系和伏笔，不要重置设定。";
    let tools = vec!["writing".to_string()];
    let role = AgentRole::Custom("writer".to_string());

    assert!(DelegateTool::task_requests_governed_fiction_project(task));

    let contract = DelegateTool::build_worker_execution_contract(&role, &tools, task, None);
    let tools_line = contract
        .lines()
        .find(|line| line.contains("Available specialist tools:"))
        .expect("tools line");

    assert!(tools_line.contains("novel_studio"));
    assert!(!tools_line.contains("read_file"));
    assert!(!tools_line.contains("write_file"));
    assert!(!tools_line.contains("list_dir"));
}

#[test]
fn writer_without_novel_studio_can_still_use_generic_continuous_path() {
    let task = "请写一部完整的原创小说，目标5万字，并保存成 txt 文档。";
    let tools = vec!["write_file".to_string()];
    let writer = AgentRole::Custom("writer".to_string());

    assert!(DelegateTool::task_requests_governed_fiction_project(task));
    assert!(!DelegateTool::worker_has_novel_studio_tool(&tools));
    assert!(DelegateTool::should_use_managed_continuous_fast_path(
        &writer, &tools, task
    ));
}

#[test]
fn non_fiction_large_document_still_uses_generic_continuous_path() {
    let task = "根据知识库写一份完整研究报告，目标5万字，并保存成 txt 文档。";
    let tools = vec!["writing".to_string()];
    let writer = AgentRole::Custom("writer".to_string());

    assert!(!DelegateTool::task_requests_governed_fiction_project(task));
    assert!(DelegateTool::should_use_managed_continuous_fast_path(
        &writer, &tools, task
    ));
}

#[test]
fn delegate_worker_session_id_is_parent_and_role_scoped() {
    let task_id = uuid::Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
    let session = DelegateTool::delegated_worker_session_id(
        Some("用户 Session/一"),
        Some(task_id),
        "writer",
        "chapter draft",
    );
    assert_eq!(session, "session::worker::writer::chapter_draft");

    let fallback =
        DelegateTool::delegated_worker_session_id(None, Some(task_id), "researcher", "search");
    assert_eq!(
        fallback,
        "task_12345678123456781234567812345678::worker::researcher::search"
    );
}

#[test]
fn governed_writing_uses_original_request_when_delegate_only_plans() {
    let original = "帮我写一个草根逆袭的玄幻小说。每章不小于4000字，每次只写1章。";
    let delegated = "Create a detailed outline for a grassroots-to-top fantasy story.";
    let task = DelegateTool::task_with_constraint_source(delegated, Some(original));

    let workflow_task = DelegateTool::governed_writing_workflow_task(&task, Some(original));

    assert_eq!(workflow_task, original);
    assert_eq!(
        DelegateTool::requested_chapter_count_with_step_target(workflow_task, 4000),
        1
    );
}

#[test]
fn governed_writing_start_chapter_does_not_override_total_target() {
    let workflow_task = "\
用户已经在多轮对话中确认小说创作草案，请不要继续追问，直接通过 writer worker 使用 novel_studio 继续正式写作。
project_path: /tmp/example
语言：zh-CN
总目标字数：500000
每章目标字数：3000
要求：从该项目继续执行章节写作；如果用户指定总目标字数，就按总目标持续推进；请从第一章开始。";

    assert_eq!(
        DelegateTool::requested_chapter_count_with_step_target(workflow_task, 3000),
        200
    );
    assert_eq!(
        DelegateTool::requested_start_chapter(workflow_task),
        Some(1)
    );
}

#[test]
fn governed_writing_conditional_single_chapter_fallback_does_not_override_total_target() {
    let workflow_task = "\
用户已经在多轮对话中确认小说创作草案，请不要继续追问，直接通过 writer worker 使用 novel_studio 继续正式写作。
project_path: /tmp/example
语言：zh-CN
简述：我要一部草根逆袭的科幻玄幻，每章约3000字，一共约50万字。
总目标字数：500000
每章目标字数：3000
要求：从该项目继续执行章节写作；如果用户指定总目标字数，就按总目标持续推进，否则先完成第一章；正文保存到 artifact/TXT。";

    assert_eq!(
        DelegateTool::requested_chapter_count_with_step_target(workflow_task, 3000),
        200
    );
}

#[test]
fn existing_artifact_path_accepts_safe_relative_generated_novel_path() {
    let task = "请清理项目路径 data/generated/novels/长歌记 里的转义残片并重新导出 txt。";

    assert_eq!(
        DelegateTool::extract_existing_artifact_project_path(task).as_deref(),
        Some("data/generated/novels/长歌记")
    );
}

#[test]
fn existing_artifact_path_accepts_chinese_path_label() {
    let task = "任务要求：\n1. 路径：data/generated/novels/长歌记\n2. 只清理正文残片。";

    assert_eq!(
        DelegateTool::extract_existing_artifact_project_path(task).as_deref(),
        Some("data/generated/novels/长歌记")
    );
}

#[test]
fn existing_artifact_path_accepts_chinese_target_path_label() {
    let task = "清理正文。\n目标路径：data/generated/novels/长歌记\n不要新建项目。";

    assert_eq!(
        DelegateTool::extract_existing_artifact_project_path(task).as_deref(),
        Some("data/generated/novels/长歌记")
    );
}

#[test]
fn existing_artifact_path_rejects_creation_draft_json_path() {
    let task = "\
[BENSHU_DIRECT_WRITER_CONTINUATION]
用户已经明确要求开始小说写作。
draft_path: /home/user/project/data/generated/novels/drafts/未命名小说.json
用户最新要求：开始写第一章。";

    assert_eq!(
        DelegateTool::extract_existing_artifact_project_path(task).as_deref(),
        None
    );
}

#[test]
fn creation_draft_path_extracts_only_draft_json() {
    let task = "\
[BENSHU_DIRECT_WRITER_CONTINUATION]
draft_path: /home/user/project/data/generated/novels/drafts/未命名小说.json
用户最新要求：开始写第一章。";

    assert_eq!(
        DelegateTool::extract_creation_draft_path(task).as_deref(),
        Some("/home/user/project/data/generated/novels/drafts/未命名小说.json")
    );
}
