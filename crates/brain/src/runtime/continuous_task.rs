use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use benshu_runtime_policy_core::{
    is_recoverable_provider_disconnect, provider_service_pause_reason,
};

/// Generic policy for a bounded continuous task.
///
/// This executor is intentionally domain-agnostic. It does not know whether a
/// task is writing chapters, importing documents, browsing pages, or processing
/// files; it only enforces bounded steps, retries, checkpoints, and completion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuousTaskPolicy {
    pub max_steps: usize,
    pub max_retries_per_step: usize,
    pub stop_on_exact_repeat: bool,
    #[serde(default)]
    pub max_step_duration_secs: Option<u64>,
    #[serde(default)]
    pub max_step_total_duration_secs: Option<u64>,
}

impl Default for ContinuousTaskPolicy {
    fn default() -> Self {
        Self {
            max_steps: 32,
            max_retries_per_step: 1,
            stop_on_exact_repeat: true,
            max_step_duration_secs: Some(300),
            max_step_total_duration_secs: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuousArtifactTarget {
    pub uri: String,
    pub kind: String,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuousTaskAnchor {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ContinuousTaskContract {
    #[serde(default)]
    pub invariants: Vec<String>,
    #[serde(default)]
    pub anchors: Vec<ContinuousTaskAnchor>,
    #[serde(default)]
    pub completion_criteria: Vec<String>,
    #[serde(default)]
    pub required_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_event: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContinuousStepAction {
    Delegate {
        role: String,
        task: String,
    },
    Tool {
        name: String,
        arguments: serde_json::Value,
    },
    Skill {
        name: String,
        arguments: serde_json::Value,
    },
    Model {
        prompt: String,
    },
    Browser {
        operation: String,
        payload: serde_json::Value,
    },
    File {
        operation: String,
        path: String,
        payload: serde_json::Value,
    },
    Composite {
        actions: Vec<ContinuousStepAction>,
    },
    Custom {
        action: String,
        payload: serde_json::Value,
    },
}

impl Default for ContinuousStepAction {
    fn default() -> Self {
        Self::Custom {
            action: "instruction".to_string(),
            payload: serde_json::json!({}),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContinuousTaskStep {
    pub index: usize,
    pub label: String,
    pub instruction: String,
    pub expected_output: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<usize>,
    #[serde(default)]
    pub action: ContinuousStepAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContinuousTaskPlan {
    pub id: Uuid,
    pub objective: String,
    pub worker_role: String,
    pub steps: Vec<ContinuousTaskStep>,
    pub policy: ContinuousTaskPolicy,
    pub artifact_target: Option<ContinuousArtifactTarget>,
    #[serde(default)]
    pub contract: Option<ContinuousTaskContract>,
}

impl ContinuousTaskPlan {
    pub fn new(objective: impl Into<String>, worker_role: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            objective: objective.into(),
            worker_role: worker_role.into(),
            steps: Vec::new(),
            policy: ContinuousTaskPolicy::default(),
            artifact_target: None,
            contract: None,
        }
    }

    pub fn with_steps(mut self, steps: Vec<ContinuousTaskStep>) -> Self {
        self.steps = steps;
        self
    }

    pub fn with_policy(mut self, policy: ContinuousTaskPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn with_artifact_target(mut self, target: ContinuousArtifactTarget) -> Self {
        self.artifact_target = Some(target);
        self
    }

    pub fn with_contract(mut self, contract: ContinuousTaskContract) -> Self {
        self.contract = Some(contract);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContinuousStepRequest {
    pub task_id: Uuid,
    pub objective: String,
    pub worker_role: String,
    pub step: ContinuousTaskStep,
    pub previous_summary: Option<String>,
    #[serde(default)]
    pub recent_checkpoint_summaries: Vec<String>,
    #[serde(default)]
    pub attempt: usize,
    #[serde(default)]
    pub previous_error: Option<String>,
    #[serde(default)]
    pub contract: Option<ContinuousTaskContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuousStepResult {
    pub output: String,
    pub summary: String,
    pub artifact_uri: Option<String>,
}

/// A domain runner reached a durable blocker and must not be retried as an
/// infrastructure failure or recorded as a completed step.
#[derive(Debug)]
pub struct ContinuousStepBlocker {
    reason: String,
    output: String,
}

impl ContinuousStepBlocker {
    pub fn new(reason: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            output: output.into(),
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn output(&self) -> &str {
        &self.output
    }
}

impl std::fmt::Display for ContinuousStepBlocker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for ContinuousStepBlocker {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuousTaskCheckpoint {
    pub step: usize,
    pub label: String,
    pub recorded_at: DateTime<Utc>,
    pub summary: String,
    pub artifact_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContinuousTaskStatus {
    Completed,
    Paused { reason: String },
    Blocked { reason: String },
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuousTaskRun {
    pub task_id: Uuid,
    pub status: ContinuousTaskStatus,
    pub completed_steps: usize,
    pub total_steps: usize,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub checkpoints: Vec<ContinuousTaskCheckpoint>,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuousTaskCompletionReport {
    pub task_id: Uuid,
    pub status: ContinuousTaskStatus,
    pub completed_steps: usize,
    pub total_steps: usize,
    pub final_summary: String,
    pub artifacts: Vec<String>,
    pub checkpoint_summaries: Vec<String>,
}

impl ContinuousTaskRun {
    pub fn final_summary(&self) -> String {
        self.checkpoints
            .last()
            .map(|checkpoint| checkpoint.summary.clone())
            .or_else(|| self.outputs.last().cloned())
            .unwrap_or_else(|| "no steps completed".to_string())
    }

    pub fn completion_report(&self) -> ContinuousTaskCompletionReport {
        ContinuousTaskCompletionReport {
            task_id: self.task_id,
            status: self.status.clone(),
            completed_steps: self.completed_steps,
            total_steps: self.total_steps,
            final_summary: self.final_summary(),
            artifacts: self
                .checkpoints
                .iter()
                .filter_map(|checkpoint| checkpoint.artifact_uri.clone())
                .collect(),
            checkpoint_summaries: self
                .checkpoints
                .iter()
                .map(|checkpoint| {
                    format!(
                        "{}. {}: {}",
                        checkpoint.step, checkpoint.label, checkpoint.summary
                    )
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContinuousCompletionGateDecision {
    Complete,
    Blocked { reason: String },
}

pub fn continuous_completion_gate_decision(
    plan: &ContinuousTaskPlan,
    run: &ContinuousTaskRun,
    events: &[benshu_state::RuntimeEventRecord],
) -> ContinuousCompletionGateDecision {
    if !matches!(run.status, ContinuousTaskStatus::Completed) {
        return ContinuousCompletionGateDecision::Blocked {
            reason: format!("continuous task status is {:?}", run.status),
        };
    }

    let mut required_topics = plan
        .contract
        .as_ref()
        .map(|contract| contract.required_events.clone())
        .unwrap_or_default();
    if let Some(topic) = plan
        .contract
        .as_ref()
        .and_then(|contract| contract.completion_event.clone())
    {
        required_topics.push(topic);
    }
    let missing_topics = benshu_state::missing_required_topics(events, &required_topics);
    if !missing_topics.is_empty() {
        return ContinuousCompletionGateDecision::Blocked {
            reason: format!(
                "missing required runtime events: {}",
                missing_topics.join(", ")
            ),
        };
    }

    if let Some(signature) = benshu_state::repeated_event_signature(events, 12, 4) {
        return ContinuousCompletionGateDecision::Blocked {
            reason: format!("repeated runtime event signature detected: {signature}"),
        };
    }

    ContinuousCompletionGateDecision::Complete
}

#[async_trait]
pub trait ContinuousStepRunner {
    async fn run_step(
        &mut self,
        request: ContinuousStepRequest,
    ) -> anyhow::Result<ContinuousStepResult>;
}

#[async_trait]
pub trait ContinuousCheckpointSink {
    async fn record_step(
        &mut self,
        request: &ContinuousStepRequest,
        result: &ContinuousStepResult,
    ) -> anyhow::Result<Option<String>>;

    async fn record_step_attempt(
        &mut self,
        _request: &ContinuousStepRequest,
        _status: &str,
        _reason: &str,
        _output_preview: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct PersistentTaskCheckpointSink<S> {
    task_manager: Arc<benshu_state::TaskManager>,
    event_manager: Option<Arc<benshu_state::RuntimeEventManager>>,
    task_id: Uuid,
    inner: S,
}

impl<S> PersistentTaskCheckpointSink<S> {
    pub fn new(task_manager: Arc<benshu_state::TaskManager>, task_id: Uuid, inner: S) -> Self {
        Self {
            task_manager,
            event_manager: None,
            task_id,
            inner,
        }
    }

    pub fn with_event_manager(
        mut self,
        event_manager: Arc<benshu_state::RuntimeEventManager>,
    ) -> Self {
        self.event_manager = Some(event_manager);
        self
    }

    pub fn into_inner(self) -> S {
        self.inner
    }

    pub fn inner_ref(&self) -> &S {
        &self.inner
    }
}

#[async_trait]
impl<S> ContinuousCheckpointSink for PersistentTaskCheckpointSink<S>
where
    S: ContinuousCheckpointSink + Send,
{
    async fn record_step(
        &mut self,
        request: &ContinuousStepRequest,
        result: &ContinuousStepResult,
    ) -> anyhow::Result<Option<String>> {
        if let Some(task) = self.task_manager.load(&self.task_id.to_string()).await? {
            if matches!(task.status, benshu_state::TaskStatus::Cancelled) {
                anyhow::bail!("continuous task was cancelled");
            }
        }
        let artifact_uri = self.inner.record_step(request, result).await?;
        if let Some(event_manager) = &self.event_manager {
            let receipt = benshu_state::RuntimeReceipt {
                receipt_id: Uuid::new_v4(),
                status: "completed".to_string(),
                started_at: None,
                finished_at: Some(Utc::now()),
                actor: Some(request.worker_role.clone()),
                action: Some("continuous_step".to_string()),
                input_fingerprint: Some(runtime_fingerprint(&serde_json::to_value(request)?)),
                output_fingerprint: Some(runtime_fingerprint(&serde_json::json!({
                    "summary": result.summary.clone(),
                    "artifact_uri": result.artifact_uri.clone(),
                }))),
                output_preview: Some(compact_runtime_preview(&result.summary, 500)),
                blocker: None,
            };
            event_manager
                .append(
                    benshu_state::RuntimeEventRecord::new("continuous.step.checkpointed")
                        .with_task(request.task_id)
                        .with_actor(request.worker_role.clone())
                        .with_receipt(receipt)
                        .with_payload(serde_json::json!({
                            "step": request.step.index,
                            "label": request.step.label.clone(),
                            "summary": result.summary.clone(),
                            "artifact_uri": artifact_uri.clone().or_else(|| result.artifact_uri.clone()),
                        })),
                )
                .await?;
            if let Some(uri) = artifact_uri.clone().or_else(|| result.artifact_uri.clone()) {
                event_manager
                    .append(
                        benshu_state::RuntimeEventRecord::new("artifact.written")
                            .with_task(request.task_id)
                            .with_actor(request.worker_role.clone())
                            .with_payload(serde_json::json!({
                                "step": request.step.index,
                                "uri": uri,
                            })),
                    )
                    .await?;
            }
        }
        if let Some(mut task) = self.task_manager.load(&self.task_id.to_string()).await? {
            task.updated_at = Utc::now();
            if !matches!(
                task.status,
                benshu_state::TaskStatus::Cancelled | benshu_state::TaskStatus::Paused(_)
            ) {
                task.status = benshu_state::TaskStatus::Running;
            }
            task.current_step = request.step.index as u32;
            task.checkpoints.push(benshu_state::TaskCheckpoint {
                step: request.step.index as u32,
                label: request.step.label.clone(),
                recorded_at: Utc::now(),
                summary: Some(result.summary.clone()),
            });
            if let Some(uri) = artifact_uri.clone().or_else(|| result.artifact_uri.clone()) {
                let artifact_id = format!("continuous:{}:{}", request.task_id, request.step.index);
                if !task
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.artifact_id == artifact_id)
                {
                    task.artifacts.push(benshu_state::TaskArtifactRef {
                        artifact_id,
                        kind: "continuous_output".to_string(),
                        uri,
                        media_type: Some("text/plain".to_string()),
                    });
                }
            }
            self.task_manager.save(task).await?;
        }
        Ok(artifact_uri)
    }

    async fn record_step_attempt(
        &mut self,
        request: &ContinuousStepRequest,
        status: &str,
        reason: &str,
        output_preview: Option<&str>,
    ) -> anyhow::Result<()> {
        loop {
            if let Some(task) = self.task_manager.load(&self.task_id.to_string()).await? {
                match task.status {
                    benshu_state::TaskStatus::Cancelled => {
                        anyhow::bail!("continuous task was cancelled");
                    }
                    benshu_state::TaskStatus::Paused(_) => {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                    _ => {}
                }
            }
            break;
        }
        let status = if status.trim().is_empty() {
            "failed"
        } else {
            status.trim()
        };
        let summary = format!(
            "continuous step {} attempt {} {}: {}",
            request.step.index,
            request.attempt + 1,
            status,
            compact_runtime_preview(reason, 500)
        );
        if let Some(event_manager) = &self.event_manager {
            let receipt = benshu_state::RuntimeReceipt {
                receipt_id: Uuid::new_v4(),
                status: status.to_string(),
                started_at: None,
                finished_at: Some(Utc::now()),
                actor: Some(request.worker_role.clone()),
                action: Some("continuous_step_attempt".to_string()),
                input_fingerprint: Some(runtime_fingerprint(&serde_json::to_value(request)?)),
                output_fingerprint: output_preview
                    .map(|preview| runtime_fingerprint(&serde_json::json!({ "preview": preview }))),
                output_preview: output_preview.map(|preview| compact_runtime_preview(preview, 500)),
                blocker: if status == "running" {
                    None
                } else {
                    Some(compact_runtime_preview(reason, 500))
                },
            };
            event_manager
                .append(
                    benshu_state::RuntimeEventRecord::new("continuous.step.attempt")
                        .with_task(request.task_id)
                        .with_actor(request.worker_role.clone())
                        .with_receipt(receipt)
                        .with_payload(serde_json::json!({
                            "step": request.step.index,
                            "label": request.step.label.clone(),
                            "attempt": request.attempt + 1,
                            "status": status,
                            "reason": reason,
                            "output_chars": output_preview.map(|preview| preview.chars().count()),
                        })),
                )
                .await?;
        }
        if let Some(mut task) = self.task_manager.load(&self.task_id.to_string()).await? {
            task.updated_at = Utc::now();
            if !matches!(
                task.status,
                benshu_state::TaskStatus::Cancelled | benshu_state::TaskStatus::Paused(_)
            ) {
                task.status = benshu_state::TaskStatus::Running;
            }
            task.current_step = request.step.index as u32;
            task.checkpoints.push(benshu_state::TaskCheckpoint {
                step: request.step.index as u32,
                label: format!(
                    "{}:attempt-{}-{}",
                    request.step.label,
                    request.attempt + 1,
                    status
                ),
                recorded_at: Utc::now(),
                summary: Some(summary),
            });
            self.task_manager.save(task).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct NoopContinuousCheckpointSink;

#[async_trait]
impl ContinuousCheckpointSink for NoopContinuousCheckpointSink {
    async fn record_step(
        &mut self,
        _request: &ContinuousStepRequest,
        _result: &ContinuousStepResult,
    ) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

#[derive(Debug, Clone)]
pub struct FileAppendCheckpointSink {
    path: PathBuf,
    separator: String,
}

impl FileAppendCheckpointSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            separator: "\n\n".to_string(),
        }
    }

    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    pub async fn initialize(&self, content: impl AsRef<str>) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&self.path, content.as_ref()).await?;
        Ok(())
    }

    pub fn uri(&self) -> String {
        self.path.to_string_lossy().to_string()
    }
}

#[async_trait]
impl ContinuousCheckpointSink for FileAppendCheckpointSink {
    async fn record_step(
        &mut self,
        request: &ContinuousStepRequest,
        result: &ContinuousStepResult,
    ) -> anyhow::Result<Option<String>> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        if !self.separator.is_empty() {
            file.write_all(self.separator.as_bytes()).await?;
        }
        file.write_all(result.output.as_bytes()).await?;
        drop(file);
        refresh_declared_artifact_progress(&self.path, request.step.index).await?;
        Ok(Some(self.uri()))
    }
}

async fn refresh_declared_artifact_progress(
    path: &PathBuf,
    current_step: usize,
) -> anyhow::Result<()> {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return Ok(());
    };
    let refreshed = refresh_declared_artifact_progress_text(&content, current_step);
    if refreshed != content {
        tokio::fs::write(path, refreshed).await?;
    }
    Ok(())
}

fn refresh_declared_artifact_progress_text(content: &str, current_step: usize) -> String {
    let cjk = Regex::new(
        r"(?m)^(\s*(?:[-*+]\s+)?[*_`]*(?:当前进度|进度)[*_`]*\s*[:：]\s*(?:第\s*)?)(\d{1,6})(\s*/\s*[*_`]*)(\d{1,6})([*_`]*\s*(?:步|章|章节)?\s*)$",
    )
    .expect("valid CJK progress refresh regex");
    let refreshed = cjk.replace_all(content, |caps: &regex::Captures<'_>| {
        format!(
            "{}{}{}{}{}",
            &caps[1], current_step, &caps[3], &caps[4], &caps[5]
        )
    });

    let english = Regex::new(
        r"(?im)^(\s*(?:[-*+]\s+)?[*_`]*Current\s+Progress[*_`]*\s*[:：]\s*)(\d{1,6})(\s*/\s*[*_`]*)(\d{1,6})([*_`]*\s*(?:steps?)?\s*)$",
    )
    .expect("valid English progress refresh regex");
    english
        .replace_all(&refreshed, |caps: &regex::Captures<'_>| {
            format!(
                "{}{}{}{}{}",
                &caps[1], current_step, &caps[3], &caps[4], &caps[5]
            )
        })
        .into_owned()
}

#[async_trait]
pub trait ContinuousActionHandler {
    async fn execute_action(
        &mut self,
        action: ContinuousStepAction,
        request: ContinuousStepRequest,
    ) -> anyhow::Result<ContinuousStepResult>;
}

pub struct ContinuousActionRunner<H> {
    handler: H,
}

impl<H> ContinuousActionRunner<H> {
    pub fn new(handler: H) -> Self {
        Self { handler }
    }

    pub fn into_inner(self) -> H {
        self.handler
    }
}

#[async_trait]
impl<H> ContinuousStepRunner for ContinuousActionRunner<H>
where
    H: ContinuousActionHandler + Send,
{
    async fn run_step(
        &mut self,
        request: ContinuousStepRequest,
    ) -> anyhow::Result<ContinuousStepResult> {
        let action = request.step.action.clone();
        match action {
            ContinuousStepAction::Composite { actions } => {
                let mut outputs = Vec::new();
                let mut summaries = Vec::new();
                let mut artifact_uri = None;

                for action in actions {
                    let mut sub_request = request.clone();
                    sub_request.step.action = action.clone();
                    let result = self.handler.execute_action(action, sub_request).await?;
                    if !result.output.trim().is_empty() {
                        outputs.push(result.output);
                    }
                    if !result.summary.trim().is_empty() {
                        summaries.push(result.summary);
                    }
                    if result.artifact_uri.is_some() {
                        artifact_uri = result.artifact_uri;
                    }
                }

                Ok(ContinuousStepResult {
                    output: outputs.join("\n"),
                    summary: summaries.join("; "),
                    artifact_uri,
                })
            }
            action => self.handler.execute_action(action, request).await,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ContinuousTaskExecutor;

impl ContinuousTaskExecutor {
    pub async fn run<R>(
        &self,
        plan: ContinuousTaskPlan,
        runner: &mut R,
    ) -> anyhow::Result<ContinuousTaskRun>
    where
        R: ContinuousStepRunner + Send,
    {
        let mut sink = NoopContinuousCheckpointSink;
        self.run_with_checkpoint_sink(plan, runner, &mut sink).await
    }

    pub async fn run_with_checkpoint_sink<R, S>(
        &self,
        plan: ContinuousTaskPlan,
        runner: &mut R,
        sink: &mut S,
    ) -> anyhow::Result<ContinuousTaskRun>
    where
        R: ContinuousStepRunner + Send,
        S: ContinuousCheckpointSink + Send,
    {
        let started_at = Utc::now();
        let max_steps = plan.policy.max_steps.min(plan.steps.len());
        let mut checkpoints = Vec::new();
        let mut outputs = Vec::new();
        let mut previous_summary = None;
        let mut previous_output = None;

        if max_steps == 0 {
            let now = Utc::now();
            return Ok(ContinuousTaskRun {
                task_id: plan.id,
                status: ContinuousTaskStatus::Blocked {
                    reason: "continuous task has no executable steps".to_string(),
                },
                completed_steps: 0,
                total_steps: plan.steps.len(),
                started_at,
                finished_at: now,
                checkpoints,
                outputs,
            });
        }

        for step in plan.steps.iter().take(max_steps).cloned() {
            if let Some(missing_dependency) = step.depends_on.iter().find(|dependency| {
                !checkpoints
                    .iter()
                    .any(|checkpoint| checkpoint.step == **dependency)
            }) {
                let now = Utc::now();
                return Ok(ContinuousTaskRun {
                    task_id: plan.id,
                    status: ContinuousTaskStatus::Blocked {
                        reason: format!(
                            "step {} depends on unfinished step {}",
                            step.index, missing_dependency
                        ),
                    },
                    completed_steps: checkpoints.len(),
                    total_steps: plan.steps.len(),
                    started_at,
                    finished_at: now,
                    checkpoints,
                    outputs,
                });
            }

            let mut last_error = None;
            let mut result = None;
            let step_budget = plan
                .policy
                .max_step_duration_secs
                .filter(|value| *value > 0)
                .map(Duration::from_secs);
            let step_total_budget = plan
                .policy
                .max_step_total_duration_secs
                .filter(|value| *value > 0)
                .map(Duration::from_secs);
            let step_started_at = Instant::now();
            for attempt in 0..=plan.policy.max_retries_per_step {
                let request = ContinuousStepRequest {
                    task_id: plan.id,
                    objective: plan.objective.clone(),
                    worker_role: plan.worker_role.clone(),
                    step: step.clone(),
                    previous_summary: previous_summary.clone(),
                    recent_checkpoint_summaries: recent_checkpoint_summaries(&checkpoints, 8),
                    attempt,
                    previous_error: last_error.clone(),
                    contract: plan.contract.clone(),
                };
                sink.record_step_attempt(
                    &request,
                    "running",
                    "continuous step attempt started",
                    None,
                )
                .await?;
                let step_result = if step_budget.is_some() || step_total_budget.is_some() {
                    let total_remaining = if let Some(total_budget) = step_total_budget {
                        let elapsed = step_started_at.elapsed();
                        if elapsed >= total_budget {
                            last_error = Some(format!(
                                "step {} exceeded its {}s total execution budget",
                                step.index,
                                total_budget.as_secs()
                            ));
                            if let Some(error) = last_error.as_deref() {
                                sink.record_step_attempt(&request, "failed", error, None)
                                    .await?;
                            }
                            break;
                        }
                        Some(total_budget.saturating_sub(elapsed))
                    } else {
                        None
                    };
                    let effective_budget = match (step_budget, total_remaining) {
                        (Some(step_budget), Some(total_remaining)) => {
                            Some(step_budget.min(total_remaining))
                        }
                        (Some(step_budget), None) => Some(step_budget),
                        (None, Some(total_remaining)) => Some(total_remaining),
                        (None, None) => None,
                    };
                    match effective_budget {
                        Some(effective_budget) => {
                            match tokio::time::timeout(
                                effective_budget,
                                runner.run_step(request.clone()),
                            )
                            .await
                            {
                                Ok(result) => result,
                                Err(_) => {
                                    last_error = Some(format!(
                                        "step {} attempt {} exceeded its {}s execution budget",
                                        step.index,
                                        attempt + 1,
                                        effective_budget.as_secs()
                                    ));
                                    if let Some(error) = last_error.as_deref() {
                                        sink.record_step_attempt(&request, "failed", error, None)
                                            .await?;
                                    }
                                    continue;
                                }
                            }
                        }
                        None => runner.run_step(request.clone()).await,
                    }
                } else {
                    runner.run_step(request.clone()).await
                };
                match step_result {
                    Ok(value) => {
                        if let Some(reason) = continuous_step_quality_blocker(&value.output) {
                            let error = format!(
                                "step {} produced invalid continuous output: {}",
                                step.index, reason
                            );
                            sink.record_step_attempt(
                                &request,
                                "rejected",
                                &error,
                                Some(&value.output),
                            )
                            .await?;
                            last_error = Some(error);
                            continue;
                        }
                        if let Some(reason) = continuous_step_declared_blocker(&value.output) {
                            if attempt < plan.policy.max_retries_per_step
                                && continuous_step_declared_blocker_is_recoverable(&value.output)
                            {
                                sink.record_step_attempt(
                                    &request,
                                    "rejected",
                                    &reason,
                                    Some(&value.output),
                                )
                                .await?;
                                last_error = Some(reason);
                                continue;
                            }
                            sink.record_step_attempt(
                                &request,
                                "blocked",
                                &reason,
                                Some(&value.output),
                            )
                            .await?;
                            let now = Utc::now();
                            outputs.push(value.output);
                            return Ok(ContinuousTaskRun {
                                task_id: plan.id,
                                status: ContinuousTaskStatus::Blocked { reason },
                                completed_steps: checkpoints.len(),
                                total_steps: plan.steps.len(),
                                started_at,
                                finished_at: now,
                                checkpoints,
                                outputs,
                            });
                        }
                        result = Some(value);
                        break;
                    }
                    Err(error) => {
                        if let Some(blocker) = error.downcast_ref::<ContinuousStepBlocker>() {
                            sink.record_step_attempt(
                                &request,
                                "blocked",
                                blocker.reason(),
                                Some(blocker.output()),
                            )
                            .await?;
                            let now = Utc::now();
                            outputs.push(blocker.output().to_string());
                            return Ok(ContinuousTaskRun {
                                task_id: plan.id,
                                status: ContinuousTaskStatus::Blocked {
                                    reason: blocker.reason().to_string(),
                                },
                                completed_steps: checkpoints.len(),
                                total_steps: plan.steps.len(),
                                started_at,
                                finished_at: now,
                                checkpoints,
                                outputs,
                            });
                        }
                        let error = error.to_string();
                        sink.record_step_attempt(&request, "failed", &error, None)
                            .await?;
                        if is_recoverable_provider_disconnect(&error) {
                            let now = Utc::now();
                            return Ok(ContinuousTaskRun {
                                task_id: plan.id,
                                status: ContinuousTaskStatus::Paused {
                                    reason: provider_service_pause_reason(&error),
                                },
                                completed_steps: checkpoints.len(),
                                total_steps: plan.steps.len(),
                                started_at,
                                finished_at: now,
                                checkpoints,
                                outputs,
                            });
                        }
                        last_error = Some(error);
                    }
                }
            }

            let Some(result) = result else {
                let now = Utc::now();
                let reason = last_error
                    .unwrap_or_else(|| "step failed without an error message".to_string());
                let status = if is_recoverable_provider_disconnect(&reason) {
                    ContinuousTaskStatus::Paused {
                        reason: provider_service_pause_reason(&reason),
                    }
                } else {
                    ContinuousTaskStatus::Failed { reason }
                };
                return Ok(ContinuousTaskRun {
                    task_id: plan.id,
                    status,
                    completed_steps: checkpoints.len(),
                    total_steps: plan.steps.len(),
                    started_at,
                    finished_at: now,
                    checkpoints,
                    outputs,
                });
            };

            if plan.policy.stop_on_exact_repeat
                && previous_output
                    .as_deref()
                    .is_some_and(|previous| previous == result.output)
            {
                let now = Utc::now();
                return Ok(ContinuousTaskRun {
                    task_id: plan.id,
                    status: ContinuousTaskStatus::Blocked {
                        reason: format!(
                            "exact repeated output at step {}; stopped to avoid a loop",
                            step.index
                        ),
                    },
                    completed_steps: checkpoints.len(),
                    total_steps: plan.steps.len(),
                    started_at,
                    finished_at: now,
                    checkpoints,
                    outputs,
                });
            }

            let artifact_uri = sink
                .record_step(
                    &request_for_checkpoint(&plan, &step, &previous_summary),
                    &result,
                )
                .await?
                .or_else(|| result.artifact_uri.clone());

            previous_summary = Some(result.summary.clone());
            previous_output = Some(result.output.clone());
            outputs.push(result.output);
            checkpoints.push(ContinuousTaskCheckpoint {
                step: step.index,
                label: step.label,
                recorded_at: Utc::now(),
                summary: result.summary,
                artifact_uri,
            });
        }

        let now = Utc::now();
        let status = if plan.steps.len() > max_steps {
            ContinuousTaskStatus::Blocked {
                reason: format!(
                    "step budget exhausted: completed {} of {} planned steps",
                    max_steps,
                    plan.steps.len()
                ),
            }
        } else {
            ContinuousTaskStatus::Completed
        };

        Ok(ContinuousTaskRun {
            task_id: plan.id,
            status,
            completed_steps: checkpoints.len(),
            total_steps: plan.steps.len(),
            started_at,
            finished_at: now,
            checkpoints,
            outputs,
        })
    }
}

fn continuous_step_quality_blocker(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Some("empty output".to_string());
    }

    if repeated_separator_ratio(trimmed) > 180 {
        return Some("output is dominated by repeated separators".to_string());
    }

    let max_same_char_run = max_consecutive_same_char_run(trimmed);
    if max_same_char_run >= 80 {
        return Some(format!(
            "single character repeated {} times consecutively",
            max_same_char_run
        ));
    }

    if let Some(reason) = repeated_line_blocker(trimmed) {
        return Some(reason);
    }

    if let Some(reason) = repeated_token_blocker(trimmed) {
        return Some(reason);
    }

    None
}

fn continuous_step_declared_blocker(output: &str) -> Option<String> {
    let lowered = output.to_ascii_lowercase();
    let declared_blocked = lowered.contains("status: blocked")
        || lowered.contains("\"status\":\"blocked\"")
        || lowered.contains("\"status\": \"blocked\"")
        || lowered.contains("runtime_effect: artifact.needs_revision")
        || lowered.contains("\"runtime_effect\":\"artifact.needs_revision\"")
        || lowered.contains("runtime_effect: artifact.metadata_needs_repair")
        || lowered.contains("\"runtime_effect\":\"artifact.metadata_needs_repair\"");
    if !declared_blocked {
        return None;
    }
    Some(
        output
            .lines()
            .find(|line| {
                let lowered = line.to_ascii_lowercase();
                lowered.starts_with("blockers:")
                    || lowered.starts_with("status:")
                    || lowered.starts_with("runtime_effect:")
            })
            .unwrap_or("continuous step declared a blocked artifact state")
            .trim()
            .to_string(),
    )
}

fn continuous_step_declared_blocker_is_recoverable(output: &str) -> bool {
    let lowered = output.to_ascii_lowercase();
    lowered.contains("runtime_effect: artifact.needs_revision")
        || lowered.contains("\"runtime_effect\":\"artifact.needs_revision\"")
        || lowered.contains("runtime_effect: artifact.metadata_needs_repair")
        || lowered.contains("\"runtime_effect\":\"artifact.metadata_needs_repair\"")
}

fn repeated_separator_ratio(text: &str) -> u16 {
    let mut total = 0usize;
    let mut separators = 0usize;
    for ch in text.chars() {
        total += 1;
        if matches!(ch, '_' | '|' | '-' | '=' | '~') {
            separators += 1;
        }
    }
    if total == 0 {
        0
    } else {
        ((separators * 1_000) / total).min(1_000) as u16
    }
}

fn max_consecutive_same_char_run(text: &str) -> usize {
    let mut previous = None;
    let mut current = 0usize;
    let mut best = 0usize;
    for ch in text.chars() {
        if Some(ch) == previous {
            current += 1;
        } else {
            previous = Some(ch);
            current = 1;
        }
        best = best.max(current);
    }
    best
}

fn repeated_line_blocker(text: &str) -> Option<String> {
    use std::collections::HashMap;

    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| line.chars().count() >= 8)
        .collect::<Vec<_>>();
    if lines.len() < 6 {
        return None;
    }

    let mut counts = HashMap::new();
    let mut best = 0usize;
    for line in &lines {
        let count = counts
            .entry(*line)
            .and_modify(|count| *count += 1)
            .or_insert(1);
        best = best.max(*count);
    }
    if best >= 4 && best * 1_000 / lines.len() >= 600 {
        return Some(format!(
            "line-level repetition detected: {} of {} substantive lines are identical",
            best,
            lines.len()
        ));
    }
    None
}

fn repeated_token_blocker(text: &str) -> Option<String> {
    use std::collections::HashMap;

    let tokens = repetition_tokens(text);
    if tokens.len() < 80 {
        return None;
    }

    let mut best_token = "";
    let mut best_count = 0usize;
    let mut counts = HashMap::new();
    for token in &tokens {
        let count = counts
            .entry(*token)
            .and_modify(|count| *count += 1)
            .or_insert(1);
        if *count > best_count {
            best_count = *count;
            best_token = token;
        }
    }

    let ratio_per_mille = best_count * 1_000 / tokens.len();
    let unique_count = counts.len();
    if best_count >= 24 && ratio_per_mille >= 250 {
        return Some(format!(
            "token-level repetition detected: '{}' appears {} times in {} tokens",
            best_token,
            best_count,
            tokens.len()
        ));
    }
    if tokens.len() >= 120 && unique_count <= tokens.len() / 12 {
        return Some(format!(
            "low-diversity repeated output detected: {} unique tokens across {} tokens",
            unique_count,
            tokens.len()
        ));
    }

    None
}

fn repetition_tokens(text: &str) -> Vec<&str> {
    text.split(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                '-' | '_'
                    | '|'
                    | '/'
                    | '\\'
                    | ','
                    | '.'
                    | ';'
                    | ':'
                    | '，'
                    | '。'
                    | '；'
                    | '：'
                    | '、'
                    | '！'
                    | '？'
                    | '('
                    | ')'
                    | '（'
                    | '）'
                    | '['
                    | ']'
                    | '【'
                    | '】'
                    | '{'
                    | '}'
            )
    })
    .map(str::trim)
    .filter(|token| token.chars().count() >= 2)
    .collect()
}

fn runtime_fingerprint(value: &serde_json::Value) -> String {
    let encoded = serde_json::to_vec(value).unwrap_or_default();
    format!("{:016x}", seahash::hash(&encoded))
}

fn compact_runtime_preview(value: &str, max_chars: usize) -> String {
    let mut preview = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

fn request_for_checkpoint(
    plan: &ContinuousTaskPlan,
    step: &ContinuousTaskStep,
    previous_summary: &Option<String>,
) -> ContinuousStepRequest {
    ContinuousStepRequest {
        task_id: plan.id,
        objective: plan.objective.clone(),
        worker_role: plan.worker_role.clone(),
        step: step.clone(),
        previous_summary: previous_summary.clone(),
        recent_checkpoint_summaries: Vec::new(),
        attempt: 0,
        previous_error: None,
        contract: plan.contract.clone(),
    }
}

fn recent_checkpoint_summaries(
    checkpoints: &[ContinuousTaskCheckpoint],
    limit: usize,
) -> Vec<String> {
    let start = checkpoints.len().saturating_sub(limit);
    checkpoints[start..]
        .iter()
        .map(|checkpoint| {
            format!(
                "{}. {}: {}",
                checkpoint.step, checkpoint.label, checkpoint.summary
            )
        })
        .collect()
}

pub fn task_state_from_continuous_run(
    plan: &ContinuousTaskPlan,
    run: &ContinuousTaskRun,
    agent_id: impl Into<String>,
    session_id: Option<String>,
    thread_id: Option<String>,
) -> benshu_state::TaskState {
    let mut task = benshu_state::TaskState::new(
        "continuous_task",
        plan.objective.clone(),
        serde_json::json!({
            "entrypoint": "brain.runtime.continuous_task",
            "worker_role": plan.worker_role,
            "planned_steps": plan.steps.len(),
            "artifact_target": plan.artifact_target,
            "contract": plan.contract,
        }),
        agent_id,
    );
    task.id = plan.id;
    task.created_at = run.started_at;
    task.updated_at = run.finished_at;
    task.current_step = run.completed_steps as u32;
    task.total_steps = Some(run.total_steps as u32);
    task.session_id = session_id;
    task.thread_id = thread_id;
    task.tags = vec!["continuous".to_string(), "checkpointed".to_string()];
    task.status = match &run.status {
        ContinuousTaskStatus::Completed => benshu_state::TaskStatus::Completed,
        ContinuousTaskStatus::Paused { .. } => benshu_state::TaskStatus::Paused(run.finished_at),
        ContinuousTaskStatus::Blocked { reason } => benshu_state::TaskStatus::Blocked {
            reason: reason.clone(),
        },
        ContinuousTaskStatus::Failed { reason } => benshu_state::TaskStatus::Failed(reason.clone()),
    };
    task.checkpoints = run
        .checkpoints
        .iter()
        .map(|checkpoint| benshu_state::TaskCheckpoint {
            step: checkpoint.step as u32,
            label: checkpoint.label.clone(),
            recorded_at: checkpoint.recorded_at,
            summary: Some(checkpoint.summary.clone()),
        })
        .collect();
    if let ContinuousTaskStatus::Paused { reason } = &run.status {
        task.checkpoints.push(benshu_state::TaskCheckpoint {
            step: run.completed_steps as u32,
            label: "continuous_task:paused:provider_service".to_string(),
            recorded_at: run.finished_at,
            summary: Some(reason.clone()),
        });
    }
    task.artifacts = run
        .checkpoints
        .iter()
        .filter_map(|checkpoint| {
            checkpoint
                .artifact_uri
                .as_ref()
                .map(|uri| benshu_state::TaskArtifactRef {
                    artifact_id: format!("continuous:{}:{}", plan.id, checkpoint.step),
                    kind: plan
                        .artifact_target
                        .as_ref()
                        .map(|target| target.kind.clone())
                        .unwrap_or_else(|| "continuous_output".to_string()),
                    uri: uri.clone(),
                    media_type: plan
                        .artifact_target
                        .as_ref()
                        .and_then(|target| target.media_type.clone()),
                })
        })
        .collect();
    task.result = Some(serde_json::json!({
        "status": run.status,
        "completed_steps": run.completed_steps,
        "total_steps": run.total_steps,
        "final_summary": run.final_summary(),
    }));
    task
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoRunner;

    #[async_trait]
    impl ContinuousStepRunner for EchoRunner {
        async fn run_step(
            &mut self,
            request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            Ok(ContinuousStepResult {
                output: format!("output:{}", request.step.index),
                summary: format!("completed {}", request.step.label),
                artifact_uri: Some(format!("memory://step/{}", request.step.index)),
            })
        }
    }

    #[tokio::test]
    async fn continuous_executor_runs_bounded_steps_and_checkpoints() {
        let plan = ContinuousTaskPlan::new("process three files", "coder")
            .with_steps(vec![
                ContinuousTaskStep {
                    index: 1,
                    label: "file-a".to_string(),
                    instruction: "process a".to_string(),
                    expected_output: None,
                    depends_on: Vec::new(),
                    action: ContinuousStepAction::default(),
                },
                ContinuousTaskStep {
                    index: 2,
                    label: "file-b".to_string(),
                    instruction: "process b".to_string(),
                    expected_output: None,
                    depends_on: Vec::new(),
                    action: ContinuousStepAction::default(),
                },
            ])
            .with_artifact_target(ContinuousArtifactTarget {
                uri: "memory://batch".to_string(),
                kind: "batch_output".to_string(),
                media_type: Some("text/plain".to_string()),
            });

        let run = ContinuousTaskExecutor
            .run(plan.clone(), &mut EchoRunner)
            .await
            .expect("continuous task should run");

        assert_eq!(run.completed_steps, 2);
        assert!(matches!(run.status, ContinuousTaskStatus::Completed));
        assert_eq!(run.checkpoints.len(), 2);

        let task = task_state_from_continuous_run(
            &plan,
            &run,
            "benshu",
            Some("session-a".to_string()),
            Some("thread-a".to_string()),
        );
        assert_eq!(task.current_step, 2);
        assert_eq!(task.artifacts.len(), 2);
        assert!(matches!(task.status, benshu_state::TaskStatus::Completed));

        let report = run.completion_report();
        assert_eq!(report.completed_steps, 2);
        assert_eq!(report.artifacts.len(), 2);
        assert!(report.final_summary.contains("file-b"));
    }

    #[tokio::test]
    async fn persistent_checkpoint_sink_records_runtime_events_for_completion_gate() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let state =
            benshu_state::StateProvider::new(temp.path().join("state.redb")).expect("state");
        let contract = ContinuousTaskContract {
            invariants: Vec::new(),
            anchors: Vec::new(),
            completion_criteria: vec!["checkpoint and write an artifact".to_string()],
            required_events: vec![
                "continuous.step.checkpointed".to_string(),
                "artifact.written".to_string(),
            ],
            completion_event: None,
        };
        let plan = ContinuousTaskPlan::new("evented task", "worker")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "write".to_string(),
                instruction: "write".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_contract(contract);

        state
            .tasks
            .save(benshu_state::TaskState {
                id: plan.id,
                ..benshu_state::TaskState::new(
                    "evented",
                    "evented",
                    serde_json::json!({}),
                    "worker",
                )
            })
            .await
            .expect("save task");

        let mut runner = EchoRunner;
        let mut sink = PersistentTaskCheckpointSink::new(
            state.tasks.clone(),
            plan.id,
            NoopContinuousCheckpointSink,
        )
        .with_event_manager(state.runtime_events.clone());
        let run = ContinuousTaskExecutor
            .run_with_checkpoint_sink(plan.clone(), &mut runner, &mut sink)
            .await
            .expect("run");
        let events = state
            .runtime_events
            .list_by_task(plan.id)
            .await
            .expect("events");

        assert!(events
            .iter()
            .any(|event| event.topic == "continuous.step.checkpointed"));
        assert!(events.iter().any(|event| event.topic == "artifact.written"));
        assert_eq!(
            continuous_completion_gate_decision(&plan, &run, &events),
            ContinuousCompletionGateDecision::Complete
        );
    }

    struct RepeatRunner;

    #[async_trait]
    impl ContinuousStepRunner for RepeatRunner {
        async fn run_step(
            &mut self,
            request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            Ok(ContinuousStepResult {
                output: "same".to_string(),
                summary: format!("step {}", request.step.index),
                artifact_uri: None,
            })
        }
    }

    #[tokio::test]
    async fn continuous_executor_stops_exact_repeat_loops() {
        let plan = ContinuousTaskPlan::new("repeat guard", "worker").with_steps(vec![
            ContinuousTaskStep {
                index: 1,
                label: "first".to_string(),
                instruction: "first".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            },
            ContinuousTaskStep {
                index: 2,
                label: "second".to_string(),
                instruction: "second".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            },
        ]);

        let run = ContinuousTaskExecutor
            .run(plan, &mut RepeatRunner)
            .await
            .expect("continuous task should return blocked run");

        assert_eq!(run.completed_steps, 1);
        assert!(matches!(run.status, ContinuousTaskStatus::Blocked { .. }));
    }

    struct DegenerateRunner;

    #[async_trait]
    impl ContinuousStepRunner for DegenerateRunner {
        async fn run_step(
            &mut self,
            request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            let repeated = "章节-报告-框架-".repeat(120);
            Ok(ContinuousStepResult {
                output: format!("### 第{}步\n\n{repeated}", request.step.index),
                summary: "degenerate".to_string(),
                artifact_uri: None,
            })
        }
    }

    #[tokio::test]
    async fn continuous_executor_rejects_degenerate_repetition_before_checkpoint() {
        let plan = ContinuousTaskPlan::new("degenerate guard", "worker")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "bad".to_string(),
                instruction: "write".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 1,
                max_retries_per_step: 0,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(300),
                max_step_total_duration_secs: None,
            });

        let run = ContinuousTaskExecutor
            .run(plan, &mut DegenerateRunner)
            .await
            .expect("continuous task should return failed run");

        assert_eq!(run.completed_steps, 0);
        assert!(matches!(run.status, ContinuousTaskStatus::Failed { .. }));
    }

    struct BlockedArtifactRunner;

    #[async_trait]
    impl ContinuousStepRunner for BlockedArtifactRunner {
        async fn run_step(
            &mut self,
            request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            let output = if request.step.index == 1 {
                "status: blocked\nruntime_effect: artifact.needs_revision\nblockers: draft requires revision".to_string()
            } else {
                "status: completed".to_string()
            };
            Ok(ContinuousStepResult {
                summary: output.clone(),
                output,
                artifact_uri: None,
            })
        }
    }

    #[tokio::test]
    async fn continuous_executor_stops_when_step_declares_blocked_artifact_state() {
        let plan = ContinuousTaskPlan::new("blocked artifact", "writer")
            .with_steps(vec![
                ContinuousTaskStep {
                    index: 1,
                    label: "chapter-1".to_string(),
                    instruction: "write chapter 1".to_string(),
                    expected_output: None,
                    depends_on: Vec::new(),
                    action: ContinuousStepAction::default(),
                },
                ContinuousTaskStep {
                    index: 2,
                    label: "chapter-2".to_string(),
                    instruction: "write chapter 2".to_string(),
                    expected_output: None,
                    depends_on: Vec::new(),
                    action: ContinuousStepAction::default(),
                },
            ])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 2,
                max_retries_per_step: 0,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(300),
                max_step_total_duration_secs: None,
            });

        let run = ContinuousTaskExecutor
            .run(plan, &mut BlockedArtifactRunner)
            .await
            .expect("continuous task should return a blocked run");

        assert_eq!(run.completed_steps, 0);
        assert_eq!(run.outputs.len(), 1);
        assert!(run.final_summary().contains("draft requires revision"));
        assert!(matches!(run.status, ContinuousTaskStatus::Blocked { .. }));
    }

    struct RecoverableArtifactRunner {
        calls: usize,
    }

    #[async_trait]
    impl ContinuousStepRunner for RecoverableArtifactRunner {
        async fn run_step(
            &mut self,
            request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            self.calls += 1;
            if request.attempt < 2 {
                return Ok(ContinuousStepResult {
                    summary: "chapter metadata still needs repair".to_string(),
                    output: "status: blocked\nruntime_effect: artifact.metadata_needs_repair\nblockers: chapter metadata still needs repair".to_string(),
                    artifact_uri: None,
                });
            }
            Ok(ContinuousStepResult {
                summary: "chapter approved".to_string(),
                output: "status: completed\nruntime_effect: artifact.verified".to_string(),
                artifact_uri: None,
            })
        }
    }

    #[tokio::test]
    async fn continuous_executor_retries_recoverable_artifact_quality_blockers() {
        let plan = ContinuousTaskPlan::new("repair artifact", "writer")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "chapter-1".to_string(),
                instruction: "write chapter 1".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 1,
                max_retries_per_step: 2,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(300),
                max_step_total_duration_secs: None,
            });

        let mut runner = RecoverableArtifactRunner { calls: 0 };
        let run = ContinuousTaskExecutor
            .run(plan, &mut runner)
            .await
            .expect("recoverable artifact blocker should be retried");

        assert_eq!(runner.calls, 3);
        assert_eq!(run.completed_steps, 1);
        assert!(matches!(run.status, ContinuousTaskStatus::Completed));
    }

    struct ProviderDisconnectRunner {
        calls: usize,
    }

    #[async_trait]
    impl ContinuousStepRunner for ProviderDisconnectRunner {
        async fn run_step(
            &mut self,
            _request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            self.calls += 1;
            anyhow::bail!(
                "Internal error: error sending request for url (http://127.0.0.1/v1/chat/completions)"
            )
        }
    }

    #[tokio::test]
    async fn continuous_executor_pauses_on_provider_disconnect() {
        let plan = ContinuousTaskPlan::new("provider outage", "worker")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "step".to_string(),
                instruction: "work".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 1,
                max_retries_per_step: 10,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(300),
                max_step_total_duration_secs: None,
            });

        let mut runner = ProviderDisconnectRunner { calls: 0 };
        let run = ContinuousTaskExecutor
            .run(plan.clone(), &mut runner)
            .await
            .expect("continuous task should pause cleanly");

        assert_eq!(
            runner.calls, 1,
            "provider failures must not be retried in a hot loop"
        );
        assert_eq!(run.completed_steps, 0);
        match &run.status {
            ContinuousTaskStatus::Paused { reason } => {
                assert!(reason.contains("model provider service disconnected"));
            }
            other => panic!("expected paused status, got {other:?}"),
        }

        let task = task_state_from_continuous_run(&plan, &run, "benshu", None, None);
        assert!(matches!(task.status, benshu_state::TaskStatus::Paused(_)));
        assert!(task
            .checkpoints
            .iter()
            .any(|checkpoint| checkpoint.label == "continuous_task:paused:provider_service"));
    }

    struct SlowRunner;

    #[async_trait]
    impl ContinuousStepRunner for SlowRunner {
        async fn run_step(
            &mut self,
            _request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            Ok(ContinuousStepResult {
                output: "late output".to_string(),
                summary: "late".to_string(),
                artifact_uri: None,
            })
        }
    }

    #[tokio::test]
    async fn continuous_executor_fails_step_that_exceeds_step_budget() {
        let plan = ContinuousTaskPlan::new("bounded step", "worker")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "slow".to_string(),
                instruction: "work".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 1,
                max_retries_per_step: 0,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(0),
                max_step_total_duration_secs: None,
            });
        let run = ContinuousTaskExecutor
            .run(plan, &mut SlowRunner)
            .await
            .expect("zero budget disables timeout");
        assert!(matches!(run.status, ContinuousTaskStatus::Completed));

        let plan = ContinuousTaskPlan::new("bounded step", "worker")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "slow".to_string(),
                instruction: "work".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 1,
                max_retries_per_step: 0,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(1),
                max_step_total_duration_secs: None,
            });
        let run = ContinuousTaskExecutor
            .run(plan, &mut SlowRunner)
            .await
            .expect("continuous task should return failed run");
        assert_eq!(run.completed_steps, 0);
        match run.status {
            ContinuousTaskStatus::Failed { reason } => {
                assert!(reason.contains("exceeded its 1s execution budget"));
            }
            other => panic!("expected failed status, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn continuous_executor_step_budget_applies_per_attempt() {
        let plan = ContinuousTaskPlan::new("bounded retry step", "worker")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "slow".to_string(),
                instruction: "work".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 1,
                max_retries_per_step: 1,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(1),
                max_step_total_duration_secs: None,
            });
        let started = Instant::now();
        let run = ContinuousTaskExecutor
            .run(plan, &mut SlowRunner)
            .await
            .expect("continuous task should return failed run");

        assert_eq!(run.completed_steps, 0);
        assert!(
            started.elapsed() >= Duration::from_millis(1800),
            "each retry should receive its own attempt budget"
        );
        match run.status {
            ContinuousTaskStatus::Failed { reason } => {
                assert!(reason.contains("attempt 2 exceeded its 1s execution budget"));
            }
            other => panic!("expected failed status, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn continuous_executor_optional_total_step_budget_covers_retries() {
        let plan = ContinuousTaskPlan::new("bounded retry step", "worker")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "slow".to_string(),
                instruction: "work".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 1,
                max_retries_per_step: 2,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(1),
                max_step_total_duration_secs: Some(1),
            });
        let started = Instant::now();
        let run = ContinuousTaskExecutor
            .run(plan, &mut SlowRunner)
            .await
            .expect("continuous task should return failed run");

        assert_eq!(run.completed_steps, 0);
        assert!(
            started.elapsed() < Duration::from_millis(1800),
            "explicit total step budget should cap retries"
        );
        match run.status {
            ContinuousTaskStatus::Failed { reason } => {
                assert!(reason.contains("total execution budget"));
            }
            other => panic!("expected failed status, got {other:?}"),
        }
    }

    struct RetryFeedbackRunner {
        attempts: Vec<(
            usize,
            Option<String>,
            Vec<String>,
            Option<ContinuousTaskContract>,
        )>,
    }

    #[async_trait]
    impl ContinuousStepRunner for RetryFeedbackRunner {
        async fn run_step(
            &mut self,
            request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            self.attempts.push((
                request.attempt,
                request.previous_error.clone(),
                request.recent_checkpoint_summaries.clone(),
                request.contract.clone(),
            ));
            if request.step.index == 2 && request.attempt == 0 {
                anyhow::bail!("simulated drift: primary anchor changed");
            }
            Ok(ContinuousStepResult {
                output: format!("corrected output {}", request.step.index),
                summary: format!("corrected {}", request.step.index),
                artifact_uri: None,
            })
        }
    }

    #[tokio::test]
    async fn continuous_executor_surfaces_validation_feedback_to_retry() {
        let contract = ContinuousTaskContract {
            invariants: vec!["keep the locked identity".to_string()],
            anchors: vec![ContinuousTaskAnchor {
                name: "subject".to_string(),
                value: "alpha".to_string(),
            }],
            completion_criteria: vec!["finish both steps".to_string()],
            required_events: Vec::new(),
            completion_event: None,
        };
        let plan = ContinuousTaskPlan::new("retry with feedback", "worker")
            .with_steps(vec![
                ContinuousTaskStep {
                    index: 1,
                    label: "first".to_string(),
                    instruction: "first".to_string(),
                    expected_output: None,
                    depends_on: Vec::new(),
                    action: ContinuousStepAction::default(),
                },
                ContinuousTaskStep {
                    index: 2,
                    label: "second".to_string(),
                    instruction: "second".to_string(),
                    expected_output: None,
                    depends_on: vec![1],
                    action: ContinuousStepAction::default(),
                },
            ])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 2,
                max_retries_per_step: 1,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(300),
                max_step_total_duration_secs: None,
            })
            .with_contract(contract.clone());
        let mut runner = RetryFeedbackRunner {
            attempts: Vec::new(),
        };

        let run = ContinuousTaskExecutor
            .run(plan, &mut runner)
            .await
            .expect("retry should recover");

        assert!(matches!(run.status, ContinuousTaskStatus::Completed));
        assert_eq!(run.completed_steps, 2);
        assert_eq!(runner.attempts.len(), 3);
        assert_eq!(runner.attempts[2].0, 1);
        assert!(runner.attempts[2]
            .1
            .as_deref()
            .is_some_and(|error| error.contains("simulated drift")));
        assert!(runner.attempts[2]
            .2
            .iter()
            .any(|summary| summary.contains("first")));
        assert_eq!(runner.attempts[2].3, Some(contract));
    }

    #[derive(Default)]
    struct AttemptRecordingSink {
        attempts: Vec<(usize, usize, String, String)>,
    }

    #[async_trait]
    impl ContinuousCheckpointSink for AttemptRecordingSink {
        async fn record_step(
            &mut self,
            _request: &ContinuousStepRequest,
            _result: &ContinuousStepResult,
        ) -> anyhow::Result<Option<String>> {
            Ok(None)
        }

        async fn record_step_attempt(
            &mut self,
            request: &ContinuousStepRequest,
            status: &str,
            reason: &str,
            _output_preview: Option<&str>,
        ) -> anyhow::Result<()> {
            self.attempts.push((
                request.step.index,
                request.attempt,
                status.to_string(),
                reason.to_string(),
            ));
            Ok(())
        }
    }

    struct QualityRetryRunner;

    #[async_trait]
    impl ContinuousStepRunner for QualityRetryRunner {
        async fn run_step(
            &mut self,
            request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            let output = if request.attempt == 0 {
                "x".repeat(90)
            } else {
                "recovered useful output".to_string()
            };
            Ok(ContinuousStepResult {
                output,
                summary: format!("attempt {}", request.attempt),
                artifact_uri: None,
            })
        }
    }

    struct DurableBlockerRunner {
        calls: usize,
    }

    #[async_trait]
    impl ContinuousStepRunner for DurableBlockerRunner {
        async fn run_step(
            &mut self,
            _request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            self.calls += 1;
            Err(ContinuousStepBlocker::new("artifact needs revision", "artifact://draft/1").into())
        }
    }

    #[tokio::test]
    async fn continuous_executor_does_not_retry_or_complete_durable_blockers() {
        let plan = ContinuousTaskPlan::new("write one artifact", "writer")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "draft".to_string(),
                instruction: "draft".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 1,
                max_retries_per_step: 10,
                ..ContinuousTaskPolicy::default()
            });
        let mut runner = DurableBlockerRunner { calls: 0 };
        let mut sink = AttemptRecordingSink::default();

        let run = ContinuousTaskExecutor
            .run_with_checkpoint_sink(plan, &mut runner, &mut sink)
            .await
            .expect("a durable blocker is a valid task outcome");

        assert_eq!(runner.calls, 1);
        assert_eq!(run.completed_steps, 0);
        assert_eq!(run.outputs, vec!["artifact://draft/1"]);
        assert!(matches!(
            run.status,
            ContinuousTaskStatus::Blocked { ref reason } if reason == "artifact needs revision"
        ));
        assert_eq!(sink.attempts.len(), 2);
        assert_eq!(sink.attempts[1].2, "blocked");
    }

    #[tokio::test]
    async fn continuous_executor_records_rejected_attempts() {
        let plan = ContinuousTaskPlan::new("retry diagnostics", "worker")
            .with_steps(vec![ContinuousTaskStep {
                index: 1,
                label: "draft".to_string(),
                instruction: "draft".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            }])
            .with_policy(ContinuousTaskPolicy {
                max_steps: 1,
                max_retries_per_step: 1,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(300),
                max_step_total_duration_secs: None,
            });
        let mut runner = QualityRetryRunner;
        let mut sink = AttemptRecordingSink::default();

        let run = ContinuousTaskExecutor
            .run_with_checkpoint_sink(plan, &mut runner, &mut sink)
            .await
            .expect("retry should recover");

        assert!(matches!(run.status, ContinuousTaskStatus::Completed));
        assert_eq!(sink.attempts.len(), 3);
        assert_eq!(sink.attempts[0].0, 1);
        assert_eq!(sink.attempts[0].1, 0);
        assert_eq!(sink.attempts[0].2, "running");
        assert_eq!(sink.attempts[1].0, 1);
        assert_eq!(sink.attempts[1].1, 0);
        assert_eq!(sink.attempts[1].2, "rejected");
        assert!(sink.attempts[1]
            .3
            .contains("single character repeated 90 times"));
        assert_eq!(sink.attempts[2].0, 1);
        assert_eq!(sink.attempts[2].1, 1);
        assert_eq!(sink.attempts[2].2, "running");
    }

    #[tokio::test]
    async fn continuous_executor_blocks_unfinished_dependencies() {
        let plan = ContinuousTaskPlan::new("dependency guard", "worker").with_steps(vec![
            ContinuousTaskStep {
                index: 2,
                label: "second".to_string(),
                instruction: "second".to_string(),
                expected_output: None,
                depends_on: vec![1],
                action: ContinuousStepAction::default(),
            },
            ContinuousTaskStep {
                index: 1,
                label: "first".to_string(),
                instruction: "first".to_string(),
                expected_output: None,
                depends_on: Vec::new(),
                action: ContinuousStepAction::default(),
            },
        ]);

        let run = ContinuousTaskExecutor
            .run(plan, &mut EchoRunner)
            .await
            .expect("continuous task should return blocked run");

        assert_eq!(run.completed_steps, 0);
        assert!(matches!(run.status, ContinuousTaskStatus::Blocked { .. }));
    }

    struct RecordingActionHandler {
        seen: Vec<String>,
    }

    #[async_trait]
    impl ContinuousActionHandler for RecordingActionHandler {
        async fn execute_action(
            &mut self,
            action: ContinuousStepAction,
            request: ContinuousStepRequest,
        ) -> anyhow::Result<ContinuousStepResult> {
            let label = match action {
                ContinuousStepAction::Custom { action, .. } => action,
                other => format!("{other:?}"),
            };
            self.seen.push(label.clone());
            Ok(ContinuousStepResult {
                output: format!("{}:{}", request.step.index, label),
                summary: format!("ran {label}"),
                artifact_uri: Some(format!("memory://{}", label)),
            })
        }
    }

    #[tokio::test]
    async fn continuous_action_runner_dispatches_composite_actions() {
        let step = ContinuousTaskStep {
            index: 1,
            label: "compound".to_string(),
            instruction: "run two generic actions".to_string(),
            expected_output: None,
            depends_on: Vec::new(),
            action: ContinuousStepAction::Composite {
                actions: vec![
                    ContinuousStepAction::Custom {
                        action: "first".to_string(),
                        payload: serde_json::json!({}),
                    },
                    ContinuousStepAction::Custom {
                        action: "second".to_string(),
                        payload: serde_json::json!({}),
                    },
                ],
            },
        };
        let request = ContinuousStepRequest {
            task_id: Uuid::new_v4(),
            objective: "test generic dispatch".to_string(),
            worker_role: "worker".to_string(),
            step,
            previous_summary: None,
            recent_checkpoint_summaries: Vec::new(),
            attempt: 0,
            previous_error: None,
            contract: None,
        };
        let handler = RecordingActionHandler { seen: Vec::new() };
        let mut runner = ContinuousActionRunner::new(handler);

        let result = runner
            .run_step(request)
            .await
            .expect("composite action should run");
        let handler = runner.into_inner();

        assert_eq!(handler.seen, vec!["first", "second"]);
        assert!(result.output.contains("1:first"));
        assert!(result.output.contains("1:second"));
        assert_eq!(result.artifact_uri, Some("memory://second".to_string()));
    }

    #[tokio::test]
    async fn continuous_executor_can_checkpoint_each_step_to_file() {
        let path = std::env::temp_dir().join(format!(
            "benshu-continuous-checkpoint-{}.txt",
            Uuid::new_v4()
        ));
        let plan = ContinuousTaskPlan::new("append two outputs", "writer")
            .with_steps(vec![
                ContinuousTaskStep {
                    index: 1,
                    label: "one".to_string(),
                    instruction: "write one".to_string(),
                    expected_output: None,
                    depends_on: Vec::new(),
                    action: ContinuousStepAction::default(),
                },
                ContinuousTaskStep {
                    index: 2,
                    label: "two".to_string(),
                    instruction: "write two".to_string(),
                    expected_output: None,
                    depends_on: Vec::new(),
                    action: ContinuousStepAction::default(),
                },
            ])
            .with_artifact_target(ContinuousArtifactTarget {
                uri: path.to_string_lossy().to_string(),
                kind: "text".to_string(),
                media_type: Some("text/plain".to_string()),
            });

        let mut sink = FileAppendCheckpointSink::new(&path).with_separator("\n---\n");
        sink.initialize("# header\n").await.expect("init sink");
        let run = ContinuousTaskExecutor
            .run_with_checkpoint_sink(plan, &mut EchoRunner, &mut sink)
            .await
            .expect("continuous task should run with file sink");

        let content = tokio::fs::read_to_string(&path)
            .await
            .expect("checkpoint file should exist");
        let _ = tokio::fs::remove_file(&path).await;

        assert_eq!(run.completed_steps, 2);
        assert!(content.contains("# header"));
        assert!(content.contains("output:1"));
        assert!(content.contains("output:2"));
        assert_eq!(
            run.checkpoints
                .last()
                .and_then(|checkpoint| checkpoint.artifact_uri.clone()),
            Some(path.to_string_lossy().to_string())
        );
    }

    #[test]
    fn refresh_declared_artifact_progress_updates_header_without_rewriting_total() {
        let content = "# 《测试》\n- **当前进度**：第 1 / _278_ 步\n\n### 第一章\n\n正文\n";
        let refreshed = refresh_declared_artifact_progress_text(content, 7);

        assert!(refreshed.contains("- **当前进度**：第 7 / _278_ 步"));
        assert!(refreshed.contains("### 第一章"));
    }

    #[test]
    fn refresh_declared_artifact_progress_updates_english_header() {
        let content = "# Test\n- **Current Progress**: 1 / _12_ steps\n\n## Step 1\nBody\n";
        let refreshed = refresh_declared_artifact_progress_text(content, 3);

        assert!(refreshed.contains("- **Current Progress**: 3 / _12_ steps"));
        assert!(refreshed.contains("## Step 1"));
    }
}
