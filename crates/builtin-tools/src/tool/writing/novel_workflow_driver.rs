//! Novel workflow orchestration.
//!
//! The driver owns step order, checkpoints, progress events, and calls into
//! focused workflow/studio helpers. It should not become the authority for
//! contract readiness, naming quality, body quality, or metadata repair.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use benshu_brain::agent::multi_agent::MultiAgent;
use benshu_brain::agent::multi_agent::{
    TextGenerationLimits, TextGenerationProgress, TextGenerationProgressSink,
    TextGenerationProgressStage,
};
use benshu_brain::runtime::continuous_task::{
    ContinuousActionRunner, ContinuousArtifactTarget, ContinuousStepAction, ContinuousStepBlocker,
    ContinuousStepRequest, ContinuousStepResult, ContinuousTaskAnchor, ContinuousTaskContract,
    ContinuousTaskExecutor, ContinuousTaskPlan, ContinuousTaskPolicy, ContinuousTaskStatus,
};
use benshu_compression::preview_text;
use benshu_infra::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::chapter_quality;
use super::longform_policy;
use super::naming;
use super::novel_contract_v2::ChapterCharacterRegistration;
use super::novel_governance::{
    self as governance, AuthorityRole, CandidateProvenance, DraftCandidateRecord,
    RevisionQualityVector, SealedChapterAuthority,
};
use super::novel_pipeline;
use super::novel_pipeline::lifecycle as chapter_lifecycle;
use super::novel_runner;
use super::novel_studio::{final_body_has_required_end_state_evidence, NovelStudioTool};
use super::surface_sanitizer::{self, strip_markdown_frontmatter as strip_frontmatter};

mod audit;
mod chapter;
mod chapter_loop;
mod chapter_runtime;
mod completion;
mod content_ops;
mod metadata_repair;
mod naming_recovery;
mod output_cleanup;
mod planning;
mod progress;
mod project_goal;
mod project_state;
mod provider;
mod quality;
mod result_format;

const MAX_LLM_REVISION_ATTEMPTS: usize = 5;
const MAX_CHAPTER_STEP_RETRY_ATTEMPTS: usize = 10;
const MAX_TAIL_COMPLETION_RECOVERIES: usize = 1;

#[cfg(test)]
use super::naming::title_language_mismatch;
use chapter::*;
use chapter_loop::*;
use completion::*;
pub(crate) use content_ops::task_requests_novel_surface_cleanup;
#[cfg(test)]
pub(crate) use content_ops::{
    content_contains_surface_cleanup_target, format_novel_content_mutation_result,
};
pub use content_ops::{run_novel_content_operation_for_delegate, NovelContentOperationConfig};
use metadata_repair::*;
#[cfg(test)]
use naming_recovery::user_facing_task_brief;
use output_cleanup::*;
use planning::*;
use progress::*;
use project_goal::*;
use project_state::*;
use provider::*;
use quality::*;
use result_format::*;

#[derive(Clone, Default)]
pub struct NovelWorkflowRuntimeState {
    pub task_id: Option<Uuid>,
    pub task_manager: Option<Arc<benshu_state::TaskManager>>,
    pub event_manager: Option<Arc<benshu_state::RuntimeEventManager>>,
}

#[derive(Clone)]
pub struct NovelWorkflowConfig {
    pub workspace: PathBuf,
    pub worker_label: String,
    pub target_units: Option<usize>,
    pub chapter_unit_target: Option<usize>,
    pub chapter_count: usize,
    pub requested_start_chapter: Option<usize>,
    pub existing_project_path: Option<String>,
    pub creation_draft_path: Option<String>,
    pub runtime: NovelWorkflowRuntimeState,
}

pub async fn run_novel_workflow_for_delegate(
    agent: Arc<dyn MultiAgent>,
    task: &str,
    config: NovelWorkflowConfig,
) -> anyhow::Result<String> {
    let tool = NovelStudioTool::new(config.workspace.clone(), config.worker_label.clone());
    let mut target_units = config.target_units;
    let configured_chapter_count = config.chapter_count;
    let (project_path, language, chapter_unit_target, start_chapter, chapter_count) = if let Some(
        existing_project_path,
    ) = config
        .existing_project_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut status = call_novel_studio_json(
            &tool,
            json!({
                "action": "status",
                "project_path": existing_project_path
            }),
        )
        .await?;
        let project_path = required_string(&status, "project_path")?.to_string();
        let project_repair_feedback =
            crate::tool::writing::session_route::writing_command_from_task(task)
                .filter(|command| command.operation.is_none())
                .map(|command| command.user_request)
                .filter(|request| {
                    crate::tool::writing::session_route::intent_requests_project_state_repair(
                        request,
                    )
                });
        if let Some(feedback) = project_repair_feedback {
            let repaired = call_novel_studio_json(
                &tool,
                json!({
                    "action": "repair_project_state",
                    "project_path": project_path,
                    "feedback": feedback
                }),
            )
            .await?;
            if !repaired
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Ok(content_ops::format_novel_project_state_repair_result(
                    &repaired,
                ));
            }
            record_workflow_checkpoint(
                &config.runtime,
                0,
                "novel-project-state:repair:completed",
                "已按用户要求修复并重新校验项目状态，继续执行后续写作。".to_string(),
            )
            .await;
            status = call_novel_studio_json(
                &tool,
                json!({
                    "action": "status",
                    "project_path": project_path
                }),
            )
            .await?;
        }
        let existing_state = status.get("state").cloned().unwrap_or_else(|| json!({}));
        let existing_target_units = state_usize(&existing_state, "target_units");
        let requested_target_units =
            sanitize_existing_project_target_update(config.target_units, existing_target_units);
        target_units = requested_target_units.or(existing_target_units);
        if requested_target_units.is_some() && requested_target_units != existing_target_units {
            call_novel_studio_json_with_timeout(
                &tool,
                json!({
                    "action": "update_project",
                    "project_path": project_path,
                    "target_units": requested_target_units
                }),
                local_tool_stage_timeout_secs(),
                "update_existing_project_target_units",
            )
            .await?;
        }
        let existing_chapter_unit_target = state_usize(&existing_state, "chapter_unit_target");
        let requested_chapter_unit_target = longform_policy::normalize_chapter_unit_target(
            config.chapter_unit_target,
            target_units,
        );
        if requested_chapter_unit_target.is_some()
            && requested_chapter_unit_target != existing_chapter_unit_target
        {
            call_novel_studio_json_with_timeout(
                &tool,
                json!({
                    "action": "update_project",
                    "project_path": project_path,
                    "chapter_unit_target": requested_chapter_unit_target
                }),
                local_tool_stage_timeout_secs(),
                "update_existing_project_chapter_unit_target",
            )
            .await?;
        }
        record_workflow_checkpoint(
            &config.runtime,
            0,
            "novel-project-state:repair:skipped",
            "已跳过入口全量修复；写作链路只在明确阻塞或用户要求修复时运行 repair_project_state，避免长篇项目热路径重复扫描。".to_string(),
        )
        .await;
        let status = call_novel_studio_json(
            &tool,
            json!({
                "action": "status",
                "project_path": project_path
            }),
        )
        .await?;
        let state = status.get("state").cloned().unwrap_or_else(|| json!({}));
        let language = state_string(&state, "language").unwrap_or_else(|| {
            if naming::prefers_chinese_output(task) {
                "Chinese".to_string()
            } else {
                "English".to_string()
            }
        });
        let chapter_unit_target = resolve_chapter_unit_target(
            config.chapter_unit_target,
            state_usize(&state, "chapter_unit_target"),
            target_units,
            configured_chapter_count,
        );
        let state_next_chapter = state_usize(&state, "next_chapter")
            .filter(|value| *value > 0)
            .unwrap_or(1);
        let first_unapproved_chapter =
            state_usize(&state, "first_unapproved_chapter").filter(|value| *value > 0);
        let start_chapter = if let Some(first_unapproved_chapter) = first_unapproved_chapter {
            first_unapproved_chapter
        } else {
            config
                .requested_start_chapter
                .filter(|value| *value > 0)
                .map(|requested| requested.max(state_next_chapter))
                .unwrap_or(state_next_chapter)
        };
        let approved_units = state_usize(&state, "approved_units").unwrap_or(0);
        let chapter_count = existing_project_turn_chapter_count(
            configured_chapter_count,
            approved_units,
            target_units,
            chapter_unit_target,
            first_unapproved_chapter.is_some(),
            task_requests_single_chapter_turn(task),
            task_requests_project_scale_generation(task),
        );
        (
            project_path,
            language,
            chapter_unit_target,
            start_chapter,
            chapter_count,
        )
    } else {
        let draft_path = match config
            .creation_draft_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(draft_path) => draft_path,
            None => {
                record_workflow_checkpoint(
                    &config.runtime,
                    0,
                    "novel-draft:missing",
                    "没有可批准的创作合同草案，已停止章节生成；请先完成并确认写作合同。"
                        .to_string(),
                )
                .await;
                anyhow::bail!(
                    "novel workflow requires an approved creation draft or existing project; generate and confirm the writing contract before starting chapters"
                );
            }
        };
        record_workflow_checkpoint(
            &config.runtime,
            0,
            "novel-draft:approve:start",
            "正在批准已确认的创作合同草案并初始化小说项目。".to_string(),
        )
        .await;
        let approved = call_novel_studio_json_raw_with_timeout(
            &tool,
            json!({
                "action": "approve_draft",
                "draft_path": draft_path
            }),
            local_tool_stage_timeout_secs(),
            "approve_ready_creation_draft",
        )
        .await?;
        if !approved
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            record_workflow_checkpoint(
                &config.runtime,
                0,
                "novel-draft:approve:blocked",
                format!(
                    "创作合同草案尚未通过批准，已停止章节生成：{}",
                    preview_text(&approved.to_string(), 360)
                ),
            )
            .await;
            anyhow::bail!(
                "creation draft is not ready for chapter writing: {}",
                preview_text(&approved.to_string(), 420)
            );
        }
        let project_path = required_string(&approved, "project_path")?.to_string();
        record_workflow_checkpoint(
            &config.runtime,
            0,
            "novel-draft:approve:completed",
            format!("创作合同草案已批准并初始化小说项目：{project_path}。"),
        )
        .await;
        let status = call_novel_studio_json(
            &tool,
            json!({
                "action": "status",
                "project_path": project_path
            }),
        )
        .await?;
        let state = status.get("state").cloned().unwrap_or_else(|| json!({}));
        target_units = target_units.or_else(|| state_usize(&state, "target_units"));
        let language = state_string(&state, "language").unwrap_or_else(|| {
            if naming::prefers_chinese_output(task) {
                "Chinese".to_string()
            } else {
                "English".to_string()
            }
        });
        let chapter_unit_target = resolve_chapter_unit_target(
            config.chapter_unit_target,
            state_usize(&state, "chapter_unit_target"),
            target_units,
            configured_chapter_count,
        );
        let approved_units = state_usize(&state, "approved_units").unwrap_or(0);
        let chapter_count = existing_project_turn_chapter_count(
            configured_chapter_count,
            approved_units,
            target_units,
            chapter_unit_target,
            false,
            task_requests_single_chapter_turn(task),
            task_requests_project_scale_generation(task),
        );
        (
            project_path,
            language,
            chapter_unit_target,
            config.requested_start_chapter.unwrap_or(1),
            chapter_count,
        )
    };

    // The studio serializes individual file mutations. This lease serializes
    // the full read-model-write chapter workflow so two tasks cannot advance
    // the same project from the same authoritative state.
    let mut workflow_lease = Some(tool.lock_project_workflow(&project_path).await?);
    activate_project_goal(
        &project_path,
        target_units,
        chapter_unit_target,
        start_chapter,
    )
    .await?;

    let mut workflow_task = task.to_string();
    let mut chapter_count = chapter_count;
    let expanded_chapter_count = expand_chapter_count_to_explicit_target(
        start_chapter,
        chapter_count,
        config.requested_start_chapter,
    );
    if expanded_chapter_count > chapter_count {
        chapter_count = expanded_chapter_count;
        let requested_start_chapter = config.requested_start_chapter.unwrap_or(start_chapter);
        record_workflow_checkpoint(
            &config.runtime,
            start_chapter as u32,
            "novel-chapter-run:explicit-target-span",
            format!(
                "用户明确要求第 {requested_start_chapter} 章；当前需先处理第 {start_chapter} 章起的前序状态，本轮将覆盖到目标章节。"
            ),
        )
        .await;
    }
    let mut force_generation_after_target = false;
    let mut chapter_completion_gate = None;
    let allow_elastic_after_target = task_requests_complete_narrative(task);
    if project_approved_target_reached(&tool, &project_path).await? {
        let completion_gate =
            evaluate_project_completion_gate(agent.clone(), &tool, &project_path, task, &language)
                .await?;
        if completion_gate.complete {
            update_project_goal_progress(
                &project_path,
                start_chapter,
                DurableRunStatus::Completed,
                "",
            )
            .await?;
            let status_packet = call_novel_studio_json_with_timeout(
                &tool,
                json!({
                    "action": "run_project",
                    "project_path": project_path,
                    "export_when_complete": true,
                    "format": "txt"
                }),
                local_tool_stage_timeout_secs(),
                "run_project_completed_export",
            )
            .await?;
            return Ok(format_completed_project_result(
                &config.worker_label,
                &project_path,
                &status_packet,
                &format!(
                    "project satisfied target and narrative completion gate: {}",
                    completion_gate.reason
                ),
            ));
        }
        workflow_task = append_finale_instruction(task, &completion_gate, &language);
        chapter_count = 1;
        force_generation_after_target = true;
        chapter_completion_gate = Some(completion_gate.clone());
        record_workflow_checkpoint(
            &config.runtime,
            start_chapter as u32,
            "novel-completion-gate:finale_required",
            format!(
                "项目已达到最低字数，但叙事收束门要求追加终局章节：{}",
                completion_gate.reason
            ),
        )
        .await;
    }

    let runner = NovelChapterRunner {
        agent: agent.clone(),
        tool,
        project_path: project_path.clone(),
        language: language.to_string(),
        chapter_unit_target,
        worker_label: config.worker_label.clone(),
        runtime: config.runtime.clone(),
        force_generation_after_target,
        completion_gate: chapter_completion_gate,
        progress_throttle: Arc::new(Mutex::new(BTreeMap::new())),
        chapter_context_cache: Arc::new(Mutex::new(BTreeMap::new())),
    };
    let mut total_steps = chapter_count.max(1);
    let mut completed_steps = 0usize;
    let mut final_summary = String::new();
    let mut next_batch_chapter = start_chapter;
    let mut runner = ContinuousActionRunner::new(runner);
    while completed_steps < total_steps {
        if workflow_lease.is_none() {
            let inner = runner.into_inner();
            workflow_lease = Some(inner.tool().lock_project_workflow(&project_path).await?);
            runner = ContinuousActionRunner::new(inner);
        }
        let batch_size = total_steps
            .saturating_sub(completed_steps)
            .min(rolling_batch_chapter_limit());
        let plan = build_novel_continuous_plan(
            &workflow_task,
            &config.worker_label,
            &project_path,
            target_units,
            chapter_unit_target,
            batch_size,
            next_batch_chapter,
        );
        record_workflow_checkpoint(
            &config.runtime,
            next_batch_chapter as u32,
            "novel-chapter-run:rolling-batch-start",
            format!(
                "开始有限章节批次：从第 {next_batch_chapter} 章开始，本批 {} 章；完成后从磁盘重新读取连续批准进度。",
                batch_size
            ),
        )
        .await;
        let mut sink = build_workflow_sink(&config.runtime);
        let run = ContinuousTaskExecutor
            .run_with_checkpoint_sink(plan, &mut runner, &mut sink)
            .await?;
        completed_steps = completed_steps.saturating_add(run.completed_steps);
        if !final_summary.is_empty() {
            final_summary.push('\n');
        }
        final_summary.push_str(&run.final_summary());
        if !matches!(run.status, ContinuousTaskStatus::Completed) {
            let (goal_status, pause_reason) = match &run.status {
                ContinuousTaskStatus::Paused { reason } => {
                    (DurableRunStatus::Active, reason.as_str())
                }
                ContinuousTaskStatus::Blocked { reason }
                | ContinuousTaskStatus::Failed { reason } => {
                    (DurableRunStatus::Blocked, reason.as_str())
                }
                ContinuousTaskStatus::Completed => (DurableRunStatus::Active, ""),
            };
            update_project_goal_progress(
                &project_path,
                next_batch_chapter,
                goal_status,
                pause_reason,
            )
            .await?;
            return Ok(format_interrupted_novel_workflow_result(
                &config,
                &project_path,
                completed_steps,
                total_steps,
                &run.status,
                &final_summary,
            ));
        }
        let inner = runner.into_inner();
        let batch_status = call_novel_studio_json_with_timeout(
            inner.tool(),
            json!({
                "action": "status",
                "project_path": project_path
            }),
            local_tool_stage_timeout_secs(),
            "status_between_rolling_batches",
        )
        .await?;
        next_batch_chapter = batch_status
            .pointer("/state/next_chapter")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "rolling batch completed without durable next_chapter progress authority"
                )
            })?;
        update_project_goal_progress(
            &project_path,
            next_batch_chapter,
            DurableRunStatus::Active,
            "",
        )
        .await?;
        runner = ContinuousActionRunner::new(inner);
        drop(workflow_lease.take());
        tokio::task::yield_now().await;
    }
    if workflow_lease.is_none() {
        let inner = runner.into_inner();
        workflow_lease = Some(inner.tool().lock_project_workflow(&project_path).await?);
        runner = ContinuousActionRunner::new(inner);
    }
    let mut runner = runner.into_inner();

    record_workflow_checkpoint(
        &config.runtime,
        start_chapter.saturating_add(completed_steps) as u32,
        "novel-project:status:start",
        "章节批次已完成，正在刷新项目状态和 TXT 导出证据。".to_string(),
    )
    .await;
    let mut status_packet = call_novel_studio_json_with_timeout(
        runner.tool(),
        json!({
            "action": "status",
            "project_path": project_path
        }),
        local_tool_stage_timeout_secs(),
        "status_after_batch",
    )
    .await?;
    let mut project_complete = status_packet
        .get("complete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let target_reached_after_run =
        project_complete || project_approved_target_reached_from_status_packet(&status_packet);
    let mut completion_gate = if target_reached_after_run {
        Some(
            evaluate_project_completion_gate(
                agent.clone(),
                runner.tool(),
                &project_path,
                task,
                &language,
            )
            .await?,
        )
    } else {
        None
    };
    if let Some(gate) = completion_gate.as_ref() {
        project_complete = gate.complete;
    }
    if project_complete {
        status_packet = call_novel_studio_json_with_timeout(
            runner.tool(),
            json!({
                "action": "run_project",
                "project_path": project_path,
                "export_when_complete": true,
                "format": "txt"
            }),
            local_tool_stage_timeout_secs(),
            "run_project_export_after_completion",
        )
        .await?;
    }
    let mut seen_completion_debt_sets = BTreeSet::new();
    let mut completion_stalled = false;
    while completion_gate
        .as_ref()
        .is_some_and(|gate| gate.target_reached && !gate.complete)
        && (!force_generation_after_target || allow_elastic_after_target)
    {
        let gate = completion_gate.clone().unwrap_or_default();
        if !seen_completion_debt_sets.insert(gate.reason.clone()) {
            completion_stalled = true;
            break;
        }
        let state = status_packet
            .get("state")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let finale_start = state_usize(&state, "next_chapter")
            .filter(|value| *value > 0)
            .unwrap_or(start_chapter + completed_steps);
        let finale_task = append_finale_instruction(task, &gate, &language);
        record_workflow_checkpoint(
            &config.runtime,
            finale_start as u32,
            "novel-completion-gate:extra_finale_start",
            format!("最低字数已满足但故事未自然完结，追加第 {finale_start} 章作为收束章节。"),
        )
        .await;
        let finale_plan = build_novel_continuous_plan(
            &finale_task,
            &config.worker_label,
            &project_path,
            target_units,
            chapter_unit_target,
            1,
            finale_start,
        );
        let chapter_context_cache = runner.chapter_context_cache.clone();
        let finale_runner = NovelChapterRunner {
            agent: agent.clone(),
            tool: runner.tool,
            project_path: project_path.clone(),
            language: language.to_string(),
            chapter_unit_target,
            worker_label: config.worker_label.clone(),
            runtime: config.runtime.clone(),
            force_generation_after_target: true,
            completion_gate: Some(gate),
            progress_throttle: Arc::new(Mutex::new(BTreeMap::new())),
            chapter_context_cache,
        };
        let mut finale_runner = ContinuousActionRunner::new(finale_runner);
        let mut finale_sink = build_workflow_sink(&config.runtime);
        let finale_run = ContinuousTaskExecutor
            .run_with_checkpoint_sink(finale_plan, &mut finale_runner, &mut finale_sink)
            .await?;
        runner = finale_runner.into_inner();
        completed_steps += finale_run.completed_steps;
        total_steps += finale_run.total_steps;
        final_summary = format!("{}\n{}", final_summary, finale_run.final_summary());
        status_packet = call_novel_studio_json_with_timeout(
            runner.tool(),
            json!({
                "action": "status",
                "project_path": project_path
            }),
            local_tool_stage_timeout_secs(),
            "status_after_finale",
        )
        .await?;
        project_complete = status_packet
            .get("complete")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let target_reached_after_finale =
            project_complete || project_approved_target_reached_from_status_packet(&status_packet);
        completion_gate = if target_reached_after_finale {
            Some(
                evaluate_project_completion_gate(
                    agent.clone(),
                    runner.tool(),
                    &project_path,
                    task,
                    &language,
                )
                .await?,
            )
        } else {
            None
        };
        if let Some(gate) = completion_gate.as_ref() {
            project_complete = gate.complete;
        }
        if project_complete {
            status_packet = call_novel_studio_json_with_timeout(
                runner.tool(),
                json!({
                    "action": "run_project",
                    "project_path": project_path,
                    "export_when_complete": true,
                    "format": "txt"
                }),
                local_tool_stage_timeout_secs(),
                "run_project_export_after_finale",
            )
            .await?;
        }
        drop(workflow_lease.take());
        tokio::task::yield_now().await;
        if completion_gate
            .as_ref()
            .is_some_and(|gate| gate.target_reached && !gate.complete)
        {
            workflow_lease = Some(runner.tool().lock_project_workflow(&project_path).await?);
        }
    }
    if completion_stalled
        && completion_gate
            .as_ref()
            .is_some_and(|gate| gate.target_reached && !gate.complete)
        && (!force_generation_after_target || allow_elastic_after_target)
    {
        record_workflow_checkpoint(
            &config.runtime,
            state_usize(
                &status_packet
                    .get("state")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                "next_chapter",
            )
            .unwrap_or(start_chapter + completed_steps) as u32,
            "novel-completion-gate:no-typed-progress",
            "连续收束批次没有减少类型化合同债务；返回具体 blocker，停止盲目追加章节。".to_string(),
        )
        .await;
    }

    let requested_turn_complete = total_steps > 0 && completed_steps == total_steps;
    let next_goal_chapter = status_packet
        .pointer("/state/next_chapter")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(start_chapter.saturating_add(completed_steps));
    update_project_goal_progress(
        &project_path,
        next_goal_chapter,
        if project_complete {
            DurableRunStatus::Completed
        } else if completion_stalled {
            DurableRunStatus::Blocked
        } else {
            DurableRunStatus::Active
        },
        if completion_stalled {
            "typed completion debts made no net progress"
        } else {
            ""
        },
    )
    .await?;
    format_novel_workflow_result(
        &config,
        &project_path,
        &status_packet,
        project_complete,
        requested_turn_complete,
        completed_steps,
        total_steps,
        completion_gate.as_ref(),
        &final_summary,
    )
}

#[derive(Debug, Deserialize, Default)]
struct RawChapterQualityAudit {
    score: Option<u8>,
    #[serde(default)]
    authority_conflicts: Vec<RawAuthorityConflict>,
    #[serde(default, alias = "issues")]
    advisories: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawAuthorityConflict {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    authority_path: String,
    #[serde(default)]
    authority_excerpt: String,
    #[serde(default)]
    body_excerpt: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct RawDeliveryAdvisory {
    #[serde(default)]
    category: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct RawDeliveryAdvisoryWindow {
    #[serde(default)]
    advisories: Vec<RawDeliveryAdvisory>,
    score: Option<u8>,
}

#[async_trait]
impl benshu_brain::runtime::continuous_task::ContinuousActionHandler for NovelChapterRunner {
    async fn execute_action(
        &mut self,
        action: ContinuousStepAction,
        request: ContinuousStepRequest,
    ) -> anyhow::Result<ContinuousStepResult> {
        match action {
            ContinuousStepAction::Custom { action, .. } if action == "novel_workflow_chapter" => {
                if !self.force_generation_after_target
                    && self.project_approved_target_reached().await?
                {
                    let output = format!(
                        "project target already reached before chapter {}; skipped additional generation",
                        request.step.index
                    );
                    return Ok(ContinuousStepResult {
                        summary: output.clone(),
                        output,
                        artifact_uri: Some(self.project_path.clone()),
                    });
                }
                let output = self.run_chapter(&request).await?;
                if let Some(reason) = chapter_step_blocker_reason(&output) {
                    return Err(ContinuousStepBlocker::new(reason, output).into());
                }
                Ok(ContinuousStepResult {
                    summary: output.clone(),
                    output,
                    artifact_uri: Some(self.project_path.clone()),
                })
            }
            other => anyhow::bail!("unsupported novel workflow action: {other:?}"),
        }
    }
}

#[cfg(test)]
#[path = "novel_workflow_driver_tests.rs"]
mod novel_workflow_driver_tests;
