use crate::findings::{
    collect_truth_verification_scorecard_findings, collect_windows_native_scorecard_findings,
};
use crate::runtime_contract::{
    append_metadata_notes, append_nonzero_metadata_notes, HOOK_RUNTIME_NOTE_PROJECTIONS,
    RUNTIME_NOTE_PROJECTIONS, SESSION_RUNTIME_NOTE_PROJECTIONS,
};
use crate::skill_loading::append_skill_loading_notes as append_skill_loading_metadata_notes;
use crate::trace::{RunReplay, RunTrace, TraceStatus, WitnessSummary};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptFailureKind {
    RuntimeStageFailed,
    ToolFailed,
    Cancelled,
    TimedOut,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeFailureKind {
    MissingReplay,
    MissingTaskLink,
    MissingSessionLink,
    EmptyExecution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalTask {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    pub suite_id: String,
    pub scenario: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalTrial {
    pub trial_id: Uuid,
    pub run_id: Uuid,
    pub trace_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalOutcome {
    pub verdict: String,
    pub score: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_failure: Option<TranscriptFailureKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_failure: Option<OutcomeFailureKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failure_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BenchmarkFingerprint {
    pub suite_id: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WitnessBundle {
    pub witness_id: Uuid,
    pub run_id: Uuid,
    pub trace_id: Uuid,
    pub task: EvalTask,
    pub trial: EvalTrial,
    pub replay: RunReplay,
    pub outcome: EvalOutcome,
    pub benchmark_fingerprint: BenchmarkFingerprint,
    pub generated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScorecardEntry {
    pub witness_id: Uuid,
    pub run_id: Uuid,
    pub verdict: String,
    pub score: f32,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scorecard {
    pub scorecard_id: String,
    pub suite_id: String,
    pub total_trials: usize,
    pub passed_trials: usize,
    pub warned_trials: usize,
    pub failed_trials: usize,
    pub average_score: f32,
    pub updated_at: DateTime<Utc>,
    pub benchmark_fingerprint: BenchmarkFingerprint,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<ScorecardEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WitnessLogEntry {
    pub witness_id: Uuid,
    pub run_id: Uuid,
    pub trace_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    pub suite_id: String,
    pub scenario: String,
    pub verdict: String,
    pub score: f32,
    pub replayable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_path: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failure_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_failure: Option<TranscriptFailureKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_failure: Option<OutcomeFailureKind>,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub budget_exhausted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub recorded_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RealHarnessCase {
    pub suite_id: String,
    pub scenario: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RealHarnessResult {
    pub case: RealHarnessCase,
    pub witness: WitnessBundle,
    pub scorecard: Scorecard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RealHarnessSuiteResult {
    pub suite_id: String,
    pub total_cases: usize,
    pub results: Vec<RealHarnessResult>,
    pub scorecard: Scorecard,
}

pub struct SimulationHarness;

pub struct RealHarness;

impl SimulationHarness {
    pub fn build_witness_bundle(run_trace: &RunTrace, suite_id: &str) -> WitnessBundle {
        build_witness_bundle_with_task(
            run_trace,
            EvalTask {
                task_id: run_trace.task_id,
                suite_id: suite_id.to_string(),
                scenario: classify_scenario(run_trace),
                thread_id: run_trace.thread_id.clone(),
            },
            "simulation_harness",
        )
    }

    pub fn witness_summary(bundle: &WitnessBundle) -> WitnessSummary {
        WitnessSummary {
            witness_id: bundle.witness_id,
            run_id: Some(bundle.run_id),
            verdict: bundle.outcome.verdict.clone(),
            scorecard: Some(serde_json::json!({
                "score": bundle.outcome.score,
                "suite_id": bundle.task.suite_id,
                "transcript_failure": bundle.outcome.transcript_failure,
                "outcome_failure": bundle.outcome.outcome_failure,
            })),
            replayable: bundle.replay.replayable,
            benchmark_fingerprint: Some(bundle.benchmark_fingerprint.fingerprint.clone()),
            notes: bundle.notes.clone(),
        }
    }

    pub fn upsert_scorecard(existing: Option<Scorecard>, bundle: &WitnessBundle) -> Scorecard {
        let mut entries = existing
            .map(|scorecard| scorecard.entries)
            .unwrap_or_default();
        entries.retain(|entry| entry.witness_id != bundle.witness_id);
        entries.push(ScorecardEntry {
            witness_id: bundle.witness_id,
            run_id: bundle.run_id,
            verdict: bundle.outcome.verdict.clone(),
            score: bundle.outcome.score,
            recorded_at: bundle.generated_at,
        });
        entries.sort_by(|left, right| right.recorded_at.cmp(&left.recorded_at));

        let total_trials = entries.len();
        let passed_trials = entries
            .iter()
            .filter(|entry| entry.verdict == "pass")
            .count();
        let warned_trials = entries
            .iter()
            .filter(|entry| entry.verdict == "warn")
            .count();
        let failed_trials = entries
            .iter()
            .filter(|entry| entry.verdict == "fail")
            .count();
        let average_score = if total_trials == 0 {
            0.0
        } else {
            entries.iter().map(|entry| entry.score).sum::<f32>() / total_trials as f32
        };

        Scorecard {
            scorecard_id: bundle.task.suite_id.clone(),
            suite_id: bundle.task.suite_id.clone(),
            total_trials,
            passed_trials,
            warned_trials,
            failed_trials,
            average_score,
            updated_at: bundle.generated_at,
            benchmark_fingerprint: bundle.benchmark_fingerprint.clone(),
            entries,
        }
    }

    pub fn build_witness_log_entry(
        run_trace: &RunTrace,
        bundle: &WitnessBundle,
    ) -> WitnessLogEntry {
        let fallback_reason = run_trace.degradation_notes.iter().find_map(|note| {
            note.strip_prefix("provider_fallback:")
                .map(|s| s.to_string())
        });
        let budget_exhausted = bundle
            .outcome
            .failure_reasons
            .iter()
            .chain(run_trace.degradation_notes.iter())
            .any(|reason| reason.contains("budget"));

        WitnessLogEntry {
            witness_id: bundle.witness_id,
            run_id: bundle.run_id,
            trace_id: bundle.trace_id,
            task_id: bundle.task.task_id,
            suite_id: bundle.task.suite_id.clone(),
            scenario: bundle.task.scenario.clone(),
            verdict: bundle.outcome.verdict.clone(),
            score: bundle.outcome.score,
            replayable: bundle.replay.replayable,
            provider: run_trace.provider.clone(),
            model: run_trace.model.clone(),
            route: run_trace.metadata.get("route").cloned(),
            context_artifacts: run_trace
                .artifacts
                .iter()
                .map(|artifact| artifact.artifact_id.clone())
                .collect(),
            tool_path: run_trace
                .tools
                .iter()
                .map(|tool| tool.tool_name.clone())
                .collect(),
            failure_reasons: bundle.outcome.failure_reasons.clone(),
            transcript_failure: bundle.outcome.transcript_failure.clone(),
            outcome_failure: bundle.outcome.outcome_failure.clone(),
            degraded: !run_trace.degradation_notes.is_empty()
                || run_trace.tools.iter().any(|tool| tool.degraded),
            budget_exhausted,
            policy_decision: run_trace.metadata.get("policy_decision").cloned(),
            fallback_reason,
            recorded_at: bundle.generated_at,
            metadata: run_trace.metadata.clone(),
        }
    }
}

impl RealHarness {
    pub async fn run_case<F, Fut>(
        case: RealHarnessCase,
        run: F,
        existing_scorecard: Option<Scorecard>,
    ) -> anyhow::Result<RealHarnessResult>
    where
        F: FnOnce(&RealHarnessCase) -> Fut,
        Fut: Future<Output = anyhow::Result<RunTrace>>,
    {
        let run_trace = run(&case).await?;
        Ok(build_real_harness_result(
            case,
            run_trace,
            existing_scorecard,
        ))
    }

    pub async fn run_suite<F, Fut>(
        cases: Vec<RealHarnessCase>,
        mut run: F,
    ) -> anyhow::Result<RealHarnessSuiteResult>
    where
        F: FnMut(RealHarnessCase) -> Fut,
        Fut: Future<Output = anyhow::Result<RunTrace>>,
    {
        let Some(first_case) = cases.first() else {
            anyhow::bail!("real harness suite requires at least one case");
        };
        let suite_id = first_case.suite_id.clone();
        let total_cases = cases.len();
        let mut scorecard = None;
        let mut results = Vec::with_capacity(total_cases);

        for case in cases {
            if case.suite_id != suite_id {
                anyhow::bail!(
                    "real harness suite expected suite_id '{}' but got '{}'",
                    suite_id,
                    case.suite_id
                );
            }

            let run_trace = run(case.clone()).await?;
            let result = build_real_harness_result(case, run_trace, scorecard.take());
            scorecard = Some(result.scorecard.clone());
            results.push(result);
        }

        let scorecard = scorecard.expect("suite with at least one case should produce scorecard");
        Ok(RealHarnessSuiteResult {
            suite_id,
            total_cases,
            results,
            scorecard,
        })
    }
}

fn build_real_harness_result(
    case: RealHarnessCase,
    run_trace: RunTrace,
    existing_scorecard: Option<Scorecard>,
) -> RealHarnessResult {
    let witness = build_witness_bundle_with_task(
        &run_trace,
        EvalTask {
            task_id: run_trace.task_id,
            suite_id: case.suite_id.clone(),
            scenario: case.scenario.clone(),
            thread_id: case
                .thread_id
                .clone()
                .or_else(|| run_trace.thread_id.clone()),
        },
        "real_harness",
    );
    let scorecard = SimulationHarness::upsert_scorecard(existing_scorecard, &witness);

    RealHarnessResult {
        case,
        witness,
        scorecard,
    }
}

fn build_witness_bundle_with_task(
    run_trace: &RunTrace,
    task: EvalTask,
    harness_label: &str,
) -> WitnessBundle {
    let replay = run_trace.to_replay();
    let transcript_failure = classify_transcript_failure(run_trace);
    let outcome_failure = classify_outcome_failure(run_trace, &replay);
    let mut failure_reasons = Vec::new();
    let windows_native_findings = collect_windows_native_scorecard_findings(run_trace);
    let truth_verification_findings = collect_truth_verification_scorecard_findings(run_trace);

    if let Some(kind) = transcript_failure.as_ref() {
        failure_reasons.push(format!("transcript::{kind:?}").to_lowercase());
    }
    if let Some(kind) = outcome_failure.as_ref() {
        failure_reasons.push(format!("outcome::{kind:?}").to_lowercase());
    }
    failure_reasons.extend(run_trace.degradation_notes.iter().cloned());
    failure_reasons.extend(
        windows_native_findings
            .iter()
            .map(|finding| finding.reason.clone()),
    );
    failure_reasons.extend(
        truth_verification_findings
            .iter()
            .map(|finding| finding.reason.clone()),
    );

    let verdict = if transcript_failure.is_some() || outcome_failure.is_some() {
        "fail"
    } else if !run_trace.degradation_notes.is_empty()
        || !windows_native_findings.is_empty()
        || !truth_verification_findings.is_empty()
    {
        "warn"
    } else {
        "pass"
    }
    .to_string();

    let mut score = 1.0_f32;
    if transcript_failure.is_some() {
        score -= 0.45;
    }
    if outcome_failure.is_some() {
        score -= 0.35;
    }
    if !run_trace.degradation_notes.is_empty() {
        score -= 0.1;
    }
    if windows_native_findings.iter().any(|finding| finding.severe) {
        score -= 0.1;
    } else if !windows_native_findings.is_empty() {
        score -= 0.05;
    }
    if truth_verification_findings
        .iter()
        .any(|finding| finding.severe)
    {
        score -= 0.1;
    } else if !truth_verification_findings.is_empty() {
        score -= 0.05;
    }
    let score = score.clamp(0.0, 1.0);

    let fingerprint = build_benchmark_fingerprint(run_trace, &task.suite_id);
    let witness_id = run_trace
        .witness
        .as_ref()
        .map(|witness| witness.witness_id)
        .unwrap_or_else(Uuid::new_v4);
    let mut notes = vec![harness_label.to_string()];
    append_skill_loading_notes(&mut notes, run_trace);
    append_runtime_middleware_notes(&mut notes, run_trace);

    WitnessBundle {
        witness_id,
        run_id: run_trace.run_id,
        trace_id: run_trace.run_id,
        task,
        trial: EvalTrial {
            trial_id: Uuid::new_v4(),
            run_id: run_trace.run_id,
            trace_id: run_trace.run_id,
            task_id: run_trace.task_id,
            started_at: run_trace.started_at,
            finished_at: run_trace.finished_at,
        },
        replay,
        outcome: EvalOutcome {
            verdict,
            score,
            transcript_failure,
            outcome_failure,
            failure_reasons,
        },
        benchmark_fingerprint: fingerprint,
        generated_at: Utc::now(),
        notes,
    }
}

fn append_skill_loading_notes(notes: &mut Vec<String>, run_trace: &RunTrace) {
    append_skill_loading_metadata_notes(notes, &run_trace.metadata);
}

fn append_runtime_middleware_notes(notes: &mut Vec<String>, run_trace: &RunTrace) {
    append_metadata_notes(notes, &run_trace.metadata, SESSION_RUNTIME_NOTE_PROJECTIONS);
    append_nonzero_metadata_notes(notes, &run_trace.metadata, HOOK_RUNTIME_NOTE_PROJECTIONS);
    append_metadata_notes(notes, &run_trace.metadata, RUNTIME_NOTE_PROJECTIONS);
}

fn classify_transcript_failure(run_trace: &RunTrace) -> Option<TranscriptFailureKind> {
    match run_trace.status {
        TraceStatus::Cancelled => Some(TranscriptFailureKind::Cancelled),
        TraceStatus::TimedOut => Some(TranscriptFailureKind::TimedOut),
        TraceStatus::Degraded => Some(TranscriptFailureKind::Degraded),
        TraceStatus::Failed => {
            if run_trace
                .tools
                .iter()
                .any(|tool| matches!(tool.status, TraceStatus::Failed))
            {
                Some(TranscriptFailureKind::ToolFailed)
            } else {
                Some(TranscriptFailureKind::RuntimeStageFailed)
            }
        }
        _ => {
            if run_trace
                .tools
                .iter()
                .any(|tool| matches!(tool.status, TraceStatus::Failed))
            {
                Some(TranscriptFailureKind::ToolFailed)
            } else if run_trace
                .stages
                .iter()
                .any(|stage| matches!(stage.status, TraceStatus::Failed))
            {
                Some(TranscriptFailureKind::RuntimeStageFailed)
            } else if !run_trace.degradation_notes.is_empty() {
                Some(TranscriptFailureKind::Degraded)
            } else {
                None
            }
        }
    }
}

fn classify_outcome_failure(
    run_trace: &RunTrace,
    replay: &RunReplay,
) -> Option<OutcomeFailureKind> {
    if !replay.replayable {
        Some(OutcomeFailureKind::MissingReplay)
    } else if run_trace.task_id.is_none() {
        Some(OutcomeFailureKind::MissingTaskLink)
    } else if run_trace.session_id.is_nil() {
        Some(OutcomeFailureKind::MissingSessionLink)
    } else if run_trace.stages.is_empty() && run_trace.tools.is_empty() {
        Some(OutcomeFailureKind::EmptyExecution)
    } else {
        None
    }
}

fn classify_scenario(run_trace: &RunTrace) -> String {
    if run_trace.metadata.get("handover").map(String::as_str) == Some("true") {
        "delegation_flow".to_string()
    } else if run_trace
        .stages
        .iter()
        .any(|stage| stage.stage.label() == "Governance")
    {
        "foreground_runtime".to_string()
    } else {
        "runtime_smoke".to_string()
    }
}

fn build_benchmark_fingerprint(run_trace: &RunTrace, suite_id: &str) -> BenchmarkFingerprint {
    let mut digest = Sha256::new();
    digest.update(suite_id.as_bytes());
    digest.update(run_trace.agent_id.as_bytes());
    digest.update(run_trace.status.status_string().as_bytes());
    for stage in &run_trace.stages {
        digest.update(stage.stage.label().as_bytes());
        digest.update(stage.status.status_string().as_bytes());
    }
    for tool in &run_trace.tools {
        digest.update(tool.tool_name.as_bytes());
        digest.update(tool.status.status_string().as_bytes());
    }

    BenchmarkFingerprint {
        suite_id: suite_id.to_string(),
        fingerprint: format!("{:x}", digest.finalize()),
    }
}

trait TraceStatusLabel {
    fn status_string(&self) -> &'static str;
}

impl TraceStatusLabel for TraceStatus {
    fn status_string(&self) -> &'static str {
        match self {
            TraceStatus::Started => "started",
            TraceStatus::Succeeded => "succeeded",
            TraceStatus::Failed => "failed",
            TraceStatus::Cancelled => "cancelled",
            TraceStatus::Degraded => "degraded",
            TraceStatus::TimedOut => "timed_out",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{AgentTracer, RuntimeStage, RuntimeStageTrace};

    #[test]
    fn simulation_harness_builds_witness_bundle_and_scorecard() {
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let mut run_trace = tracer.start_run_trace();
        run_trace.task_id = Some(Uuid::new_v4());
        run_trace.metadata.insert(
            "matched_skill_manuals".to_string(),
            "deep-research".to_string(),
        );
        run_trace.metadata.insert(
            "matched_skill_assets".to_string(),
            "references/query-plan.md".to_string(),
        );
        run_trace.metadata.insert(
            "read_skill_manuals".to_string(),
            "deep-research".to_string(),
        );
        run_trace.metadata.insert(
            "skill_surface_classifications".to_string(),
            "deep-research:executable".to_string(),
        );
        run_trace.metadata.insert(
            "skill_surface_executions".to_string(),
            "deep-research:runtime".to_string(),
        );
        run_trace.metadata.insert(
            "skill_surface_runtimes".to_string(),
            "deep-research:uv".to_string(),
        );
        run_trace.metadata.insert(
            "skill_surface_kinds".to_string(),
            "deep-research:tool".to_string(),
        );
        run_trace.metadata.insert(
            "skill_surface_contract_happened".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "read_skill_assets".to_string(),
            "deep-research:references/query-plan.md".to_string(),
        );
        run_trace.metadata.insert(
            "skill_asset_followups".to_string(),
            "deep-research:references/query-plan.md:shell".to_string(),
        );
        run_trace.metadata.insert(
            "skill_asset_execution_surfaces".to_string(),
            "deep-research:references/query-plan.md:runtime:shell".to_string(),
        );
        run_trace
            .metadata
            .insert("skill_manual_gate_active".to_string(), "true".to_string());
        run_trace
            .metadata
            .insert("skill_asset_gate_active".to_string(), "true".to_string());
        run_trace
            .metadata
            .insert("skill_manual_read_happened".to_string(), "true".to_string());
        run_trace
            .metadata
            .insert("skill_asset_read_happened".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "skill_asset_followup_happened".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "skill_surface_contract_core_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "skill_surface_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "skill_asset_execution_surface_happened".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("session_title".to_string(), "Deep Research".to_string());
        run_trace.metadata.insert(
            "session_title_source".to_string(),
            "extra_params.session_title".to_string(),
        );
        run_trace
            .metadata
            .insert("truth_status".to_string(), "Verified".to_string());
        run_trace.metadata.insert(
            "verification_domain".to_string(),
            "KnowledgeFact".to_string(),
        );
        run_trace.metadata.insert(
            "verification_requirement".to_string(),
            "Required".to_string(),
        );
        run_trace.metadata.insert(
            "verification_mode".to_string(),
            "WebSearchFetch".to_string(),
        );
        run_trace.metadata.insert(
            "verification_outcome".to_string(),
            "VerificationSucceeded".to_string(),
        );
        run_trace.metadata.insert(
            "verification_answer_readiness".to_string(),
            "search_results_only".to_string(),
        );
        run_trace.metadata.insert(
            "verification_next_tools".to_string(),
            "web_fetch".to_string(),
        );
        run_trace
            .metadata
            .insert("verification_cite_required".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "verification_followup_note".to_string(),
            "Search results were observed, but source pages were not fetched yet.".to_string(),
        );
        run_trace.metadata.insert(
            "truth_verification_guidance_active".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("source_posture".to_string(), "SourcesAttached".to_string());
        run_trace.metadata.insert(
            "verification_last_tool".to_string(),
            "web_search".to_string(),
        );
        run_trace.metadata.insert(
            "verification_tools".to_string(),
            "web_fetch,web_search".to_string(),
        );
        run_trace
            .metadata
            .insert("verification_source_count".to_string(), "2".to_string());
        run_trace.metadata.insert(
            "verification_execution_evidence_count".to_string(),
            "1".to_string(),
        );
        run_trace.metadata.insert(
            "verification_state_evidence_count".to_string(),
            "2".to_string(),
        );
        run_trace
            .metadata
            .insert("verification_note_count".to_string(), "1".to_string());
        run_trace.metadata.insert(
            "verification_surface_note_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "verification_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("hook_memory_surface_count".to_string(), "1".to_string());
        run_trace
            .metadata
            .insert("hook_subagent_surface_count".to_string(), "1".to_string());
        run_trace
            .metadata
            .insert("hook_title_surface_count".to_string(), "1".to_string());
        run_trace.metadata.insert(
            "hook_summarization_surface_count".to_string(),
            "1".to_string(),
        );
        run_trace
            .metadata
            .insert("hook_media_surface_count".to_string(), "1".to_string());
        run_trace
            .metadata
            .insert("tactical_slm_present".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "tactical_slm_model_id".to_string(),
            "api:openai/test-small-model".to_string(),
        );
        run_trace.metadata.insert(
            "tactical_slm_factory_id".to_string(),
            "cloud_llm".to_string(),
        );
        run_trace
            .metadata
            .insert("tactical_slm_source".to_string(), "cloud".to_string());
        run_trace
            .metadata
            .insert("tactical_slm_roles".to_string(), "llm,slm".to_string());
        run_trace.metadata.insert(
            "tactical_slm_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("background_present".to_string(), "true".to_string());
        run_trace
            .metadata
            .insert("background_revision".to_string(), "3".to_string());
        run_trace
            .metadata
            .insert("background_previous_revision".to_string(), "2".to_string());
        run_trace.metadata.insert(
            "background_update_reason".to_string(),
            "post_response_background_refresh".to_string(),
        );
        run_trace.metadata.insert(
            "background_quality_signal".to_string(),
            "stable".to_string(),
        );
        run_trace
            .metadata
            .insert("background_persona_present".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "background_relationship_present".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("background_session_present".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "background_recent_window_present".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("background_source_ref_count".to_string(), "4".to_string());
        run_trace.metadata.insert(
            "background_compression_reason".to_string(),
            "session_continuity_pressure".to_string(),
        );
        run_trace.metadata.insert(
            "background_decision".to_string(),
            "promoterelationshipfact".to_string(),
        );
        run_trace
            .metadata
            .insert("background_used_slm".to_string(), "false".to_string());
        run_trace
            .metadata
            .insert("background_total_attempts".to_string(), "3".to_string());
        run_trace
            .metadata
            .insert("background_skip_count".to_string(), "1".to_string());
        run_trace
            .metadata
            .insert("background_reject_count".to_string(), "0".to_string());
        run_trace.metadata.insert(
            "background_refresh_session_count".to_string(),
            "1".to_string(),
        );
        run_trace.metadata.insert(
            "background_promote_relationship_count".to_string(),
            "1".to_string(),
        );
        run_trace
            .metadata
            .insert("background_rewrite_count".to_string(), "0".to_string());
        run_trace.metadata.insert(
            "background_session_persistence_status".to_string(),
            "persisted".to_string(),
        );
        run_trace.metadata.insert(
            "background_durable_promotion_pending".to_string(),
            "false".to_string(),
        );
        run_trace.metadata.insert(
            "background_durable_promotion_status".to_string(),
            "pending_review".to_string(),
        );
        run_trace.metadata.insert(
            "background_review_reason".to_string(),
            "background_relationship_candidate".to_string(),
        );
        run_trace.metadata.insert(
            "background_review_source".to_string(),
            "background_compression:default".to_string(),
        );
        run_trace.metadata.insert(
            "background_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_host_runtime".to_string(),
            "linux_validation".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_deployment_lane".to_string(),
            "validation_only".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_deployment_strategy".to_string(),
            "switch_to_windows_native_host".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_deployment_note".to_string(),
            "Current host is only for validation; Windows-native deployment remains the product mainline."
                .to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_product_mainline".to_string(),
            "windows_native_mainline".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_validation_tracks".to_string(),
            "wsl2,linux_rocm_smoke".to_string(),
        );
        run_trace
            .metadata
            .insert("windows_native_priority".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "windows_native_small_model_runtime_target".to_string(),
            "onnx_runtime_directml_winml".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_small_model_execution_linked".to_string(),
            "false".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_small_model_execution_provider".to_string(),
            "validation_only".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_small_model_device_target".to_string(),
            "windows_native_accelerator".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_small_model_fallback_mode".to_string(),
            "validation_only".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_small_model_runtime_outcome".to_string(),
            "validation_only".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_small_model_runtime_strategy".to_string(),
            "validation_host_only".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_small_model_runtime_readiness".to_string(),
            "validation_only".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_small_model_runtime_reason".to_string(),
            "WSL2/Linux paths are validation-only; product mainline is native Windows.".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_main_brain_runtime_target".to_string(),
            "llama.cpp".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_runtime_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "windows_native_runtime_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_outcome".to_string(),
            "fallback_runtime_active".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_class".to_string(),
            "fallback_runtime".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_provider".to_string(),
            "directml_winml".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_device_target".to_string(),
            "windows_native_accelerator".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_fallback_mode".to_string(),
            "cpu_fallback_with_explicit_reason".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_strategy".to_string(),
            "migrate_to_windows_native_runtime".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_note".to_string(),
            "Embedding currently runs through the fallback runtime.".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_outcome".to_string(),
            "windows_native_active".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_class".to_string(),
            "active".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_provider".to_string(),
            "directml_winml".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_device_target".to_string(),
            "windows_native_accelerator".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_fallback_mode".to_string(),
            "cpu_fallback_with_explicit_reason".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_strategy".to_string(),
            "active".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_note".to_string(),
            "Rerank executed through the Windows-native small-model runtime.".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_surface_note_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "deferred_tool_filter_active".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_tools".to_string(),
            "normalize_audio".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_statuses".to_string(),
            "normalize_audio:ok".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_kinds".to_string(),
            "normalize_audio:audio".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_inputs".to_string(),
            "normalize_audio:/tmp/input.wav".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_outputs".to_string(),
            "normalize_audio:file:/tmp/output.wav".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_engines".to_string(),
            "normalize_audio:ffmpeg".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_cleanup".to_string(),
            "normalize_audio:false".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_surface_note_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_artifact_registered".to_string(),
            "normalize_audio:true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_artifact_source_kinds".to_string(),
            "normalize_audio:builtin_tool_output".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_artifact_kinds".to_string(),
            "normalize_audio:normalized_audio_output".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_artifact_uris".to_string(),
            "normalize_audio:/tmp/output.wav".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_source_kinds".to_string(),
            "image_page_raster:direct_image,pdf_parse_tool:page_image_ocr:pdf_page_image"
                .to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_source_refs".to_string(),
            "image_page_raster:/tmp/screenshot.png,pdf_parse_tool:page_image_ocr:pdf_page:3"
                .to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_consumed_by".to_string(),
            "image_page_raster:ocr,normalize_audio:stt".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_consumption_routes".to_string(),
            "image_page_raster:ocr_backend,normalize_audio:media_runtime_audio_stt".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_outcomes".to_string(),
            "extract_video_frames:preprocess_failed,image_page_raster:model_result_insufficient,normalize_audio:model_failed_after_preprocess"
                .to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_preprocess_failed_routes".to_string(),
            "extract_video_frames".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_model_failed_routes".to_string(),
            "normalize_audio".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_result_insufficient_routes".to_string(),
            "image_page_raster".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_followup_strategies".to_string(),
            "extract_video_frames:attachment_fallback,image_page_raster:clarification_or_manual_review,normalize_audio:alternate_model_fallback"
                .to_string(),
        );
        run_trace.metadata.insert(
            "media_followup_strategies".to_string(),
            "extract_video_frames:attachment_fallback,image_page_raster:clarification_or_manual_review,normalize_audio:alternate_model_fallback"
                .to_string(),
        );
        run_trace.metadata.insert(
            "media_followup_guidance_active".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_followup_capability_route".to_string(),
            "document_understanding".to_string(),
        );
        run_trace.metadata.insert(
            "media_followup_execution_surface".to_string(),
            "document_understanding_alternate_model_fallback".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_attachment_fallback_routes".to_string(),
            "extract_video_frames".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_alternate_model_fallback_routes".to_string(),
            "normalize_audio".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_clarification_routes".to_string(),
            "image_page_raster".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_artifact_surface_note_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_artifact_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_contract_core_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_artifact_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_consumption_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_consumption_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_outcome_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_outcome_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_strategy_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "media_preprocess_strategy_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "session_status".to_string(),
            "awaiting_clarification".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_prompt".to_string(),
            "你想查哪个城市的天气？".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_original_request".to_string(),
            "帮我查一下天气".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_status_kind".to_string(),
            "awaiting_clarification".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_session_status_json_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_session_status_json_valid".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_contract_core_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_surface_note_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_awaiting_seen".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_event".to_string(),
            "status_surface".to_string(),
        );
        run_trace.metadata.insert(
            "clarification_status_surface".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("deferred_tool_visible_count".to_string(), "6".to_string());
        run_trace
            .metadata
            .insert("deferred_tool_total_count".to_string(), "14".to_string());
        run_trace
            .metadata
            .insert("deferred_tool_deferred_count".to_string(), "8".to_string());
        run_trace.metadata.insert(
            "deferred_tool_surface_note_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "deferred_tool_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("tool_error_tools".to_string(), "web_search".to_string());
        run_trace.metadata.insert(
            "tool_error_surface_tools".to_string(),
            "web_search".to_string(),
        );
        run_trace
            .metadata
            .insert("tool_error_surface_present".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "tool_error_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "forge_registered_tools".to_string(),
            "pdf_builder".to_string(),
        );
        run_trace
            .metadata
            .insert("forge_source".to_string(), "forge".to_string());
        run_trace
            .metadata
            .insert("forge_scope".to_string(), "session".to_string());
        run_trace.metadata.insert(
            "forge_followup_candidates".to_string(),
            "pdf_builder".to_string(),
        );
        run_trace
            .metadata
            .insert("forge_followup_gate_active".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "forge_execution_surfaces".to_string(),
            "pdf_builder:runtime".to_string(),
        );
        run_trace.metadata.insert(
            "forge_capability_domains".to_string(),
            "pdf_builder:runtime_surface".to_string(),
        );
        run_trace.metadata.insert(
            "forge_smoke_statuses".to_string(),
            "pdf_builder:passed".to_string(),
        );
        run_trace.metadata.insert(
            "forge_smoke_latency_ms".to_string(),
            "pdf_builder:42".to_string(),
        );
        run_trace.metadata.insert(
            "forge_cleanup_recorded".to_string(),
            "pdf_builder:true".to_string(),
        );
        run_trace
            .metadata
            .insert("forge_surface_present".to_string(), "true".to_string());
        run_trace
            .metadata
            .insert("forge_contract_complete".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "forge_followup_tools".to_string(),
            "pdf_builder".to_string(),
        );
        run_trace.metadata.insert(
            "forge_followup_execution_happened".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("forge_closed_loop_complete".to_string(), "true".to_string());
        run_trace
            .metadata
            .insert("degraded_tool_names".to_string(), "web_fetch".to_string());
        run_trace
            .metadata
            .insert("loop_guard_tools".to_string(), "shell".to_string());
        run_trace.metadata.insert(
            "runtime_finish_reason".to_string(),
            "tool_calls".to_string(),
        );
        run_trace
            .metadata
            .insert("provider_name".to_string(), "capture".to_string());
        run_trace
            .metadata
            .insert("provider_model".to_string(), "gpt-4.1-mini".to_string());
        run_trace
            .metadata
            .insert("provider_latency_ms".to_string(), "37".to_string());
        run_trace
            .metadata
            .insert("provider_prompt_tokens".to_string(), "120".to_string());
        run_trace
            .metadata
            .insert("provider_completion_tokens".to_string(), "45".to_string());
        run_trace
            .metadata
            .insert("provider_total_tokens".to_string(), "165".to_string());
        run_trace.metadata.insert(
            "provider_finish_reason".to_string(),
            "tool_calls".to_string(),
        );
        run_trace
            .metadata
            .insert("provider_tool_call_count".to_string(), "1".to_string());
        run_trace.metadata.insert(
            "provider_tool_contract_mode".to_string(),
            "tagged_json_tool_calls".to_string(),
        );
        run_trace.metadata.insert(
            "provider_mainline_stability".to_string(),
            "stable".to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_consumed_by".to_string(),
            "normalize_audio:stt,extract_video_frames:vlm".to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_consumption_routes".to_string(),
            "normalize_audio:native_local_stt,extract_video_frames:native_provider_vision"
                .to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_outcomes".to_string(),
            "extract_video_frames:model_failed_after_preprocess,normalize_audio:model_result_insufficient"
                .to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_model_failed_routes".to_string(),
            "extract_video_frames".to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_result_insufficient_routes".to_string(),
            "normalize_audio".to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_followup_strategies".to_string(),
            "extract_video_frames:alternate_model_fallback,normalize_audio:clarification_or_manual_review"
                .to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_alternate_model_fallback_routes".to_string(),
            "extract_video_frames".to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_clarification_routes".to_string(),
            "normalize_audio".to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_outcome_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_outcome_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_strategy_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "provider_media_preprocess_strategy_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "provider_contract_core_complete".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("provider_usage_complete".to_string(), "true".to_string());
        run_trace
            .metadata
            .insert("provider_contract_complete".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "provider_surface_note_core_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "provider_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "post_run_summary".to_string(),
            "thoughts=1,tool_calls=0".to_string(),
        );
        run_trace
            .metadata
            .insert("visible_owner".to_string(), "benshu".to_string());
        run_trace
            .metadata
            .insert("memory_owner".to_string(), "engram".to_string());
        run_trace
            .metadata
            .insert("approval_owner".to_string(), "benshu".to_string());
        run_trace.metadata.insert(
            "memory_session_contract_core_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "memory_session_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "memory_session_surface_core_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "memory_session_surface_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "memory_session_surface_note_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "memory_session_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "subagent_budget_surface_note_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "subagent_budget_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("title_surface_note_present".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "title_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "summarization_surface_note_present".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "summarization_surface_note_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "memory_session_orchestration_contract_core_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "memory_session_orchestration_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "runtime_evidence_contract_core_complete".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "runtime_evidence_contract_complete".to_string(),
            "true".to_string(),
        );
        run_trace
            .metadata
            .insert("delegation_present".to_string(), "false".to_string());
        run_trace
            .metadata
            .insert("handover_present".to_string(), "false".to_string());
        run_trace
            .metadata
            .insert("max_parallel_tools".to_string(), "4".to_string());
        run_trace
            .metadata
            .insert("hook_loop_abort_count".to_string(), "1".to_string());
        run_trace.stages.push(RuntimeStageTrace {
            stage: RuntimeStage::Ingress,
            status: TraceStatus::Succeeded,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            detail: Some("message accepted".to_string()),
            metadata: Default::default(),
        });
        run_trace.finished_at = Some(Utc::now());
        run_trace.status = TraceStatus::Succeeded;

        let bundle = SimulationHarness::build_witness_bundle(&run_trace, "runtime_main_path");
        assert_eq!(bundle.outcome.verdict, "warn");
        assert!(bundle
            .outcome
            .failure_reasons
            .iter()
            .any(|reason| reason == "windows_native::embed::fallback_runtime_active"));
        assert!(bundle.replay.replayable);
        assert_eq!(bundle.task.suite_id, "runtime_main_path");
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_manual_gate_active"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_manual_read_happened"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_asset_read_happened"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_manual_match:deep-research"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_asset_match:references/query-plan.md"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_manual_read:deep-research"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "skill_surface_classification:deep-research:executable" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "skill_surface_execution:deep-research:runtime" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "skill_surface_runtime:deep-research:uv" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "skill_surface_kind:deep-research:tool" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "skill_asset_read:deep-research:references/query-plan.md" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "skill_asset_followup:deep-research:references/query-plan.md:shell"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note
                == "skill_asset_execution_surface:deep-research:references/query-plan.md:runtime:shell"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_asset_gate_active"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_asset_followup_happened"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_asset_execution_surface_happened"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_loading_contract_core_complete"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_loading_contract_complete"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_loading_surface_note_core_complete"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_loading_surface_note_complete"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_surface_contract_core_complete"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "skill_surface_contract_complete"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_matched_skill_manuals:deep-research" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_skill_surface_classifications:deep-research:executable"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_surface_executions:deep-research:runtime" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_surface_runtimes:deep-research:uv" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_surface_kinds:deep-research:tool" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_matched_skill_assets:references/query-plan.md" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_read_skill_manuals:deep-research" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_read_skill_assets:deep-research:references/query-plan.md"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_skill_asset_followups:deep-research:references/query-plan.md:shell"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note
                == "runtime_skill_asset_execution_surfaces:deep-research:references/query-plan.md:runtime:shell"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_manual_read_happened:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_asset_read_happened:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_asset_followup_happened:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_asset_execution_surface_happened:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_loading_contract_core_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_loading_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_loading_surface_note_core_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_loading_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_surface_contract_core_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_skill_surface_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "session_title:Deep Research"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "session_title_source:extra_params.session_title"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_truth_status:Verified"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_verification_domain:KnowledgeFact"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_verification_requirement:Required"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_verification_mode:WebSearchFetch"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_verification_outcome:VerificationSucceeded" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_verification_answer_readiness:search_results_only"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_verification_next_tools:web_fetch"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_verification_cite_required:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_source_posture:SourcesAttached"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_truth_verification_guidance_active:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_verification_last_tool:web_search"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_verification_tools:web_fetch,web_search" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_verification_execution_evidence_count:1" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_verification_state_evidence_count:2" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_verification_surface_note_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_verification_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_title_surface_count:1"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_summarization_surface_count:1"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "deferred_tool_filter_active:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_session_status:awaiting_clarification"));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_clarification_prompt:你想查哪个城市的天气？"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_clarification_original_request:帮我查一下天气"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_clarification_status_kind:awaiting_clarification" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_clarification_session_status_json_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_clarification_session_status_json_valid:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_clarification_contract_core_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_clarification_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_clarification_surface_note_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_clarification_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_clarification_awaiting_seen:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_clarification_event:status_surface"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_clarification_status_surface:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "deferred_tool_visible_count:6"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "deferred_tool_total_count:14"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "deferred_tool_deferred_count:8"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_deferred_tool_surface_note_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_deferred_tool_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tool_error_tools:web_search"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tool_error_surface_tools:web_search"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tool_error_surface_present:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tool_error_contract_complete:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_registered_tools:pdf_builder"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_source:forge"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_scope:session"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_followup_candidates:pdf_builder"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_followup_gate_active:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_forge_execution_surfaces:pdf_builder:runtime" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_forge_capability_domains:pdf_builder:runtime_surface"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_smoke_statuses:pdf_builder:passed"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_smoke_latency_ms:pdf_builder:42"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_cleanup_recorded:pdf_builder:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_surface_present:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_contract_complete:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_followup_tools:pdf_builder"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_forge_followup_execution_happened:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_forge_closed_loop_complete:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_degraded_tool_names:web_fetch"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_loop_guard_tools:shell"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_finish_reason:tool_calls"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tactical_slm_present:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tactical_slm_model_id:api:openai/test-small-model"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tactical_slm_factory_id:cloud_llm"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tactical_slm_source:cloud"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tactical_slm_roles:llm,slm"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_tactical_slm_contract_complete:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_background_present:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_background_revision:3"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_background_previous_revision:2"));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_background_update_reason:post_response_background_refresh"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_background_quality_signal:stable"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_background_persona_present:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_relationship_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_background_session_present:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_recent_window_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_background_source_ref_count:4"));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_background_compression_reason:session_continuity_pressure"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_decision:promoterelationshipfact" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_background_used_slm:false"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_total_attempts:3" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_skip_count:1" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_reject_count:0" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_refresh_session_count:1" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_promote_relationship_count:1" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_rewrite_count:0" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_session_persistence_status:persisted" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_durable_promotion_pending:false" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_durable_promotion_status:pending_review" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_background_review_reason:background_relationship_candidate"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_background_review_source:background_compression:default"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_background_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_windows_native_deployment_lane:validation_only"));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_windows_native_deployment_strategy:switch_to_windows_native_host"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_windows_native_product_mainline:windows_native_mainline"));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_windows_native_small_model_runtime_target:onnx_runtime_directml_winml"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_windows_native_small_model_execution_provider:validation_only"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_windows_native_small_model_device_target:windows_native_accelerator"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_windows_native_small_model_fallback_mode:validation_only"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_windows_native_small_model_runtime_outcome:validation_only"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_windows_native_small_model_runtime_strategy:validation_host_only"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_windows_native_runtime_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_windows_native_runtime_surface_note_complete:true" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_engram_windows_native_embed_outcome:fallback_runtime_active"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_engram_windows_native_embed_class:fallback_runtime"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_engram_windows_native_embed_provider:directml_winml"));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_engram_windows_native_embed_device_target:windows_native_accelerator"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_engram_windows_native_embed_fallback_mode:cpu_fallback_with_explicit_reason"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_engram_windows_native_embed_strategy:migrate_to_windows_native_runtime"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_engram_windows_native_rerank_outcome:windows_native_active"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_engram_windows_native_rerank_class:active"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_engram_windows_native_rerank_provider:directml_winml"));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_engram_windows_native_rerank_device_target:windows_native_accelerator"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_engram_windows_native_rerank_fallback_mode:cpu_fallback_with_explicit_reason"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_engram_windows_native_rerank_strategy:active"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_engram_windows_native_surface_note_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_engram_windows_native_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_media_surface_count:1"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_media_preprocess_tools:normalize_audio"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_media_preprocess_statuses:normalize_audio:ok"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note
                == "runtime_media_preprocess_outputs:normalize_audio:file:/tmp/output.wav"));
        assert!(bundle.notes.iter().any(|note| {
            note
                == "runtime_media_preprocess_source_kinds:image_page_raster:direct_image,pdf_parse_tool:page_image_ocr:pdf_page_image"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note
                == "runtime_media_preprocess_source_refs:image_page_raster:/tmp/screenshot.png,pdf_parse_tool:page_image_ocr:pdf_page:3"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_artifact_kinds:normalize_audio:normalized_audio_output"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_artifact_uris:normalize_audio:/tmp/output.wav"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_consumed_by:image_page_raster:ocr,normalize_audio:stt"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note
                == "runtime_media_preprocess_consumption_routes:image_page_raster:ocr_backend,normalize_audio:media_runtime_audio_stt"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note
                == "runtime_media_preprocess_outcomes:extract_video_frames:preprocess_failed,image_page_raster:model_result_insufficient,normalize_audio:model_failed_after_preprocess"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_preprocess_failed_routes:extract_video_frames"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_model_failed_routes:normalize_audio"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_result_insufficient_routes:image_page_raster"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_followup_strategies:extract_video_frames:attachment_fallback,image_page_raster:clarification_or_manual_review,normalize_audio:alternate_model_fallback"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_followup_strategies:extract_video_frames:attachment_fallback,image_page_raster:clarification_or_manual_review,normalize_audio:alternate_model_fallback"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_media_followup_guidance_active:true" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_followup_capability_route:document_understanding"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_followup_execution_surface:document_understanding_alternate_model_fallback"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_attachment_fallback_routes:extract_video_frames"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_alternate_model_fallback_routes:normalize_audio"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_clarification_routes:image_page_raster"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_media_preprocess_surface_note_complete:true" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_artifact_surface_note_complete:true"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_consumption_surface_note_complete:true"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_media_preprocess_outcome_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_media_preprocess_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_media_preprocess_artifact_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_media_preprocess_consumption_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_media_preprocess_outcome_contract_complete:true" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_media_preprocess_strategy_surface_note_complete:true"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_media_preprocess_strategy_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_name:capture"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_model:gpt-4.1-mini"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_latency_ms:37"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_prompt_tokens:120"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_completion_tokens:45"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_total_tokens:165"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_finish_reason:tool_calls"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_tool_call_count:1"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_tool_contract_mode:tagged_json_tool_calls"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_mainline_stability:stable"));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_consumed_by:normalize_audio:stt,extract_video_frames:vlm"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_consumption_routes:normalize_audio:native_local_stt,extract_video_frames:native_provider_vision"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_outcomes:extract_video_frames:model_failed_after_preprocess,normalize_audio:model_result_insufficient"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_model_failed_routes:extract_video_frames"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_result_insufficient_routes:normalize_audio"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_followup_strategies:extract_video_frames:alternate_model_fallback,normalize_audio:clarification_or_manual_review"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_alternate_model_fallback_routes:extract_video_frames"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_clarification_routes:normalize_audio"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_outcome_note_complete:true"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_outcome_contract_complete:true"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_strategy_note_complete:true"
        }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_provider_media_preprocess_strategy_contract_complete:true"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_contract_core_complete:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_usage_complete:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_provider_contract_complete:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_provider_surface_note_core_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_provider_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_post_run_summary:thoughts=1,tool_calls=0"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_visible_owner:benshu"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_memory_owner:engram"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_memory_session_contract_core_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_memory_session_contract_complete:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_memory_session_surface_core_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_memory_session_surface_complete:true"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_memory_session_surface_note_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_memory_session_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_subagent_budget_surface_note_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_subagent_budget_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_title_surface_note_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_title_surface_note_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_summarization_surface_note_present:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_summarization_surface_note_complete:true" }));
        assert!(bundle.notes.iter().any(|note| {
            note == "runtime_memory_session_orchestration_contract_core_complete:true"
        }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_memory_session_orchestration_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_evidence_contract_core_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| { note == "runtime_evidence_contract_complete:true" }));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_approval_owner:benshu"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_max_parallel_tools:4"));
        assert!(bundle
            .notes
            .iter()
            .any(|note| note == "runtime_loop_abort_count:1"));

        let summary = SimulationHarness::witness_summary(&bundle);
        assert_eq!(summary.witness_id, bundle.witness_id);
        assert_eq!(summary.verdict, "warn");
        assert!(summary.benchmark_fingerprint.is_some());
        assert!(summary
            .notes
            .iter()
            .any(|note| note == "skill_manual_gate_active"));

        let scorecard = SimulationHarness::upsert_scorecard(None, &bundle);
        assert_eq!(scorecard.total_trials, 1);
        assert_eq!(scorecard.passed_trials, 0);
        assert_eq!(scorecard.warned_trials, 1);
        assert_eq!(scorecard.failed_trials, 0);
    }

    #[test]
    fn cpu_fallback_windows_native_outcome_stays_warn_not_fail() {
        let mut run_trace =
            crate::trace::AgentTracer::new(uuid::Uuid::new_v4(), "agent-main").start_run_trace();
        run_trace.status = crate::trace::TraceStatus::Succeeded;
        run_trace.task_id = Some(uuid::Uuid::new_v4());
        run_trace.finished_at = Some(run_trace.started_at);
        run_trace.stages.push(crate::trace::RuntimeStageTrace {
            stage: crate::trace::RuntimeStage::Execution,
            status: crate::trace::TraceStatus::Succeeded,
            started_at: run_trace.started_at,
            finished_at: run_trace.finished_at,
            detail: Some("cpu fallback test".to_string()),
            metadata: HashMap::new(),
        });
        run_trace.metadata.insert(
            "engram_windows_native_embed_outcome".to_string(),
            "cpu_fallback_active".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_strategy".to_string(),
            "inspect_cpu_fallback".to_string(),
        );

        let bundle = SimulationHarness::build_witness_bundle(&run_trace, "cpu_fallback_suite");
        assert_eq!(bundle.outcome.verdict, "warn");
        assert!(bundle
            .outcome
            .failure_reasons
            .iter()
            .any(|reason| reason == "windows_native::embed::cpu_fallback_active"));

        let scorecard = SimulationHarness::upsert_scorecard(None, &bundle);
        assert_eq!(scorecard.total_trials, 1);
        assert_eq!(scorecard.passed_trials, 0);
        assert_eq!(scorecard.warned_trials, 1);
        assert_eq!(scorecard.failed_trials, 0);
    }

    #[test]
    fn provider_downgrade_windows_native_outcome_stays_warn_not_fail() {
        let mut run_trace =
            crate::trace::AgentTracer::new(uuid::Uuid::new_v4(), "agent-main").start_run_trace();
        run_trace.status = crate::trace::TraceStatus::Succeeded;
        run_trace.task_id = Some(uuid::Uuid::new_v4());
        run_trace.finished_at = Some(run_trace.started_at);
        run_trace.stages.push(crate::trace::RuntimeStageTrace {
            stage: crate::trace::RuntimeStage::Execution,
            status: crate::trace::TraceStatus::Succeeded,
            started_at: run_trace.started_at,
            finished_at: run_trace.finished_at,
            detail: Some("provider downgrade test".to_string()),
            metadata: HashMap::new(),
        });
        run_trace.metadata.insert(
            "engram_windows_native_embed_outcome".to_string(),
            "cpu_fallback_provider_downgrade".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_strategy".to_string(),
            "inspect_execution_provider".to_string(),
        );

        let bundle =
            SimulationHarness::build_witness_bundle(&run_trace, "provider_downgrade_suite");
        assert_eq!(bundle.outcome.verdict, "warn");
        assert!(bundle
            .outcome
            .failure_reasons
            .iter()
            .any(|reason| reason == "windows_native::embed::cpu_fallback_provider_downgrade"));

        let scorecard = SimulationHarness::upsert_scorecard(None, &bundle);
        assert_eq!(scorecard.total_trials, 1);
        assert_eq!(scorecard.passed_trials, 0);
        assert_eq!(scorecard.warned_trials, 1);
        assert_eq!(scorecard.failed_trials, 0);
    }

    #[test]
    fn truth_verification_execution_missing_stays_warn_not_fail() {
        let mut run_trace =
            crate::trace::AgentTracer::new(uuid::Uuid::new_v4(), "agent-main").start_run_trace();
        run_trace.status = crate::trace::TraceStatus::Succeeded;
        run_trace.task_id = Some(uuid::Uuid::new_v4());
        run_trace.finished_at = Some(run_trace.started_at);
        run_trace.stages.push(crate::trace::RuntimeStageTrace {
            stage: crate::trace::RuntimeStage::Execution,
            status: crate::trace::TraceStatus::Succeeded,
            started_at: run_trace.started_at,
            finished_at: run_trace.finished_at,
            detail: Some("truth verification execution missing".to_string()),
            metadata: HashMap::new(),
        });
        run_trace
            .metadata
            .insert("truth_status".to_string(), "Unverified".to_string());
        run_trace.metadata.insert(
            "verification_domain".to_string(),
            "ExecutionFact".to_string(),
        );
        run_trace.metadata.insert(
            "verification_outcome".to_string(),
            "VerificationExecutionMissing".to_string(),
        );
        run_trace.metadata.insert(
            "source_posture".to_string(),
            "SourcesRequiredButMissing".to_string(),
        );

        let bundle =
            SimulationHarness::build_witness_bundle(&run_trace, "truth_verification_exec_suite");
        assert_eq!(bundle.outcome.verdict, "warn");
        assert!(bundle
            .outcome
            .failure_reasons
            .iter()
            .any(|reason| reason == "verification::truth_status::unverified"));
        assert!(
            bundle
                .outcome
                .failure_reasons
                .iter()
                .any(|reason| reason
                    == "verification::execution_fact::verification_execution_missing")
        );
        assert!(bundle
            .outcome
            .failure_reasons
            .iter()
            .any(|reason| reason == "verification::source_posture::sources_required_but_missing"));

        let scorecard = SimulationHarness::upsert_scorecard(None, &bundle);
        assert_eq!(scorecard.total_trials, 1);
        assert_eq!(scorecard.passed_trials, 0);
        assert_eq!(scorecard.warned_trials, 1);
        assert_eq!(scorecard.failed_trials, 0);
    }

    #[test]
    fn truth_verification_source_missing_stays_warn_not_fail() {
        let mut run_trace =
            crate::trace::AgentTracer::new(uuid::Uuid::new_v4(), "agent-main").start_run_trace();
        run_trace.status = crate::trace::TraceStatus::Succeeded;
        run_trace.task_id = Some(uuid::Uuid::new_v4());
        run_trace.finished_at = Some(run_trace.started_at);
        run_trace.stages.push(crate::trace::RuntimeStageTrace {
            stage: crate::trace::RuntimeStage::Execution,
            status: crate::trace::TraceStatus::Succeeded,
            started_at: run_trace.started_at,
            finished_at: run_trace.finished_at,
            detail: Some("truth verification source missing".to_string()),
            metadata: HashMap::new(),
        });
        run_trace
            .metadata
            .insert("truth_status".to_string(), "Unverified".to_string());
        run_trace.metadata.insert(
            "verification_domain".to_string(),
            "KnowledgeFact".to_string(),
        );
        run_trace.metadata.insert(
            "verification_outcome".to_string(),
            "VerificationSourceInsufficient".to_string(),
        );
        run_trace.metadata.insert(
            "verification_answer_readiness".to_string(),
            "search_results_only".to_string(),
        );
        run_trace
            .metadata
            .insert("verification_cite_required".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "source_posture".to_string(),
            "SourcesReferencedButNotAttached".to_string(),
        );

        let bundle =
            SimulationHarness::build_witness_bundle(&run_trace, "truth_verification_source_suite");
        assert_eq!(bundle.outcome.verdict, "warn");
        assert!(bundle.outcome.failure_reasons.iter().any(
            |reason| reason == "verification::knowledge_fact::verification_source_insufficient"
        ));
        assert!(bundle.outcome.failure_reasons.iter().any(|reason| {
            reason == "verification::source_posture::sources_referenced_but_not_attached"
        }));
        assert!(bundle
            .outcome
            .failure_reasons
            .iter()
            .any(|reason| reason == "verification::source_required::still_missing"));

        let scorecard = SimulationHarness::upsert_scorecard(None, &bundle);
        assert_eq!(scorecard.total_trials, 1);
        assert_eq!(scorecard.passed_trials, 0);
        assert_eq!(scorecard.warned_trials, 1);
        assert_eq!(scorecard.failed_trials, 0);
    }

    #[test]
    fn truth_verification_source_observed_does_not_emit_source_required_missing_reason() {
        let mut run_trace =
            crate::trace::AgentTracer::new(uuid::Uuid::new_v4(), "agent-main").start_run_trace();
        run_trace.status = crate::trace::TraceStatus::Succeeded;
        run_trace.task_id = Some(uuid::Uuid::new_v4());
        run_trace.finished_at = Some(run_trace.started_at);
        run_trace.stages.push(crate::trace::RuntimeStageTrace {
            stage: crate::trace::RuntimeStage::Execution,
            status: crate::trace::TraceStatus::Succeeded,
            started_at: run_trace.started_at,
            finished_at: run_trace.finished_at,
            detail: Some("truth verification source observed".to_string()),
            metadata: HashMap::new(),
        });
        run_trace
            .metadata
            .insert("truth_status".to_string(), "Verified".to_string());
        run_trace.metadata.insert(
            "verification_domain".to_string(),
            "KnowledgeFact".to_string(),
        );
        run_trace.metadata.insert(
            "verification_outcome".to_string(),
            "VerificationSucceeded".to_string(),
        );
        run_trace.metadata.insert(
            "verification_answer_readiness".to_string(),
            "source_content_observed".to_string(),
        );
        run_trace
            .metadata
            .insert("verification_cite_required".to_string(), "true".to_string());
        run_trace
            .metadata
            .insert("source_posture".to_string(), "SourcesAttached".to_string());

        let bundle =
            SimulationHarness::build_witness_bundle(&run_trace, "truth_verification_source_ok");
        assert!(!bundle
            .outcome
            .failure_reasons
            .iter()
            .any(|reason| reason == "verification::source_required::still_missing"));
    }

    #[test]
    fn truth_verification_local_context_only_stays_warn_and_gets_distinct_reason() {
        let mut run_trace =
            crate::trace::AgentTracer::new(uuid::Uuid::new_v4(), "agent-main").start_run_trace();
        run_trace.status = crate::trace::TraceStatus::Succeeded;
        run_trace.task_id = Some(uuid::Uuid::new_v4());
        run_trace.finished_at = Some(run_trace.started_at);
        run_trace.stages.push(crate::trace::RuntimeStageTrace {
            stage: crate::trace::RuntimeStage::Execution,
            status: crate::trace::TraceStatus::Succeeded,
            started_at: run_trace.started_at,
            finished_at: run_trace.finished_at,
            detail: Some("truth verification local context only".to_string()),
            metadata: HashMap::new(),
        });
        run_trace
            .metadata
            .insert("truth_status".to_string(), "Unverified".to_string());
        run_trace.metadata.insert(
            "verification_domain".to_string(),
            "KnowledgeFact".to_string(),
        );
        run_trace.metadata.insert(
            "verification_requirement".to_string(),
            "LocalContextAllowed".to_string(),
        );
        run_trace.metadata.insert(
            "verification_mode".to_string(),
            "LocalContextOnly".to_string(),
        );
        run_trace.metadata.insert(
            "verification_outcome".to_string(),
            "VerificationNotRequired".to_string(),
        );
        run_trace.metadata.insert(
            "source_posture".to_string(),
            "NoSourcesRequired".to_string(),
        );
        run_trace.metadata.insert(
            "verification_answer_readiness".to_string(),
            "local_context_only".to_string(),
        );

        let bundle =
            SimulationHarness::build_witness_bundle(&run_trace, "truth_verification_local_suite");
        assert_eq!(bundle.outcome.verdict, "warn");
        assert!(bundle
            .outcome
            .failure_reasons
            .iter()
            .any(|reason| reason == "verification::truth_status::unverified"));
        assert!(bundle
            .outcome
            .failure_reasons
            .iter()
            .any(|reason| reason == "verification::knowledge_fact::local_context_only"));

        let scorecard = SimulationHarness::upsert_scorecard(None, &bundle);
        assert_eq!(scorecard.total_trials, 1);
        assert_eq!(scorecard.passed_trials, 0);
        assert_eq!(scorecard.warned_trials, 1);
        assert_eq!(scorecard.failed_trials, 0);
    }

    #[tokio::test]
    async fn real_harness_runs_case_and_produces_scorecard() {
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let case = RealHarnessCase {
            suite_id: "real_runtime_suite".to_string(),
            scenario: "single_agent_tool_execution".to_string(),
            session_id: Some(Uuid::new_v4().to_string()),
            thread_id: Some("thread-main".to_string()),
        };

        let result = RealHarness::run_case(
            case.clone(),
            |_| async move {
                let mut run_trace = tracer.start_run_trace();
                run_trace.task_id = Some(Uuid::new_v4());
                run_trace.thread_id = case.thread_id.clone();
                run_trace.status = TraceStatus::Succeeded;
                run_trace.finished_at = Some(Utc::now());
                run_trace.stages.push(RuntimeStageTrace {
                    stage: RuntimeStage::Ingress,
                    status: TraceStatus::Succeeded,
                    started_at: Utc::now(),
                    finished_at: Some(Utc::now()),
                    detail: Some("real case executed".to_string()),
                    metadata: Default::default(),
                });
                Ok(run_trace)
            },
            None,
        )
        .await
        .expect("real harness should succeed");

        assert_eq!(result.case.suite_id, "real_runtime_suite");
        assert_eq!(result.witness.task.scenario, "single_agent_tool_execution");
        assert!(result
            .witness
            .notes
            .iter()
            .any(|note| note == "real_harness"));
        assert_eq!(result.scorecard.total_trials, 1);
        assert_eq!(result.scorecard.passed_trials, 1);
    }

    #[tokio::test]
    async fn real_harness_runs_suite_and_accumulates_scorecard() {
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let suite = vec![
            RealHarnessCase {
                suite_id: "runtime_suite_batch".to_string(),
                scenario: "case_one".to_string(),
                session_id: Some(Uuid::new_v4().to_string()),
                thread_id: Some("thread-one".to_string()),
            },
            RealHarnessCase {
                suite_id: "runtime_suite_batch".to_string(),
                scenario: "case_two".to_string(),
                session_id: Some(Uuid::new_v4().to_string()),
                thread_id: Some("thread-two".to_string()),
            },
        ];

        let result = RealHarness::run_suite(suite, |case| {
            let tracer = tracer.clone();
            async move {
                let mut run_trace = tracer.start_run_trace();
                run_trace.task_id = Some(Uuid::new_v4());
                run_trace.thread_id = case.thread_id.clone();
                run_trace.status = TraceStatus::Succeeded;
                run_trace.finished_at = Some(Utc::now());
                run_trace.stages.push(RuntimeStageTrace {
                    stage: RuntimeStage::Ingress,
                    status: TraceStatus::Succeeded,
                    started_at: Utc::now(),
                    finished_at: Some(Utc::now()),
                    detail: Some(format!("{} executed", case.scenario)),
                    metadata: Default::default(),
                });
                Ok(run_trace)
            }
        })
        .await
        .expect("real harness suite should succeed");

        assert_eq!(result.total_cases, 2);
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.scorecard.total_trials, 2);
        assert_eq!(result.scorecard.passed_trials, 2);
        assert_eq!(result.suite_id, "runtime_suite_batch");
    }
}
