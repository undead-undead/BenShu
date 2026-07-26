//! AgentOS Observability & Telemetry Framework (BenShu-TELEMETRY)
//!
//! Provides global tracing, hardware metrics, and performance analytics for agent runs.
//! Privacy-first: Full Trace output defaults to local database only.

pub mod eval;
pub mod findings;
pub mod metrics;
pub mod profiler;
pub mod runtime_contract;
pub mod skill_loading;
pub mod trace;

pub use eval::{
    BenchmarkFingerprint, EvalOutcome, EvalTask, EvalTrial, OutcomeFailureKind, RealHarness,
    RealHarnessCase, RealHarnessResult, RealHarnessSuiteResult, Scorecard, ScorecardEntry,
    SimulationHarness, TranscriptFailureKind, WitnessBundle, WitnessLogEntry,
};
pub use metrics::{HardwareMetrics, ProcessMetrics};
pub use profiler::{
    profiler_id_for_run, LatencyBreakdownEntry, ProfilerArtifact, ProfilerArtifactQuery,
    ProfilerExport, ProfilerLatencyArtifact, ProfilerMemoryArtifact, ProfilerResourceArtifact,
    ResourceMetricKind, PROFILER_EXPORT_SCHEMA_VERSION,
};
pub use runtime_contract::{
    ScorecardQuery, TruthVerificationQueryFields, WindowsNativeQueryFields, WitnessLogQuery,
};
pub use trace::{
    AgentTracer, ArtifactRef, RunReplay, RunReplayStep, RunTrace, RuntimeStage, RuntimeStageTrace,
    ToolTrace, TraceStatus, WitnessSummary,
};

use benshu_infra::{HealthCheck, HealthStatus};
use dashmap::DashMap;
use parking_lot::Mutex;
use runtime_contract::{
    META_SOURCE_POSTURE, META_TRUTH_STATUS, META_VERIFICATION_ANSWER_READINESS,
    META_VERIFICATION_CAN_FINALIZE_ANSWER, META_VERIFICATION_CITE_REQUIRED,
    META_VERIFICATION_CONTINUATION, META_VERIFICATION_DOMAIN, META_VERIFICATION_LAST_TOOL,
    META_VERIFICATION_MODE, META_VERIFICATION_OUTCOME, META_VERIFICATION_REQUIREMENT,
    META_VERIFICATION_REQUIRES_FOLLOWUP, META_VERIFICATION_ROUTE_REASON,
    META_VERIFICATION_TERMINATION, META_WINDOWS_NATIVE_EMBED_OUTCOME,
    META_WINDOWS_NATIVE_EMBED_STRATEGY, META_WINDOWS_NATIVE_RERANK_OUTCOME,
    META_WINDOWS_NATIVE_RERANK_STRATEGY,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

/// Telemetry level for different runtime environments
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryLevel {
    Silent,
    Minimal,
    Production,
    Diagnostic,
    Verbose,
}

pub struct TelemetryManager {
    level: TelemetryLevel,
    run_traces: DashMap<Uuid, RunTrace>,
    witness_summaries: DashMap<Uuid, WitnessSummary>,
    witness_bundles: DashMap<Uuid, WitnessBundle>,
    witness_logs: DashMap<Uuid, WitnessLogEntry>,
    scorecards: DashMap<String, Scorecard>,
    profiler_artifacts: DashMap<String, ProfilerArtifact>,
    storage_root: Option<PathBuf>,
    pending_witness_logs: Mutex<Vec<WitnessLogEntry>>,
    last_witness_log_flush: Mutex<Instant>,
    witness_log_batch_size: usize,
    witness_log_max_pending: usize,
    witness_log_max_delay: Duration,
    witness_log_retention: usize,
}

#[async_trait::async_trait]
impl HealthCheck for TelemetryManager {
    async fn check_health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
    fn module_name(&self) -> &'static str {
        "benshu-telemetry"
    }
}

impl Default for TelemetryManager {
    fn default() -> Self {
        Self {
            level: TelemetryLevel::Production,
            run_traces: DashMap::new(),
            witness_summaries: DashMap::new(),
            witness_bundles: DashMap::new(),
            witness_logs: DashMap::new(),
            scorecards: DashMap::new(),
            profiler_artifacts: DashMap::new(),
            storage_root: Some(Self::default_storage_root()),
            pending_witness_logs: Mutex::new(Vec::new()),
            last_witness_log_flush: Mutex::new(Instant::now()),
            witness_log_batch_size: 16,
            witness_log_max_pending: 64,
            witness_log_max_delay: Duration::from_millis(500),
            witness_log_retention: 2048,
        }
    }
}

impl TelemetryManager {
    pub fn new(level: TelemetryLevel) -> Self {
        Self::with_storage_config(
            level,
            Some(Self::default_storage_root()),
            16,
            64,
            Duration::from_millis(500),
            2048,
        )
    }

    pub fn with_storage_root(level: TelemetryLevel, storage_root: Option<PathBuf>) -> Self {
        Self::with_storage_config(
            level,
            storage_root,
            16,
            64,
            Duration::from_millis(500),
            2048,
        )
    }

    pub fn with_storage_config(
        level: TelemetryLevel,
        storage_root: Option<PathBuf>,
        witness_log_batch_size: usize,
        witness_log_max_pending: usize,
        witness_log_max_delay: Duration,
        witness_log_retention: usize,
    ) -> Self {
        let telemetry = Self {
            level,
            run_traces: DashMap::new(),
            witness_summaries: DashMap::new(),
            witness_bundles: DashMap::new(),
            witness_logs: DashMap::new(),
            scorecards: DashMap::new(),
            profiler_artifacts: DashMap::new(),
            storage_root,
            pending_witness_logs: Mutex::new(Vec::new()),
            last_witness_log_flush: Mutex::new(Instant::now()),
            witness_log_batch_size: witness_log_batch_size.max(1),
            witness_log_max_pending: witness_log_max_pending.max(1),
            witness_log_max_delay,
            witness_log_retention: witness_log_retention.max(1),
        };
        telemetry.load_persisted_state();
        telemetry
    }

    pub fn init_global(&self) -> anyhow::Result<()> {
        // Init tracing subscriber
        tracing::debug!("Initializing Global Telemetry (Level: {:?})...", self.level);
        Ok(())
    }

    pub fn save_run_trace(&self, run_trace: RunTrace) {
        if let Some(witness) = run_trace.witness.clone() {
            let witness_id = witness.witness_id;
            self.witness_summaries.insert(witness_id, witness);
            if let Some(bundle) = self.get_witness_bundle(&witness_id) {
                self.save_witness_log(SimulationHarness::build_witness_log_entry(
                    &run_trace, &bundle,
                ));
            }
        }
        self.persist_run_trace(&run_trace);
        self.run_traces.insert(run_trace.run_id, run_trace);
    }

    pub fn get_run_trace(&self, run_id: &Uuid) -> Option<RunTrace> {
        self.run_traces.get(run_id).map(|entry| entry.clone())
    }

    pub fn get_run_replay(&self, run_id: &Uuid) -> Option<RunReplay> {
        self.get_run_trace(run_id).map(|trace| trace.to_replay())
    }

    pub fn list_session_traces(&self, session_id: &Uuid) -> Vec<RunTrace> {
        let mut traces: Vec<RunTrace> = self
            .run_traces
            .iter()
            .filter_map(|entry| {
                if entry.session_id == *session_id {
                    Some(entry.clone())
                } else {
                    None
                }
            })
            .collect();
        traces.sort_by(|left, right| right.started_at.cmp(&left.started_at));
        traces
    }

    pub fn get_witness_summary(&self, witness_id: &Uuid) -> Option<WitnessSummary> {
        self.witness_summaries
            .get(witness_id)
            .map(|entry| entry.clone())
    }

    pub fn attach_simulation_witness(
        &self,
        run_trace: &mut RunTrace,
        scorecard_id: Option<&str>,
    ) -> WitnessBundle {
        let suite_id = scorecard_id.unwrap_or("runtime_main_path");
        let bundle = SimulationHarness::build_witness_bundle(run_trace, suite_id);
        let summary = SimulationHarness::witness_summary(&bundle);
        run_trace.witness = Some(summary.clone());

        self.save_witness_bundle(bundle.clone());

        let next_scorecard = SimulationHarness::upsert_scorecard(
            self.scorecards.get(suite_id).map(|entry| entry.clone()),
            &bundle,
        );
        self.save_scorecard(next_scorecard);
        self.save_profiler_artifact(ProfilerArtifact::from_run_trace(run_trace, Some(&bundle)));

        bundle
    }

    pub fn capture_evaluation_tap(
        &self,
        run_trace: &mut RunTrace,
        scorecard_id: Option<&str>,
    ) -> WitnessBundle {
        let bundle = self.attach_simulation_witness(run_trace, scorecard_id);
        self.save_run_trace(run_trace.clone());
        bundle
    }

    pub fn get_witness_bundle(&self, witness_id: &Uuid) -> Option<WitnessBundle> {
        self.witness_bundles
            .get(witness_id)
            .map(|entry| entry.clone())
    }

    pub fn get_scorecard(&self, scorecard_id: &str) -> Option<Scorecard> {
        self.scorecards.get(scorecard_id).map(|entry| entry.clone())
    }

    pub fn list_scorecards(&self) -> Vec<Scorecard> {
        self.query_scorecards(&ScorecardQuery::default())
    }

    pub fn query_scorecards(&self, query: &ScorecardQuery) -> Vec<Scorecard> {
        let normalized_text = query
            .text
            .as_ref()
            .map(|text| Self::normalize_search_text(text))
            .filter(|text| !text.is_empty());
        let mut entries: Vec<Scorecard> = self
            .scorecards
            .iter()
            .filter_map(|entry| {
                let value = entry.value();
                if let Some(suite_id) = &query.suite_id {
                    if &value.suite_id != suite_id {
                        return None;
                    }
                }
                if let Some(text) = normalized_text.as_ref() {
                    let matches = Self::matches_search_text(&value.scorecard_id, &text)
                        || Self::matches_search_text(&value.suite_id, &text)
                        || Self::matches_search_text(
                            &value.benchmark_fingerprint.fingerprint,
                            &text,
                        );
                    if !matches {
                        return None;
                    }
                }
                if query.truth_verification.has_filters() || query.windows_native.has_filters() {
                    let any_entry_matches = value.entries.iter().any(|score_entry| {
                        self.witness_logs
                            .get(&score_entry.witness_id)
                            .map(|log| {
                                Self::matches_truth_verification_filters(
                                    log.value(),
                                    &query.truth_verification,
                                ) && Self::matches_windows_native_witness_filters(
                                    log.value(),
                                    &query.windows_native,
                                )
                            })
                            .unwrap_or(false)
                    });
                    if !any_entry_matches {
                        return None;
                    }
                }
                Some(value.clone())
            })
            .collect();
        entries.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.scorecard_id.cmp(&right.scorecard_id))
        });
        if let Some(limit) = query.limit {
            entries.truncate(limit);
        }
        entries
    }

    pub fn get_witness_log(&self, witness_id: &Uuid) -> Option<WitnessLogEntry> {
        self.witness_logs.get(witness_id).map(|entry| entry.clone())
    }

    pub fn save_profiler_artifact(&self, artifact: ProfilerArtifact) {
        self.persist_profiler_artifact(&artifact);
        self.profiler_artifacts
            .insert(artifact.profiler_id.clone(), artifact);
    }

    pub fn get_profiler_artifact(&self, profiler_id: &str) -> Option<ProfilerArtifact> {
        self.profiler_artifacts
            .get(profiler_id)
            .map(|entry| entry.clone())
    }

    pub fn get_run_profiler_artifact(&self, run_id: &Uuid) -> Option<ProfilerArtifact> {
        self.get_profiler_artifact(&profiler_id_for_run(run_id))
    }

    pub fn query_profiler_artifacts(&self, query: &ProfilerArtifactQuery) -> Vec<ProfilerArtifact> {
        let mut entries: Vec<ProfilerArtifact> = self
            .profiler_artifacts
            .iter()
            .filter_map(|entry| {
                let value = entry.value();
                if let Some(suite_id) = &query.suite_id {
                    if value.suite_id.as_ref() != Some(suite_id) {
                        return None;
                    }
                }
                if let Some(run_id) = query.run_id {
                    if value.run_id != run_id {
                        return None;
                    }
                }
                if let Some(trace_id) = query.trace_id {
                    if value.trace_id != trace_id {
                        return None;
                    }
                }
                if let Some(witness_id) = query.witness_id {
                    if value.witness_id != Some(witness_id) {
                        return None;
                    }
                }
                if let Some(fingerprint) = &query.benchmark_fingerprint {
                    let matches = value
                        .benchmark_fingerprint
                        .as_ref()
                        .map(|item| item.fingerprint.as_str())
                        == Some(fingerprint.as_str());
                    if !matches {
                        return None;
                    }
                }
                Some(value.clone())
            })
            .collect();
        entries.sort_by(|left, right| {
            right
                .generated_at
                .cmp(&left.generated_at)
                .then_with(|| left.profiler_id.cmp(&right.profiler_id))
        });
        if let Some(limit) = query.limit {
            entries.truncate(limit);
        }
        entries
    }

    pub fn export_profiler_artifacts(&self, query: &ProfilerArtifactQuery) -> ProfilerExport {
        ProfilerExport::from_artifacts(self.query_profiler_artifacts(query))
    }

    pub fn query_witness_logs(&self, query: &WitnessLogQuery) -> Vec<WitnessLogEntry> {
        let normalized_text = query
            .text
            .as_ref()
            .map(|text| Self::normalize_search_text(text))
            .filter(|text| !text.is_empty());
        let mut entries: Vec<WitnessLogEntry> = self
            .witness_logs
            .iter()
            .filter_map(|entry| {
                let value = entry.value();
                if let Some(suite_id) = &query.suite_id {
                    if &value.suite_id != suite_id {
                        return None;
                    }
                }
                if let Some(verdict) = &query.verdict {
                    if &value.verdict != verdict {
                        return None;
                    }
                }
                if let Some(provider) = &query.provider {
                    if value.provider.as_ref() != Some(provider) {
                        return None;
                    }
                }
                if let Some(task_id) = query.task_id {
                    if value.task_id != Some(task_id) {
                        return None;
                    }
                }
                if let Some(run_id) = query.run_id {
                    if value.run_id != run_id {
                        return None;
                    }
                }
                if let Some(trace_id) = query.trace_id {
                    if value.trace_id != trace_id {
                        return None;
                    }
                }
                if let Some(degraded) = query.degraded {
                    if value.degraded != degraded {
                        return None;
                    }
                }
                if let Some(budget_exhausted) = query.budget_exhausted {
                    if value.budget_exhausted != budget_exhausted {
                        return None;
                    }
                }
                if !Self::matches_truth_verification_filters(value, &query.truth_verification) {
                    return None;
                }
                if !Self::matches_windows_native_witness_filters(value, &query.windows_native) {
                    return None;
                }
                if let Some(text) = normalized_text.as_ref() {
                    let matches_text = Self::matches_search_text(&value.scenario, text)
                        || Self::matches_search_text(&value.suite_id, text)
                        || Self::matches_search_text(&value.verdict, text)
                        || value
                            .provider
                            .as_ref()
                            .map(|provider| Self::matches_search_text(provider, text))
                            .unwrap_or(false)
                        || value
                            .model
                            .as_ref()
                            .map(|model| Self::matches_search_text(model, text))
                            .unwrap_or(false)
                        || value
                            .route
                            .as_ref()
                            .map(|route| Self::matches_search_text(route, text))
                            .unwrap_or(false)
                        || value
                            .context_artifacts
                            .iter()
                            .any(|artifact| Self::matches_search_text(artifact, text))
                        || value
                            .tool_path
                            .iter()
                            .any(|tool| Self::matches_search_text(tool, text))
                        || value
                            .failure_reasons
                            .iter()
                            .any(|reason| Self::matches_search_text(reason, text))
                        || value
                            .policy_decision
                            .as_ref()
                            .map(|decision| Self::matches_search_text(decision, text))
                            .unwrap_or(false)
                        || value
                            .fallback_reason
                            .as_ref()
                            .map(|reason| Self::matches_search_text(reason, text))
                            .unwrap_or(false)
                        || value.metadata.iter().any(|(key, value)| {
                            Self::matches_search_text(key, text)
                                || Self::matches_search_text(value, text)
                        });
                    if !matches_text {
                        return None;
                    }
                }
                Some(value.clone())
            })
            .collect();
        entries.sort_by(|left, right| right.recorded_at.cmp(&left.recorded_at));
        if let Some(limit) = query.limit {
            entries.truncate(limit);
        }
        entries
    }

    pub fn save_witness_bundle(&self, bundle: WitnessBundle) {
        let summary = SimulationHarness::witness_summary(&bundle);
        self.witness_summaries.insert(summary.witness_id, summary);
        self.persist_witness_bundle(&bundle);
        self.witness_bundles.insert(bundle.witness_id, bundle);
    }

    pub fn save_witness_log(&self, entry: WitnessLogEntry) {
        self.witness_logs.insert(entry.witness_id, entry.clone());
        let should_flush = {
            let mut pending = self.pending_witness_logs.lock();
            pending.push(entry);
            pending.len() >= self.witness_log_batch_size
                || pending.len() >= self.witness_log_max_pending
                || self.last_witness_log_flush.lock().elapsed() >= self.witness_log_max_delay
        };
        if should_flush {
            self.flush_pending_witness_logs();
        }
    }

    pub fn flush_pending_witness_logs(&self) {
        let batch = {
            let mut pending = self.pending_witness_logs.lock();
            if pending.is_empty() {
                return;
            }
            pending.drain(..).collect::<Vec<_>>()
        };
        self.persist_witness_log_batch(&batch);
        *self.last_witness_log_flush.lock() = Instant::now();
    }

    pub fn save_scorecard(&self, scorecard: Scorecard) {
        self.persist_scorecard(&scorecard);
        self.scorecards
            .insert(scorecard.scorecard_id.clone(), scorecard);
    }

    fn default_storage_root() -> PathBuf {
        PathBuf::from("data/telemetry")
    }

    fn load_persisted_state(&self) {
        let Some(root) = &self.storage_root else {
            return;
        };

        if let Err(err) = self.ensure_storage_layout(root) {
            tracing::warn!(
                "Failed to initialize telemetry storage at {:?}: {}",
                root,
                err
            );
            return;
        }

        self.load_witness_bundles(root);
        self.load_witness_logs(root);
        self.load_scorecards(root);
        self.load_profiler_artifacts(root);
        self.load_run_traces(root);
        *self.last_witness_log_flush.lock() = Instant::now();
    }

    fn ensure_storage_layout(&self, root: &Path) -> anyhow::Result<()> {
        fs::create_dir_all(root.join("run_traces"))?;
        fs::create_dir_all(root.join("witness_bundles"))?;
        fs::create_dir_all(root.join("witness_logs"))?;
        fs::create_dir_all(root.join("scorecards"))?;
        fs::create_dir_all(root.join("profiler_artifacts"))?;
        Ok(())
    }

    fn load_run_traces(&self, root: &Path) {
        self.load_json_dir::<RunTrace>(&root.join("run_traces"), |trace| {
            if let Some(witness) = trace.witness.clone() {
                self.witness_summaries.insert(witness.witness_id, witness);
            }
            self.run_traces.insert(trace.run_id, trace);
        });
    }

    fn load_witness_bundles(&self, root: &Path) {
        self.load_json_dir::<WitnessBundle>(&root.join("witness_bundles"), |bundle| {
            let summary = SimulationHarness::witness_summary(&bundle);
            self.witness_summaries.insert(summary.witness_id, summary);
            self.witness_bundles.insert(bundle.witness_id, bundle);
        });
    }

    fn load_scorecards(&self, root: &Path) {
        self.load_json_dir::<Scorecard>(&root.join("scorecards"), |scorecard| {
            self.scorecards
                .insert(scorecard.scorecard_id.clone(), scorecard);
        });
    }

    fn load_profiler_artifacts(&self, root: &Path) {
        self.load_json_dir::<ProfilerArtifact>(&root.join("profiler_artifacts"), |artifact| {
            self.profiler_artifacts
                .insert(artifact.profiler_id.clone(), artifact);
        });
    }

    fn load_witness_logs(&self, root: &Path) {
        self.load_json_dir::<WitnessLogEntry>(&root.join("witness_logs"), |entry| {
            self.witness_logs.insert(entry.witness_id, entry);
        });
    }

    fn load_json_dir<T>(&self, dir: &Path, mut on_loaded: impl FnMut(T))
    where
        T: serde::de::DeserializeOwned,
    {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => {
                tracing::warn!("Failed to read telemetry dir {:?}: {}", dir, err);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            match fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<T>(&bytes).ok())
            {
                Some(value) => on_loaded(value),
                None => tracing::warn!("Failed to load telemetry artifact {:?}", path),
            }
        }
    }

    fn persist_run_trace(&self, run_trace: &RunTrace) {
        let Some(root) = &self.storage_root else {
            return;
        };
        self.persist_json(
            &root
                .join("run_traces")
                .join(format!("{}.json", run_trace.run_id)),
            run_trace,
        );
    }

    fn persist_witness_bundle(&self, bundle: &WitnessBundle) {
        let Some(root) = &self.storage_root else {
            return;
        };
        self.persist_json(
            &root
                .join("witness_bundles")
                .join(format!("{}.json", bundle.witness_id)),
            bundle,
        );
    }

    fn persist_scorecard(&self, scorecard: &Scorecard) {
        let Some(root) = &self.storage_root else {
            return;
        };
        self.persist_json(
            &root.join("scorecards").join(format!(
                "{}.json",
                Self::sanitize_file_stem(&scorecard.scorecard_id)
            )),
            scorecard,
        );
    }

    fn persist_profiler_artifact(&self, artifact: &ProfilerArtifact) {
        let Some(root) = &self.storage_root else {
            return;
        };
        self.persist_json(
            &root.join("profiler_artifacts").join(format!(
                "{}.json",
                Self::sanitize_file_stem(&artifact.profiler_id)
            )),
            artifact,
        );
    }

    fn persist_witness_log(&self, entry: &WitnessLogEntry) {
        let Some(root) = &self.storage_root else {
            return;
        };
        self.persist_json(
            &root
                .join("witness_logs")
                .join(format!("{}.json", entry.witness_id)),
            entry,
        );
    }

    fn persist_witness_log_batch(&self, entries: &[WitnessLogEntry]) {
        if entries.is_empty() {
            return;
        }
        for entry in entries {
            self.persist_witness_log(entry);
        }
        self.prune_witness_log_retention();
    }

    fn persist_json<T: serde::Serialize>(&self, path: &Path, value: &T) {
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                tracing::warn!("Failed to create telemetry dir {:?}: {}", parent, err);
                return;
            }
        }

        match serde_json::to_vec_pretty(value) {
            Ok(bytes) => {
                if let Err(err) = fs::write(path, bytes) {
                    tracing::warn!("Failed to persist telemetry artifact {:?}: {}", path, err);
                }
            }
            Err(err) => {
                tracing::warn!("Failed to serialize telemetry artifact {:?}: {}", path, err);
            }
        }
    }

    fn sanitize_file_stem(id: &str) -> String {
        id.chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
                _ => '_',
            })
            .collect()
    }

    fn normalize_search_text(value: &str) -> String {
        value
            .chars()
            .map(|ch| match ch {
                '_' | '-' | ':' | '/' | '.' => ' ',
                _ => ch.to_ascii_lowercase(),
            })
            .collect::<String>()
    }

    fn matches_search_text(value: &str, query: &str) -> bool {
        Self::normalize_search_text(value).contains(query)
    }

    fn matches_windows_native_witness_filters(
        value: &WitnessLogEntry,
        query: &WindowsNativeQueryFields,
    ) -> bool {
        Self::matches_optional_metadata_filter(
            value,
            META_WINDOWS_NATIVE_EMBED_OUTCOME,
            query.windows_native_embed_outcome.as_deref(),
        ) && Self::matches_optional_outcome_class_filter(
            value,
            META_WINDOWS_NATIVE_EMBED_OUTCOME,
            query.windows_native_embed_class.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_WINDOWS_NATIVE_EMBED_STRATEGY,
            query.windows_native_embed_strategy.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_WINDOWS_NATIVE_RERANK_OUTCOME,
            query.windows_native_rerank_outcome.as_deref(),
        ) && Self::matches_optional_outcome_class_filter(
            value,
            META_WINDOWS_NATIVE_RERANK_OUTCOME,
            query.windows_native_rerank_class.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_WINDOWS_NATIVE_RERANK_STRATEGY,
            query.windows_native_rerank_strategy.as_deref(),
        ) && Self::matches_optional_failure_reason_filter(
            value,
            query.windows_native_failure_reason.as_deref(),
        )
    }

    fn matches_truth_verification_filters(
        value: &WitnessLogEntry,
        query: &TruthVerificationQueryFields,
    ) -> bool {
        Self::matches_optional_metadata_filter(
            value,
            META_TRUTH_STATUS,
            query.truth_status.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_DOMAIN,
            query.verification_domain.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_REQUIREMENT,
            query.verification_requirement.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_MODE,
            query.verification_mode.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_OUTCOME,
            query.verification_outcome.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_ANSWER_READINESS,
            query.verification_answer_readiness.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_ROUTE_REASON,
            query.verification_route_reason.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_CONTINUATION,
            query.verification_continuation.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_TERMINATION,
            query.verification_termination.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_REQUIRES_FOLLOWUP,
            query.verification_requires_followup.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_CAN_FINALIZE_ANSWER,
            query.verification_can_finalize_answer.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            runtime_contract::META_VERIFICATION_NEXT_TOOLS,
            query.verification_next_tools.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_CITE_REQUIRED,
            query.verification_cite_required.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_SOURCE_POSTURE,
            query.source_posture.as_deref(),
        ) && Self::matches_optional_metadata_filter(
            value,
            META_VERIFICATION_LAST_TOOL,
            query.verification_last_tool.as_deref(),
        )
    }

    fn matches_optional_metadata_filter(
        value: &WitnessLogEntry,
        key: &str,
        expected: Option<&str>,
    ) -> bool {
        match expected {
            Some(expected) => value.metadata.get(key).map(|v| v.as_str()) == Some(expected),
            None => true,
        }
    }

    fn matches_optional_outcome_class_filter(
        value: &WitnessLogEntry,
        key: &str,
        expected: Option<&str>,
    ) -> bool {
        match expected {
            Some(expected) => value
                .metadata
                .get(key)
                .map(|outcome| Self::windows_native_outcome_class(outcome) == expected)
                .unwrap_or(false),
            None => true,
        }
    }

    fn matches_optional_failure_reason_filter(
        value: &WitnessLogEntry,
        expected: Option<&str>,
    ) -> bool {
        match expected {
            Some(expected) => value
                .failure_reasons
                .iter()
                .any(|reason| reason == expected),
            None => true,
        }
    }

    fn windows_native_outcome_class(outcome: &str) -> &'static str {
        match outcome {
            "windows_native_active" | "active" => "active",
            "cpu_fallback_provider_downgrade" => "provider_downgrade",
            "cpu_fallback_no_accelerator_route" => "no_accelerator_route",
            "cpu_fallback_active" => "cpu_fallback",
            "windows_native_provider_execution_failed" => "provider_failure",
            "windows_native_execution_failed" => "runtime_failure",
            "fallback_runtime_active" | "migrate_to_windows_native_runtime" => "fallback_runtime",
            "backend_unlinked" | "runtime_missing" | "validation_only" => "pending_runtime",
            "model_contract_incompatible" => "contract_incompatible",
            "accelerator_resource_exhausted" => "resource_exhausted",
            "accelerator_unavailable" => "accelerator_unavailable",
            "not_observed" | "not_reported" => "not_observed",
            _ => "other",
        }
    }

    fn prune_witness_log_retention(&self) {
        let Some(root) = &self.storage_root else {
            return;
        };
        let dir = root.join("witness_logs");
        let Ok(entries) = fs::read_dir(&dir) else {
            return;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    return None;
                }
                let modified = entry
                    .metadata()
                    .ok()
                    .and_then(|meta| meta.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                Some((path, modified))
            })
            .collect();
        if files.len() <= self.witness_log_retention {
            return;
        }
        files.sort_by(|left, right| right.1.cmp(&left.1));
        for (path, _) in files.into_iter().skip(self.witness_log_retention) {
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                if let Ok(witness_id) = Uuid::parse_str(stem) {
                    self.witness_logs.remove(&witness_id);
                }
            }
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn telemetry_manager_can_store_and_list_run_traces() {
        let root = temp_telemetry_root();
        let telemetry =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");

        let mut first = tracer.start_run_trace();
        first
            .metadata
            .insert("step".to_string(), "first".to_string());

        let mut second = tracer.start_run_trace();
        second.started_at = second.started_at + chrono::Duration::seconds(3);
        second
            .metadata
            .insert("step".to_string(), "second".to_string());

        telemetry.save_run_trace(first.clone());
        telemetry.save_run_trace(second.clone());

        let loaded = telemetry
            .get_run_trace(&first.run_id)
            .expect("stored run trace");
        assert_eq!(
            loaded.metadata.get("step").map(String::as_str),
            Some("first")
        );

        let session_traces = telemetry.list_session_traces(&tracer.session_id);
        assert_eq!(session_traces.len(), 2);
        assert_eq!(session_traces[0].run_id, second.run_id);
        assert_eq!(session_traces[1].run_id, first.run_id);

        let reloaded =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        assert!(reloaded.get_run_trace(&first.run_id).is_some());
        assert_eq!(reloaded.list_session_traces(&tracer.session_id).len(), 2);
        cleanup_telemetry_root(&root);
    }

    #[test]
    fn telemetry_manager_indexes_witness_summaries_from_run_traces() {
        let root = temp_telemetry_root();
        let telemetry =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let mut run_trace = tracer.start_run_trace();
        let witness_id = Uuid::new_v4();
        run_trace.witness = Some(WitnessSummary {
            witness_id,
            run_id: Some(run_trace.run_id),
            verdict: "pass".to_string(),
            scorecard: None,
            replayable: true,
            benchmark_fingerprint: Some("bench:v1".to_string()),
            notes: vec!["ground truth available".to_string()],
        });

        telemetry.save_run_trace(run_trace.clone());

        let stored = telemetry
            .get_witness_summary(&witness_id)
            .expect("stored witness summary");
        assert_eq!(stored.witness_id, witness_id);
        assert_eq!(stored.run_id, Some(run_trace.run_id));
        assert_eq!(stored.verdict, "pass");

        let reloaded =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        assert_eq!(
            reloaded
                .get_witness_summary(&witness_id)
                .expect("reloaded summary")
                .verdict,
            "pass"
        );
        cleanup_telemetry_root(&root);
    }

    #[test]
    fn telemetry_manager_materializes_witness_bundle_and_scorecard() {
        let root = temp_telemetry_root();
        let telemetry =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let mut run_trace = tracer.start_run_trace();
        run_trace.task_id = Some(Uuid::new_v4());
        run_trace.status = TraceStatus::Succeeded;
        run_trace.stages.push(RuntimeStageTrace {
            stage: RuntimeStage::Ingress,
            status: TraceStatus::Succeeded,
            started_at: chrono::Utc::now(),
            finished_at: Some(chrono::Utc::now()),
            detail: Some("message accepted".to_string()),
            metadata: std::collections::HashMap::new(),
        });

        let bundle = telemetry.attach_simulation_witness(&mut run_trace, Some("runtime_main_path"));
        telemetry.save_run_trace(run_trace.clone());
        telemetry.flush_pending_witness_logs();

        assert_eq!(
            run_trace.witness.as_ref().map(|witness| witness.witness_id),
            Some(bundle.witness_id)
        );
        assert!(
            telemetry.get_witness_bundle(&bundle.witness_id).is_some(),
            "witness bundle should be queryable"
        );
        let scorecard = telemetry
            .get_scorecard("runtime_main_path")
            .expect("scorecard stored");
        assert_eq!(scorecard.total_trials, 1);
        assert_eq!(scorecard.passed_trials, 1);
        let profiler = telemetry
            .get_run_profiler_artifact(&run_trace.run_id)
            .expect("profiler artifact stored");
        assert_eq!(profiler.run_id, run_trace.run_id);
        assert_eq!(profiler.suite_id.as_deref(), Some("runtime_main_path"));

        let reloaded =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        assert!(
            reloaded.get_witness_bundle(&bundle.witness_id).is_some(),
            "persisted witness bundle should reload"
        );
        assert!(
            reloaded.get_witness_log(&bundle.witness_id).is_some(),
            "persisted witness log should reload"
        );
        assert_eq!(
            reloaded
                .get_scorecard("runtime_main_path")
                .expect("persisted scorecard")
                .total_trials,
            1
        );
        assert!(reloaded
            .get_run_profiler_artifact(&run_trace.run_id)
            .is_some());
        cleanup_telemetry_root(&root);
    }

    #[test]
    fn telemetry_manager_capture_evaluation_tap_persists_mainline_artifacts() {
        let root = temp_telemetry_root();
        let telemetry =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let mut run_trace = tracer.start_run_trace();
        run_trace.task_id = Some(Uuid::new_v4());
        run_trace.status = TraceStatus::Succeeded;
        run_trace.finished_at = Some(run_trace.started_at + chrono::Duration::milliseconds(120));
        run_trace.stages.push(RuntimeStageTrace {
            stage: RuntimeStage::Ingress,
            status: TraceStatus::Succeeded,
            started_at: run_trace.started_at,
            finished_at: run_trace.finished_at,
            detail: Some("evaluation tap".to_string()),
            metadata: std::collections::HashMap::new(),
        });

        let bundle = telemetry.capture_evaluation_tap(&mut run_trace, Some("evaluation_tap_suite"));
        telemetry.flush_pending_witness_logs();

        assert!(telemetry.get_run_trace(&run_trace.run_id).is_some());
        assert!(telemetry.get_witness_bundle(&bundle.witness_id).is_some());
        assert!(telemetry.get_witness_log(&bundle.witness_id).is_some());
        assert!(telemetry
            .get_run_profiler_artifact(&run_trace.run_id)
            .is_some());
        assert!(telemetry.get_scorecard("evaluation_tap_suite").is_some());

        cleanup_telemetry_root(&root);
    }

    #[test]
    fn telemetry_manager_lists_scorecards_newest_first() {
        let root = temp_telemetry_root();
        let telemetry =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");

        let mut older = tracer.start_run_trace();
        older.started_at = older.started_at - chrono::Duration::seconds(10);
        let older_bundle = telemetry.attach_simulation_witness(&mut older, Some("alpha_suite"));
        telemetry.save_run_trace(older);
        telemetry.save_scorecard(SimulationHarness::upsert_scorecard(None, &older_bundle));

        let mut newer = tracer.start_run_trace();
        newer.started_at = newer.started_at + chrono::Duration::seconds(10);
        let newer_bundle = telemetry.attach_simulation_witness(&mut newer, Some("beta_suite"));
        telemetry.save_run_trace(newer);
        telemetry.save_scorecard(SimulationHarness::upsert_scorecard(None, &newer_bundle));

        let listed = telemetry.list_scorecards();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].scorecard_id, "beta_suite");
        assert_eq!(listed[1].scorecard_id, "alpha_suite");

        let reloaded =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        assert_eq!(reloaded.list_scorecards()[0].scorecard_id, "beta_suite");
        cleanup_telemetry_root(&root);
    }

    #[test]
    fn telemetry_manager_can_query_witness_logs() {
        let root = temp_telemetry_root();
        let telemetry =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let mut run_trace = tracer.start_run_trace();
        run_trace.task_id = Some(Uuid::new_v4());
        run_trace.provider = Some("openai".to_string());
        run_trace.model = Some("gpt-test".to_string());
        run_trace
            .metadata
            .insert("route".to_string(), "foreground_chat".to_string());
        run_trace
            .metadata
            .insert("policy_decision".to_string(), "allow".to_string());
        run_trace
            .metadata
            .insert("truth_status".to_string(), "Unverified".to_string());
        run_trace.metadata.insert(
            "verification_domain".to_string(),
            "ExecutionFact".to_string(),
        );
        run_trace.metadata.insert(
            "verification_requirement".to_string(),
            "Required".to_string(),
        );
        run_trace.metadata.insert(
            "verification_mode".to_string(),
            "ExecutionResultCheck".to_string(),
        );
        run_trace.metadata.insert(
            "verification_outcome".to_string(),
            "VerificationExecutionMissing".to_string(),
        );
        run_trace.metadata.insert(
            "source_posture".to_string(),
            "SourcesRequiredButMissing".to_string(),
        );
        run_trace.metadata.insert(
            "verification_last_tool".to_string(),
            "runtime_surface".to_string(),
        );
        run_trace.metadata.insert(
            "verification_answer_readiness".to_string(),
            "search_results_only".to_string(),
        );
        run_trace.metadata.insert(
            "verification_route_reason".to_string(),
            "external_fact_requires_search_then_source_read".to_string(),
        );
        run_trace.metadata.insert(
            "verification_continuation".to_string(),
            "ContinueFetchOrBrowse".to_string(),
        );
        run_trace.metadata.insert(
            "verification_termination".to_string(),
            "TentativeOnly".to_string(),
        );
        run_trace.metadata.insert(
            "verification_requires_followup".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "verification_can_finalize_answer".to_string(),
            "false".to_string(),
        );
        run_trace.metadata.insert(
            "verification_next_tools".to_string(),
            "web_fetch".to_string(),
        );
        run_trace
            .metadata
            .insert("verification_cite_required".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "engram_windows_native_embed_outcome".to_string(),
            "fallback_runtime_active".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_strategy".to_string(),
            "migrate_to_windows_native_runtime".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_outcome".to_string(),
            "windows_native_active".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_strategy".to_string(),
            "active".to_string(),
        );
        run_trace
            .degradation_notes
            .push("budget_exhausted".to_string());
        run_trace.status = TraceStatus::Failed;
        run_trace.stages.push(RuntimeStageTrace {
            stage: RuntimeStage::Governance,
            status: TraceStatus::Failed,
            started_at: chrono::Utc::now(),
            finished_at: Some(chrono::Utc::now()),
            detail: Some("budget exhausted".to_string()),
            metadata: std::collections::HashMap::new(),
        });

        let bundle = telemetry.attach_simulation_witness(&mut run_trace, Some("runtime_main_path"));
        telemetry.save_run_trace(run_trace.clone());
        telemetry.flush_pending_witness_logs();

        let queried = telemetry.query_witness_logs(&WitnessLogQuery {
            suite_id: Some("runtime_main_path".to_string()),
            provider: Some("openai".to_string()),
            degraded: Some(true),
            budget_exhausted: Some(true),
            limit: Some(10),
            ..Default::default()
        });
        assert_eq!(queried.len(), 1);
        assert_eq!(queried[0].witness_id, bundle.witness_id);
        assert_eq!(queried[0].route.as_deref(), Some("foreground_chat"));
        assert_eq!(queried[0].policy_decision.as_deref(), Some("allow"));
        assert_eq!(
            telemetry
                .query_witness_logs(&WitnessLogQuery {
                    text: Some("budget exhausted".to_string()),
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            1
        );
        assert_eq!(
            telemetry
                .query_witness_logs(&WitnessLogQuery {
                    text: Some("gpt-test".to_string()),
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            1
        );
        assert_eq!(
            telemetry
                .query_witness_logs(&WitnessLogQuery {
                    truth_verification: TruthVerificationQueryFields {
                        truth_status: Some("Unverified".to_string()),
                        verification_outcome: Some("VerificationExecutionMissing".to_string()),
                        verification_last_tool: Some("runtime_surface".to_string()),
                        verification_answer_readiness: Some("search_results_only".to_string()),
                        verification_route_reason: Some(
                            "external_fact_requires_search_then_source_read".to_string(),
                        ),
                        verification_continuation: Some("ContinueFetchOrBrowse".to_string(),),
                        verification_termination: Some("TentativeOnly".to_string()),
                        verification_requires_followup: Some("true".to_string()),
                        verification_can_finalize_answer: Some("false".to_string()),
                        verification_next_tools: Some("web_fetch".to_string()),
                        verification_cite_required: Some("true".to_string()),
                        ..Default::default()
                    },
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            1
        );
        assert_eq!(
            telemetry
                .query_witness_logs(&WitnessLogQuery {
                    windows_native: WindowsNativeQueryFields {
                        windows_native_embed_outcome: Some("fallback_runtime_active".to_string()),
                        ..Default::default()
                    },
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            1
        );
        assert_eq!(
            telemetry
                .query_witness_logs(&WitnessLogQuery {
                    windows_native: WindowsNativeQueryFields {
                        windows_native_embed_class: Some("fallback_runtime".to_string()),
                        ..Default::default()
                    },
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            1
        );
        assert_eq!(
            telemetry
                .query_witness_logs(&WitnessLogQuery {
                    windows_native: WindowsNativeQueryFields {
                        windows_native_rerank_strategy: Some("active".to_string()),
                        ..Default::default()
                    },
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            1
        );
        assert_eq!(
            telemetry
                .query_witness_logs(&WitnessLogQuery {
                    windows_native: WindowsNativeQueryFields {
                        windows_native_failure_reason: Some(
                            "windows_native::embed::fallback_runtime_active".to_string()
                        ),
                        ..Default::default()
                    },
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            1
        );

        let reloaded =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        assert_eq!(
            reloaded
                .query_witness_logs(&WitnessLogQuery {
                    suite_id: Some("runtime_main_path".to_string()),
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            1
        );
        cleanup_telemetry_root(&root);
    }

    #[test]
    fn telemetry_manager_can_query_scorecards_by_windows_native_role_results() {
        let root = temp_telemetry_root();
        let telemetry =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let mut run_trace = tracer.start_run_trace();
        run_trace
            .metadata
            .insert("truth_status".to_string(), "Unverified".to_string());
        run_trace.metadata.insert(
            "verification_domain".to_string(),
            "ExecutionFact".to_string(),
        );
        run_trace.metadata.insert(
            "verification_requirement".to_string(),
            "Required".to_string(),
        );
        run_trace.metadata.insert(
            "verification_mode".to_string(),
            "ExecutionResultCheck".to_string(),
        );
        run_trace.metadata.insert(
            "verification_outcome".to_string(),
            "VerificationExecutionMissing".to_string(),
        );
        run_trace.metadata.insert(
            "source_posture".to_string(),
            "SourcesRequiredButMissing".to_string(),
        );
        run_trace.metadata.insert(
            "verification_last_tool".to_string(),
            "runtime_surface".to_string(),
        );
        run_trace.metadata.insert(
            "verification_answer_readiness".to_string(),
            "search_results_only".to_string(),
        );
        run_trace.metadata.insert(
            "verification_route_reason".to_string(),
            "external_fact_requires_search_then_source_read".to_string(),
        );
        run_trace.metadata.insert(
            "verification_continuation".to_string(),
            "ContinueFetchOrBrowse".to_string(),
        );
        run_trace.metadata.insert(
            "verification_termination".to_string(),
            "TentativeOnly".to_string(),
        );
        run_trace.metadata.insert(
            "verification_requires_followup".to_string(),
            "true".to_string(),
        );
        run_trace.metadata.insert(
            "verification_can_finalize_answer".to_string(),
            "false".to_string(),
        );
        run_trace.metadata.insert(
            "verification_next_tools".to_string(),
            "web_fetch".to_string(),
        );
        run_trace
            .metadata
            .insert("verification_cite_required".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "engram_windows_native_embed_outcome".to_string(),
            "fallback_runtime_active".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_embed_strategy".to_string(),
            "migrate_to_windows_native_runtime".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_outcome".to_string(),
            "windows_native_active".to_string(),
        );
        run_trace.metadata.insert(
            "engram_windows_native_rerank_strategy".to_string(),
            "active".to_string(),
        );

        let bundle = telemetry.attach_simulation_witness(&mut run_trace, Some("windows_suite"));
        telemetry.save_run_trace(run_trace);
        telemetry.flush_pending_witness_logs();

        let queried = telemetry.query_scorecards(&ScorecardQuery {
            suite_id: Some("windows_suite".to_string()),
            truth_verification: TruthVerificationQueryFields {
                truth_status: Some("Unverified".to_string()),
                verification_outcome: Some("VerificationExecutionMissing".to_string()),
                ..Default::default()
            },
            windows_native: WindowsNativeQueryFields {
                windows_native_embed_outcome: Some("fallback_runtime_active".to_string()),
                ..Default::default()
            },
            limit: Some(10),
            ..Default::default()
        });
        assert_eq!(queried.len(), 1);
        assert_eq!(queried[0].scorecard_id, "windows_suite");
        assert!(queried[0]
            .entries
            .iter()
            .any(|entry| entry.witness_id == bundle.witness_id));

        let rerank_queried = telemetry.query_scorecards(&ScorecardQuery {
            windows_native: WindowsNativeQueryFields {
                windows_native_rerank_strategy: Some("active".to_string()),
                ..Default::default()
            },
            limit: Some(10),
            ..Default::default()
        });
        assert_eq!(rerank_queried.len(), 1);
        let truth_queried = telemetry.query_scorecards(&ScorecardQuery {
            truth_verification: TruthVerificationQueryFields {
                verification_last_tool: Some("runtime_surface".to_string()),
                source_posture: Some("SourcesRequiredButMissing".to_string()),
                verification_answer_readiness: Some("search_results_only".to_string()),
                verification_route_reason: Some(
                    "external_fact_requires_search_then_source_read".to_string(),
                ),
                verification_continuation: Some("ContinueFetchOrBrowse".to_string()),
                verification_termination: Some("TentativeOnly".to_string()),
                verification_requires_followup: Some("true".to_string()),
                verification_can_finalize_answer: Some("false".to_string()),
                verification_next_tools: Some("web_fetch".to_string()),
                verification_cite_required: Some("true".to_string()),
                ..Default::default()
            },
            limit: Some(10),
            ..Default::default()
        });
        assert_eq!(truth_queried.len(), 1);
        let class_queried = telemetry.query_scorecards(&ScorecardQuery {
            windows_native: WindowsNativeQueryFields {
                windows_native_embed_class: Some("fallback_runtime".to_string()),
                ..Default::default()
            },
            limit: Some(10),
            ..Default::default()
        });
        assert_eq!(class_queried.len(), 1);
        let failure_reason_queried = telemetry.query_scorecards(&ScorecardQuery {
            windows_native: WindowsNativeQueryFields {
                windows_native_failure_reason: Some(
                    "windows_native::embed::fallback_runtime_active".to_string(),
                ),
                ..Default::default()
            },
            limit: Some(10),
            ..Default::default()
        });
        assert_eq!(failure_reason_queried.len(), 1);

        cleanup_telemetry_root(&root);
    }

    #[test]
    fn telemetry_manager_batches_and_prunes_witness_logs() {
        let root = temp_telemetry_root();
        let telemetry = TelemetryManager::with_storage_config(
            TelemetryLevel::Production,
            Some(root.clone()),
            8,
            8,
            Duration::from_secs(60),
            2,
        );
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");

        let make_log = |idx: usize| {
            let mut run_trace = tracer.start_run_trace();
            run_trace.provider = Some("openai".to_string());
            run_trace
                .metadata
                .insert("route".to_string(), format!("case-{idx}"));
            let bundle = SimulationHarness::build_witness_bundle(&run_trace, "retention_suite");
            SimulationHarness::build_witness_log_entry(&run_trace, &bundle)
        };

        telemetry.save_witness_log(make_log(1));
        telemetry.save_witness_log(make_log(2));
        telemetry.save_witness_log(make_log(3));
        assert_eq!(
            telemetry
                .query_witness_logs(&WitnessLogQuery {
                    suite_id: Some("retention_suite".to_string()),
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            3
        );

        telemetry.flush_pending_witness_logs();

        let reloaded = TelemetryManager::with_storage_config(
            TelemetryLevel::Production,
            Some(root.clone()),
            8,
            8,
            Duration::from_secs(60),
            2,
        );
        assert_eq!(
            reloaded
                .query_witness_logs(&WitnessLogQuery {
                    suite_id: Some("retention_suite".to_string()),
                    limit: Some(10),
                    ..Default::default()
                })
                .len(),
            2
        );
        cleanup_telemetry_root(&root);
    }

    #[test]
    fn telemetry_manager_exports_profiler_artifacts() {
        let root = temp_telemetry_root();
        let telemetry =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");

        let mut run_trace = tracer.start_run_trace();
        run_trace.task_id = Some(Uuid::new_v4());
        run_trace.prompt_tokens = Some(144);
        run_trace.completion_tokens = Some(56);
        run_trace.metadata.insert(
            "profiler.memory.peak_rss_bytes".to_string(),
            "4096".to_string(),
        );
        let bundle = telemetry.attach_simulation_witness(&mut run_trace, Some("export_suite"));
        telemetry.save_run_trace(run_trace.clone());

        let queried = telemetry.query_profiler_artifacts(&ProfilerArtifactQuery {
            suite_id: Some("export_suite".to_string()),
            run_id: Some(run_trace.run_id),
            trace_id: Some(bundle.trace_id),
            witness_id: Some(bundle.witness_id),
            benchmark_fingerprint: Some(bundle.benchmark_fingerprint.fingerprint.clone()),
            limit: Some(10),
        });
        assert_eq!(queried.len(), 1);
        assert_eq!(queried[0].memory.peak_rss_bytes, Some(4096));

        let export = telemetry.export_profiler_artifacts(&ProfilerArtifactQuery {
            suite_id: Some("export_suite".to_string()),
            ..Default::default()
        });
        assert_eq!(export.schema_version, PROFILER_EXPORT_SCHEMA_VERSION);
        assert_eq!(export.artifacts.len(), 1);
        assert_eq!(export.artifacts[0].run_id, run_trace.run_id);
        assert_eq!(
            export.artifacts[0]
                .benchmark_fingerprint
                .as_ref()
                .map(|item| item.fingerprint.as_str()),
            Some(bundle.benchmark_fingerprint.fingerprint.as_str())
        );

        let reloaded =
            TelemetryManager::with_storage_root(TelemetryLevel::Production, Some(root.clone()));
        assert_eq!(
            reloaded
                .query_profiler_artifacts(&ProfilerArtifactQuery {
                    suite_id: Some("export_suite".to_string()),
                    ..Default::default()
                })
                .len(),
            1
        );
        cleanup_telemetry_root(&root);
    }

    fn temp_telemetry_root() -> PathBuf {
        env::temp_dir().join(format!("benshu-telemetry-test-{}", Uuid::new_v4()))
    }

    fn cleanup_telemetry_root(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }
}
