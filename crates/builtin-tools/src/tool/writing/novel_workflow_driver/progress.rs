use super::{chapter_runtime::StreamProgressThrottleState, NovelWorkflowRuntimeState};
use async_trait::async_trait;
use benshu_brain::agent::multi_agent::TextGenerationProgressStage;
use benshu_brain::runtime::continuous_task::{
    ContinuousCheckpointSink, ContinuousStepRequest, ContinuousStepResult,
    PersistentTaskCheckpointSink,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const WORKFLOW_TASK_CHECKPOINT_LIMIT: usize = 96;
const STREAM_PROGRESS_MIN_CHARS: usize = 768;
const STREAM_PROGRESS_MIN_INTERVAL: Duration = Duration::from_secs(2);

pub(super) enum NovelWorkflowSink {
    Plain(NovelWorkflowCheckpointSink),
    Persistent(PersistentTaskCheckpointSink<NovelWorkflowCheckpointSink>),
}

pub(super) fn build_workflow_sink(runtime: &NovelWorkflowRuntimeState) -> NovelWorkflowSink {
    let sink = NovelWorkflowCheckpointSink;
    let (Some(task_manager), Some(task_id)) = (runtime.task_manager.clone(), runtime.task_id)
    else {
        return NovelWorkflowSink::Plain(sink);
    };
    let persistent = PersistentTaskCheckpointSink::new(task_manager, task_id, sink);
    NovelWorkflowSink::Persistent(if let Some(event_manager) = runtime.event_manager.clone() {
        persistent.with_event_manager(event_manager)
    } else {
        persistent
    })
}

#[async_trait]
impl ContinuousCheckpointSink for NovelWorkflowSink {
    async fn record_step(
        &mut self,
        request: &ContinuousStepRequest,
        result: &ContinuousStepResult,
    ) -> anyhow::Result<Option<String>> {
        match self {
            Self::Plain(sink) => sink.record_step(request, result).await,
            Self::Persistent(sink) => sink.record_step(request, result).await,
        }
    }

    async fn record_step_attempt(
        &mut self,
        request: &ContinuousStepRequest,
        status: &str,
        reason: &str,
        output_preview: Option<&str>,
    ) -> anyhow::Result<()> {
        match self {
            Self::Plain(sink) => {
                sink.record_step_attempt(request, status, reason, output_preview)
                    .await
            }
            Self::Persistent(sink) => {
                sink.record_step_attempt(request, status, reason, output_preview)
                    .await
            }
        }
    }
}

pub(super) struct NovelWorkflowCheckpointSink;

#[async_trait]
impl ContinuousCheckpointSink for NovelWorkflowCheckpointSink {
    async fn record_step(
        &mut self,
        _request: &ContinuousStepRequest,
        result: &ContinuousStepResult,
    ) -> anyhow::Result<Option<String>> {
        Ok(result.artifact_uri.clone())
    }
}

pub(super) async fn record_workflow_checkpoint(
    runtime: &NovelWorkflowRuntimeState,
    step: u32,
    label: &str,
    summary: String,
) {
    let Some(task_id) = runtime.task_id else {
        return;
    };
    let timeout = std::time::Duration::from_secs(checkpoint_io_timeout_secs());
    if let Some(manager) = runtime.event_manager.as_ref() {
        let append = manager.append(
            benshu_state::RuntimeEventRecord::new("novel.workflow.progress")
                .with_task(task_id)
                .with_payload(json!({
                    "step": step,
                    "label": label,
                    "summary": summary.clone(),
                })),
        );
        let _ = tokio::time::timeout(timeout, append).await;
    }
    if let Some(manager) = runtime.task_manager.as_ref() {
        let task_key = task_id.to_string();
        let label = label.to_string();
        let persist = async {
            if let Ok(Some(mut task)) = manager.load(&task_key).await {
                task.updated_at = chrono::Utc::now();
                task.current_step = step;
                push_task_checkpoint_bounded(
                    &mut task.checkpoints,
                    benshu_state::TaskCheckpoint {
                        step,
                        label,
                        recorded_at: chrono::Utc::now(),
                        summary: Some(summary),
                    },
                );
                let _ = manager.save_preserving_completion_fields(task).await;
            }
        };
        let _ = tokio::time::timeout(timeout, persist).await;
    }
}

pub(super) fn stream_progress_should_emit(
    throttle: &Arc<Mutex<BTreeMap<String, StreamProgressThrottleState>>>,
    chapter_number: usize,
    phase: &str,
    generated_chars: usize,
    stage: TextGenerationProgressStage,
) -> bool {
    if !matches!(stage, TextGenerationProgressStage::Streaming) {
        return true;
    }
    let key = format!("{chapter_number}:{phase}");
    let now = Instant::now();
    let Ok(mut states) = throttle.lock() else {
        return true;
    };
    match states.get_mut(&key) {
        Some(state) => {
            let char_delta = generated_chars.saturating_sub(state.last_chars);
            if char_delta >= STREAM_PROGRESS_MIN_CHARS
                || now.duration_since(state.last_emit) >= STREAM_PROGRESS_MIN_INTERVAL
            {
                state.last_chars = generated_chars;
                state.last_emit = now;
                true
            } else {
                false
            }
        }
        None => {
            states.insert(
                key,
                StreamProgressThrottleState {
                    last_emit: now,
                    last_chars: generated_chars,
                },
            );
            true
        }
    }
}

pub(super) fn push_task_checkpoint_bounded(
    checkpoints: &mut Vec<benshu_state::TaskCheckpoint>,
    checkpoint: benshu_state::TaskCheckpoint,
) {
    checkpoints.push(checkpoint);
    if checkpoints.len() <= WORKFLOW_TASK_CHECKPOINT_LIMIT {
        return;
    }
    let overflow = checkpoints
        .len()
        .saturating_sub(WORKFLOW_TASK_CHECKPOINT_LIMIT);
    checkpoints.drain(0..overflow);
}

fn checkpoint_io_timeout_secs() -> u64 {
    std::env::var("BENSHU_NOVEL_CHECKPOINT_IO_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=30).contains(value))
        .unwrap_or(2)
}
