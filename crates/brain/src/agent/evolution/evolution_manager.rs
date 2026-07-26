use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::agent::evolution::auditor::Auditor;
use crate::agent::evolution::observation::{ObservationStatus, ObservationWindow};
use benshu_experience_core::{
    EvidenceRefs, ExperienceScope, ExperienceStep, ExperienceStore, TaskExperience,
};

/// Manages governed background learning for an agent.
///
/// Coordinates experience mining, observation windows, and failure analysis.
/// It does not rewrite agent identity files or register new tools.
pub struct EvolutionManager {
    auditor: Arc<Auditor>,
    observation_window: Arc<parking_lot::RwLock<ObservationWindow>>,
    _base_dir: PathBuf,
    memory: Arc<parking_lot::RwLock<Option<Arc<dyn crate::agent::memory::Memory>>>>,
    experience_store: Arc<parking_lot::RwLock<Option<Arc<ExperienceStore>>>>,
    /// Phase 14.3: Background task queue for evolution learning (Backpressure support)
    task_tx: tokio::sync::mpsc::Sender<EvolutionTask>,
    /// Phase 14.3: Receiver for background tasks (Wrapped for Arc-based worker startup)
    task_rx: Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Receiver<EvolutionTask>>>>,
    /// Phase 15.3: Metabolic-aware sensor for hardware-adaptive scheduling
    sensor: Arc<
        parking_lot::RwLock<
            Option<Arc<tokio::sync::Mutex<dyn benshu_infra::traits::resource::ResourceSensor>>>,
        >,
    >,
    /// Phase 15.3: Configurable or auto-adaptive threshold
    metabolic_threshold: Arc<parking_lot::RwLock<Option<f32>>>,
    /// Managed worker lifecycle
    worker_started: Arc<AtomicBool>,
    worker_shutdown: CancellationToken,
    worker_handle: Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>,
}

enum EvolutionTask {
    Experience {
        history: Vec<crate::agent::message::Message>,
        outcome: crate::agent::protocol::ChatOutcome,
        agent_id: String,
    },
    Failure {
        tool: String,
        args: String,
        error: String,
        context: Vec<crate::agent::message::Message>,
    },
}

impl EvolutionManager {
    pub fn new(auditor: Arc<Auditor>, base_dir: PathBuf) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel::<EvolutionTask>(64); // Capacity 64

        Self {
            auditor,
            observation_window: Arc::new(parking_lot::RwLock::new(ObservationWindow::default())),
            _base_dir: base_dir,
            memory: Arc::new(parking_lot::RwLock::new(None)),
            experience_store: Arc::new(parking_lot::RwLock::new(None)),
            task_tx: tx,
            task_rx: Arc::new(tokio::sync::Mutex::new(Some(rx))),
            sensor: Arc::new(parking_lot::RwLock::new(None)),
            metabolic_threshold: Arc::new(parking_lot::RwLock::new(None)),
            worker_started: Arc::new(AtomicBool::new(false)),
            worker_shutdown: CancellationToken::new(),
            worker_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Create a fresh manager with the same long-lived dependencies but isolated runtime state.
    pub fn fork(&self) -> Arc<Self> {
        let fork = Self::new(self.auditor.clone(), self._base_dir.clone());
        if let Some(store) = self.experience_store.read().clone() {
            fork.set_experience_store(store);
        }
        if let Some(threshold) = *self.metabolic_threshold.read() {
            fork.set_metabolic_threshold(threshold);
        }
        Arc::new(fork)
    }

    /// Initialize the managed background worker.
    pub fn start_worker(&self) {
        if self.worker_started.swap(true, Ordering::AcqRel) {
            return;
        }

        let rx = match self.task_rx.try_lock() {
            Ok(mut lock) => lock.take(),
            Err(_) => None,
        };

        let Some(mut rx) = rx else {
            self.worker_started.store(false, Ordering::Release);
            warn!("Evolution: worker start requested, but receiver was unavailable.");
            return;
        };

        let shutdown = self.worker_shutdown.clone();
        let auditor = self.auditor.clone();
        let observation_window = self.observation_window.clone();
        let memory = self.memory.clone();
        let experience_store = self.experience_store.clone();
        let sensor = self.sensor.clone();
        let metabolic_threshold = self.metabolic_threshold.clone();
        let worker_started = self.worker_started.clone();

        let handle = tokio::spawn(async move {
            info!("Evolution: Background worker started with capacity 64.");
            let mut pending_task: Option<EvolutionTask> = None;

            loop {
                let task = if let Some(task) = pending_task.take() {
                    task
                } else {
                    tokio::select! {
                        _ = shutdown.cancelled() => break,
                        maybe_task = rx.recv() => {
                            match maybe_task {
                                Some(task) => task,
                                None => break,
                            }
                        }
                    }
                };

                let throttle = Self::current_throttle(&sensor, &metabolic_threshold).await;
                match throttle {
                    benshu_infra::traits::resource::ThrottleLevel::Low => {
                        warn!("Evolution: Critical resource pressure detected. Delaying background task for 5 seconds.");
                        pending_task = Some(task);
                        tokio::select! {
                            _ = shutdown.cancelled() => break,
                            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                        }
                        continue;
                    }
                    benshu_infra::traits::resource::ThrottleLevel::Medium => {
                        debug!("Evolution: Moderate resource pressure. Throttling task processing speed.");
                        tokio::select! {
                            _ = shutdown.cancelled() => break,
                            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
                        }
                    }
                    benshu_infra::traits::resource::ThrottleLevel::High => {}
                }

                if let Err(err) = Self::process_task(
                    task,
                    auditor.clone(),
                    observation_window.clone(),
                    memory.clone(),
                    experience_store.clone(),
                )
                .await
                {
                    warn!("Evolution: Background task failed: {}", err);
                }
            }

            worker_started.store(false, Ordering::Release);
            info!("Evolution: Background worker stopped.");
        });

        if let Ok(mut slot) = self.worker_handle.try_lock() {
            *slot = Some(handle);
        } else {
            warn!("Evolution: failed to store worker handle; worker will rely on shutdown signal only.");
        }
    }

    pub fn signal_shutdown(&self) {
        self.worker_shutdown.cancel();
    }

    pub async fn shutdown_worker(&self) {
        self.signal_shutdown();
        if let Some(handle) = self.worker_handle.lock().await.take() {
            let _ = handle.await;
        }
    }

    pub fn is_worker_running(&self) -> bool {
        self.worker_started.load(Ordering::Acquire)
    }

    /// Enqueue a learning task with backpressure support
    pub fn enqueue_learning(
        &self,
        history: Vec<crate::agent::message::Message>,
        outcome: crate::agent::protocol::ChatOutcome,
        agent_id: String,
    ) {
        match self.task_tx.try_send(EvolutionTask::Experience {
            history,
            outcome,
            agent_id: agent_id.clone(),
        }) {
            Ok(_) => debug!("[EVOLUTION][{}] Task enqueued", agent_id),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                warn!(
                    "[EVOLUTION][{}] Evolution queue full - dropping learning task",
                    agent_id
                );
            }
            Err(e) => warn!("[EVOLUTION][{}] Failed to enqueue task: {}", agent_id, e),
        }
    }

    /// Enqueue a failure analysis task with backpressure support
    pub fn enqueue_failure_analysis(
        &self,
        tool: String,
        args: String,
        error: String,
        context: Vec<crate::agent::message::Message>,
    ) {
        if let Err(e) = self.task_tx.try_send(EvolutionTask::Failure {
            tool,
            args,
            error,
            context,
        }) {
            warn!("Evolution: Failed to enqueue failure analysis: {}", e);
        }
    }

    pub fn try_set_memory(&self, memory: Arc<dyn crate::agent::memory::Memory>) -> Result<()> {
        let mut guard = self.memory.write();
        match guard.as_ref() {
            None => {
                *guard = Some(memory);
                Ok(())
            }
            Some(existing) if Arc::ptr_eq(existing, &memory) => Ok(()),
            Some(_) => Err(anyhow!(
                "EvolutionManager memory binding already initialized"
            )),
        }
    }

    pub fn set_memory(&self, memory: Arc<dyn crate::agent::memory::Memory>) {
        if let Err(err) = self.try_set_memory(memory) {
            warn!("Evolution: {}", err);
        }
    }

    pub fn try_set_experience_store(&self, store: Arc<ExperienceStore>) -> Result<()> {
        let mut guard = self.experience_store.write();
        match guard.as_ref() {
            None => {
                *guard = Some(store);
                Ok(())
            }
            Some(existing) if Arc::ptr_eq(existing, &store) => Ok(()),
            Some(_) => Err(anyhow::anyhow!(
                "EvolutionManager experience store binding already initialized"
            )),
        }
    }

    pub fn set_experience_store(&self, store: Arc<ExperienceStore>) {
        if let Err(err) = self.try_set_experience_store(store) {
            warn!("Evolution: {}", err);
        }
    }

    pub fn try_set_metabolic_threshold(&self, threshold: f32) -> Result<()> {
        let mut guard = self.metabolic_threshold.write();
        match *guard {
            None => {
                *guard = Some(threshold);
                Ok(())
            }
            Some(existing) if (existing - threshold).abs() < f32::EPSILON => Ok(()),
            Some(_) => Err(anyhow!(
                "EvolutionManager metabolic threshold already initialized"
            )),
        }
    }

    pub fn set_metabolic_threshold(&self, threshold: f32) {
        if let Err(err) = self.try_set_metabolic_threshold(threshold) {
            warn!("Evolution: {}", err);
        }
    }

    pub fn try_set_sensor(
        &self,
        sensor: Arc<tokio::sync::Mutex<dyn benshu_infra::traits::resource::ResourceSensor>>,
    ) -> Result<()> {
        let mut guard = self.sensor.write();
        match guard.as_ref() {
            None => {
                *guard = Some(sensor);
                Ok(())
            }
            Some(existing) if Arc::ptr_eq(existing, &sensor) => Ok(()),
            Some(_) => Err(anyhow!(
                "EvolutionManager sensor binding already initialized"
            )),
        }
    }

    pub fn set_sensor(
        &self,
        sensor: Arc<tokio::sync::Mutex<dyn benshu_infra::traits::resource::ResourceSensor>>,
    ) {
        if let Err(err) = self.try_set_sensor(sensor) {
            warn!("Evolution: {}", err);
        }
    }

    pub fn observation_window(&self) -> Arc<parking_lot::RwLock<ObservationWindow>> {
        self.observation_window.clone()
    }

    pub fn auditor(&self) -> Arc<Auditor> {
        self.auditor.clone()
    }

    pub async fn current_background_throttle(
        &self,
    ) -> benshu_infra::traits::resource::ThrottleLevel {
        Self::current_throttle(&self.sensor, &self.metabolic_threshold).await
    }

    async fn current_throttle(
        sensor: &Arc<
            parking_lot::RwLock<
                Option<Arc<tokio::sync::Mutex<dyn benshu_infra::traits::resource::ResourceSensor>>>,
            >,
        >,
        metabolic_threshold: &Arc<parking_lot::RwLock<Option<f32>>>,
    ) -> benshu_infra::traits::resource::ThrottleLevel {
        let (sensor_mutex, threshold) = {
            let sensor_guard = sensor.read();
            let threshold_guard = metabolic_threshold.read();
            (sensor_guard.clone(), *threshold_guard)
        };

        if let Some(mutex) = sensor_mutex {
            let mut sensor = mutex.lock().await;
            sensor.suggest_throttle_level(threshold)
        } else {
            benshu_infra::traits::resource::ThrottleLevel::High
        }
    }

    async fn process_task(
        task: EvolutionTask,
        auditor: Arc<Auditor>,
        observation_window: Arc<parking_lot::RwLock<ObservationWindow>>,
        memory: Arc<parking_lot::RwLock<Option<Arc<dyn crate::agent::memory::Memory>>>>,
        experience_store: Arc<parking_lot::RwLock<Option<Arc<ExperienceStore>>>>,
    ) -> Result<()> {
        let isolated = Self {
            auditor,
            observation_window,
            _base_dir: PathBuf::new(),
            memory,
            experience_store,
            task_tx: tokio::sync::mpsc::channel::<EvolutionTask>(1).0,
            task_rx: Arc::new(tokio::sync::Mutex::new(None)),
            sensor: Arc::new(parking_lot::RwLock::new(None)),
            metabolic_threshold: Arc::new(parking_lot::RwLock::new(None)),
            worker_started: Arc::new(AtomicBool::new(false)),
            worker_shutdown: CancellationToken::new(),
            worker_handle: Arc::new(tokio::sync::Mutex::new(None)),
        };

        match task {
            EvolutionTask::Experience {
                history,
                outcome,
                agent_id,
            } => match isolated.learn_from_experience(&history, &outcome).await {
                Ok(count) if count > 0 => info!(
                    "[EVOLUTION][{}] Background: Learned {} patterns",
                    agent_id, count
                ),
                Ok(_) => {}
                Err(e) => warn!(
                    "[EVOLUTION][{}] Background: Learning failed: {}",
                    agent_id, e
                ),
            },
            EvolutionTask::Failure {
                tool,
                args,
                error,
                context,
            } => {
                if let Err(e) = isolated
                    .report_failure(&tool, &args, &error, &context)
                    .await
                {
                    warn!("Evolution: Background failure analysis failed: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Priority 4: Learn from a successful task execution
    pub async fn learn_from_experience(
        &self,
        history: &[crate::agent::message::Message],
        outcome: &crate::agent::protocol::ChatOutcome,
    ) -> Result<usize> {
        let miner = crate::agent::evolution::experience::ExperienceMiner::new(
            self.auditor.provider(),
            self.auditor.model().to_string(),
        );

        match miner.distill(history, outcome).await {
            Ok(entry) => {
                info!(
                    "Evolution: Successfully distilled experience for pattern: {}",
                    entry.problem_description
                );

                // Phase 14: Save to Memory (Engram)
                let memory_opt = self.memory.read().clone();
                if let Some(memory) = &memory_opt {
                    let mut exp_json = serde_json::to_value(&entry)?;
                    // Add success score (Phase 14)
                    exp_json["success_score"] = serde_json::json!(1.0); // Successful task
                    if let Err(e) = memory.store_experience(exp_json).await {
                        warn!(
                            "Evolution: Failed to store experience for pattern '{}': {}",
                            entry.problem_description, e
                        );
                    }
                }

                if let Some(store) = self.experience_store.read().clone() {
                    let experience = Self::task_experience_from_entry(&entry, outcome);
                    if let Err(error) = store.upsert(experience) {
                        warn!(
                            "Evolution: Failed to store experience record in experience.redb: {}",
                            error
                        );
                    }
                }

                let pattern_count = entry.successful_path.len();

                // Reward used patterns without synthesizing or registering new tools.
                if let Some(memory) = &memory_opt {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    for msg in history {
                        for exp_id in &msg.used_experience_ids {
                            if let Ok(Some(mut exp_val)) = memory.get_experience(exp_id).await {
                                let last_updated = exp_val["last_updated_at"].as_u64().unwrap_or(0);

                                // Debounce utility updates (Wait at least 60s between rewards for the same experience)
                                if (now - last_updated) > 60 {
                                    if let Err(e) =
                                        memory.increment_experience_utility(exp_id, 0.1).await
                                    {
                                        warn!(
                                            "Evolution: Failed to reward experience '{}': {}",
                                            exp_id, e
                                        );
                                    }
                                    exp_val["last_updated_at"] = serde_json::json!(now);
                                    if let Err(e) = memory.store_experience(exp_val.clone()).await {
                                        warn!("Evolution: Failed to update experience record '{}': {}", exp_id, e);
                                    }
                                }
                            }
                        }
                        for ap_id in &msg.used_anti_pattern_ids {
                            if let Err(e) = memory.increment_anti_pattern_utility(ap_id, 0.1).await
                            {
                                warn!(
                                    "Evolution: Failed to reward anti-pattern '{}': {}",
                                    ap_id, e
                                );
                            }
                        }
                    }
                }

                Ok(pattern_count)
            }
            Err(e) => {
                warn!("Evolution: Failed to distill experience: {}", e);
                Ok(0)
            }
        }
    }

    fn task_experience_from_entry(
        entry: &crate::agent::evolution::experience::ExperienceEntry,
        outcome: &crate::agent::protocol::ChatOutcome,
    ) -> TaskExperience {
        let mut experience = TaskExperience::new(
            entry.problem_description.clone(),
            entry.problem_description.clone(),
            ExperienceScope::Agent,
        );
        experience.successful_steps = entry
            .successful_path
            .iter()
            .enumerate()
            .map(|(index, step)| ExperienceStep {
                label: format!("step_{}", index + 1),
                action: step.clone(),
                evidence_ref: None,
            })
            .collect();
        experience.anti_patterns = entry.anti_patterns.clone();
        experience.hints = entry.lessons_learned.clone();
        experience.confidence = 0.6;
        experience.evidence_refs = EvidenceRefs {
            trace_id: outcome
                .run_trace
                .as_ref()
                .map(|trace| trace.run_id.to_string())
                .or_else(|| {
                    outcome
                        .runtime_task
                        .as_ref()
                        .and_then(|task| task.trace_id.map(|id| id.to_string()))
                }),
            task_id: outcome
                .runtime_task
                .as_ref()
                .map(|task| task.id.to_string()),
            run_id: outcome
                .runtime_task
                .as_ref()
                .and_then(|task| task.run_id.map(|id| id.to_string())),
            ..EvidenceRefs::default()
        };
        experience.metadata.insert(
            "source".to_string(),
            "evolution_experience_miner".to_string(),
        );
        if let Some(signature) = &entry.metabolic_signature {
            experience.metadata.insert(
                "avg_latency_ms".to_string(),
                signature.avg_latency_ms.to_string(),
            );
            experience.metadata.insert(
                "max_cpu_pressure".to_string(),
                signature.max_cpu_pressure.to_string(),
            );
            experience.metadata.insert(
                "max_vram_pressure".to_string(),
                signature.max_vram_pressure.to_string(),
            );
        }
        experience
    }

    /// Record an error that might be related to a recent evolution.
    pub fn report_error(&self, error_type: &str) {
        let mut window = self.observation_window.write();
        let active_ids: Vec<String> = window
            .active_observations()
            .iter()
            .map(|o| o.id.clone())
            .collect();
        for id in &active_ids {
            window.record_error(id);
            error!(
                "Evolution: Error reported during observation '{}': {}",
                id, error_type
            );
        }
    }

    /// Phase 14: Reflexion 2.0 - Analyze and report a tool execution failure
    pub async fn report_failure(
        &self,
        tool: &str,
        args: &str,
        error: &str,
        context: &[crate::agent::message::Message],
    ) -> Result<()> {
        // Build a mini-transcript for failure analysis
        let mut transcript = String::new();
        for m in context.iter().rev().take(5).rev() {
            transcript.push_str(&format!("{:?}: {}\n", m.role, m.content.as_text()));
        }

        let prompt = format!(
            "### REFLEXION 2.0: FAILURE ANALYSIS\n\
             Review this tool execution failure and identify the 'Anti-Pattern'.\n\n\
             CONTEXT:\n{}\n\n\
             FAILED ACTION:\nTool: {}\nArgs: {}\nError: {}\n\n\
             INSTRUCTIONS:\n\
             1. Identify the 'Error Fingerprint' (A unique pattern in the error message or context).\n\
             2. Determine the 'Root Cause' (Why did this specific sequence of actions fail?).\n\
             3. Propose a 'Correction' (What should be done instead?).\n\n\
             OUTPUT FORMAT: Respond with a JSON object:\n\
             {{\n  \"fingerprint\": \"...\",\n  \"cause\": \"...\",\n  \"fix\": \"...\"\n}}",
            transcript, tool, args, error
        );

        let request = crate::agent::provider::ChatRequest {
            model: self.auditor.model().to_string(),
            messages: vec![crate::agent::message::Message::user(prompt)],
            temperature: Some(0.1),
            ..Default::default()
        };

        if let Ok(stream) = self.auditor.provider().stream_completion(request).await {
            let full_text = stream.collect_text().await.unwrap_or_default();
            if let Some(json_start) = full_text.find('{') {
                if let Some(json_end) = full_text.rfind('}') {
                    let json_str = &full_text[json_start..=json_end];
                    match serde_json::from_str::<
                        crate::agent::evolution::experience::AntiPatternUpdate,
                    >(json_str)
                    {
                        Ok(update) => {
                            info!(
                                "Evolution[Reflexion]: Identified new anti-pattern for tool {}: {}",
                                tool, update.cause
                            );

                            let memory_opt = self.memory.read().clone();
                            if let Some(memory) = memory_opt {
                                let mut ap_json = serde_json::to_value(&update)?;
                                ap_json["metadata"] = serde_json::json!({ "tool": tool });
                                if let Err(e) = memory.store_anti_pattern(ap_json).await {
                                    warn!(
                                        "Evolution[Reflexion]: Failed to store anti-pattern: {}",
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            error!("Evolution[Reflexion]: Failed to parse anti-pattern JSON: {} (Content: {})", e, json_str);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if any active observations have failed and need rollback.
    pub async fn check_evolution_health(&self) -> Result<()> {
        let (failed_ids, healthy_ids) = {
            let window = self.observation_window.write();
            let mut failed = Vec::new();
            let mut healthy = Vec::new();

            for obs in window.active_observations() {
                // Safety: Observation Hard Timeout (2 hours)
                if obs.started_at.elapsed().as_secs() > 7200 {
                    failed.push((
                        obs.id.clone(),
                        "SAFETY TIMEOUT: Observation window exceeded 2 hours.".to_string(),
                    ));
                    continue;
                }

                match window.check_health(&obs.id) {
                    ObservationStatus::Failed { reason } => {
                        failed.push((obs.id.clone(), reason));
                    }
                    ObservationStatus::Healthy => {
                        healthy.push(obs.id.clone());
                    }
                    _ => {}
                }
            }
            (failed, healthy)
        };

        // Handle Healthy (outside of lock)
        {
            let mut window = self.observation_window.write();
            for id in healthy_ids {
                info!(
                    "Evolution: Observation window '{}' completed successfully.",
                    id
                );
                window.complete(&id);
            }
        }

        // Handle Failures.
        for (id, reason) in failed_ids {
            error!("Evolution: Observation window '{}' FAILED: {}.", id, reason);

            // Mark as complete/failed in window
            {
                let mut window = self.observation_window.write();
                window.complete(&id);
            }
        }
        Ok(())
    }

    pub fn with_observation_window_config(
        self,
        duration: std::time::Duration,
        threshold: u32,
    ) -> Self {
        *self.observation_window.write() =
            ObservationWindow::with_duration_and_threshold(duration, threshold);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::memory::InMemoryMemory;
    use crate::agent::provider::MockProvider;

    #[tokio::test]
    async fn test_evolution_manager_learn_from_experience() {
        let provider = Arc::new(MockProvider::new(
            r#"{"problem_description": "test", "successful_path": ["step1"], "key_parameters": [], "anti_patterns": [], "lessons_learned": [], "timestamp": "2026-03-14T12:00:00Z"}"#,
        ));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let base_dir = tempfile::tempdir().unwrap().path().to_path_buf();
        let manager = EvolutionManager::new(auditor, base_dir);

        let memory = Arc::new(InMemoryMemory::new());
        manager.set_memory(Arc::clone(&memory) as Arc<dyn crate::agent::memory::Memory>);

        let history = vec![crate::agent::message::Message::user("do something")];
        let outcome = crate::agent::protocol::ChatOutcome {
            response: "Evolution cycle complete.".to_string(),
            thoughts: Vec::new(),
            tool_calls: Vec::new(),
            metabolic_stats: None,
            ownership: crate::agent::protocol::TaskOwnership::direct(
                crate::agent::multi_agent::AgentRole::Custom("benshu".to_string()),
                None,
            ),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        };

        let result = manager
            .learn_from_experience(&history, &outcome)
            .await
            .unwrap();
        assert_eq!(result, 1);
    }

    #[tokio::test]
    async fn test_evolution_manager_observation_flow() {
        let provider = Arc::new(MockProvider::new("APPROVED"));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let base_dir = tempfile::tempdir().unwrap().path().to_path_buf();
        let manager = EvolutionManager::new(auditor, base_dir);

        // 1. Enter observation
        {
            let window_lock = manager.observation_window();
            let mut window = window_lock.write();
            window.enter_observation("test-obs", "testing");
        }

        // 2. Health check (should be active)
        manager.check_evolution_health().await.unwrap();
        {
            let window_lock = manager.observation_window();
            let window = window_lock.read();
            assert!(window.is_active());
        }

        // 3. Force error threshold
        {
            let window_lock = manager.observation_window();
            let mut window = window_lock.write();
            for _ in 0..6 {
                window.record_error("test-obs");
            }
        }

        // 4. Health check (should fail and cleanup)
        manager.check_evolution_health().await.unwrap();
        {
            let window_lock = manager.observation_window();
            let window = window_lock.read();
            assert!(!window.is_active());
        }
    }
}
