use chrono::Utc;
use parking_lot::RwLock;
use std::sync::Arc;

use async_trait::async_trait;
use benshu_auth::Vault;
use benshu_brain::agent::memory::{Fact, FactProtection, FactStatus, Memory, MemoryManager};
use benshu_brain::agent::multi_agent::Coordinator;
use benshu_builtin_tools::SkillLoader;
use benshu_engram::{HierarchicalRetriever, HybridSearchEngine};
use benshu_experience_core::ExperienceStore;
use benshu_inference::KvEngine;
use benshu_infra::traits::kernel::KernelCapability;
use benshu_infra::traits::nlu::NluEngine;
use benshu_infra::traits::resource::ResourceArbiterProvider;
use benshu_mcp::manager::McpManager;
use benshu_orchestrator::ResourceArbiter;
use benshu_scheduler::Scheduler;
use benshu_security::SecurityManager;
use benshu_sensory::SensoryHub;
use benshu_state::{
    ArtifactLifecycle, ArtifactManager, ArtifactRecord, RunManager, RunRecord, RuntimeEventManager,
    SnapshotManager, TaskArtifactRef, TaskManager, TaskState,
};
use benshu_telemetry::{
    ArtifactRef, ProfilerArtifact, RealHarnessCase, RealHarnessResult, RealHarnessSuiteResult,
    RunTrace, TelemetryManager, TraceStatus, WitnessBundle,
};

fn fact_matches_memory_query(fact: &Fact, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return false;
    }

    let haystack = format!("{} {}", fact.category, fact.content).to_lowercase();
    let lowered_query = query.to_lowercase();
    if haystack.contains(&lowered_query) {
        return true;
    }

    lowered_query
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '-'))
        .filter(|token| token.chars().count() >= 3)
        .any(|token| haystack.contains(token))
}

fn memory_query_tokens(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '-'))
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_string)
        .collect()
}

fn fact_memory_score(fact: &Fact, query: &str) -> usize {
    let query = query.trim().to_lowercase();
    let haystack = format!("{} {}", fact.category, fact.content).to_lowercase();
    let mut score = 0usize;

    if !query.is_empty() && haystack.contains(&query) {
        score += 1000;
    }

    for token in memory_query_tokens(&query) {
        if haystack.contains(&token) {
            score += token.chars().count().max(1) * 10;
        }
    }

    if fact.verified {
        score += 25;
    }
    if matches!(
        fact.status,
        benshu_brain::agent::memory::FactStatus::Verified
    ) {
        score += 25;
    }

    score
}

fn fact_slot_key(content: &str) -> Option<String> {
    let normalized = content
        .trim()
        .trim_matches(|ch: char| ch == '「' || ch == '」' || ch == '"' || ch == '\'')
        .replace("我的", "")
        .replace("用户的", "")
        .replace("用户", "")
        .replace("我", "")
        .trim()
        .to_string();

    let lower = normalized.to_lowercase();
    if normalized.contains('「')
        || normalized.contains('"')
        || normalized.contains('\'')
        || lower.contains("更新")
        || lower.contains("改成")
        || lower.contains("update")
        || lower.contains("change")
    {
        if let Some(slot) = [
            "测试验证码",
            "验证码",
            "手机号",
            "电话",
            "地址",
            "偏好",
            "名字",
            "姓名",
            "邮箱",
            "标记",
            "账号",
            "密码",
            "生日",
            "token",
            "code",
            "phone",
            "email",
            "preference",
            "name",
            "address",
        ]
        .iter()
        .find(|term| lower.contains(**term))
        {
            return Some((*slot).to_string());
        }
    }

    for separator in ["：", ":", " 是 ", "为", "="] {
        if let Some((left, right)) = normalized.split_once(separator) {
            let key = left.trim();
            let value = right.trim();
            if key.chars().count() >= 2 && !value.is_empty() {
                return Some(key.to_lowercase());
            }
        }
    }

    [
        "测试验证码",
        "验证码",
        "手机号",
        "电话",
        "地址",
        "偏好",
        "名字",
        "姓名",
        "邮箱",
        "标记",
        "账号",
        "密码",
        "生日",
        "token",
        "code",
        "phone",
        "email",
        "preference",
        "name",
        "address",
    ]
    .iter()
    .find(|term| lower.contains(**term))
    .map(|term| (*term).to_string())
}

async fn delete_same_slot_facts(
    memory: &Arc<MemoryManager>,
    category: &str,
    slot_key: &str,
    keep_fact_id: &str,
) -> anyhow::Result<usize> {
    let facts = memory.retrieve_facts("default", None).await?;
    let mut deleted = 0usize;
    for fact in facts {
        if fact.id == keep_fact_id
            || fact.category != category
            || !matches!(fact.protection, FactProtection::Normal)
        {
            continue;
        }
        if fact_slot_key(&fact.content).as_deref() == Some(slot_key) {
            memory.delete_fact("default", None, &fact.id).await?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

async fn query_memory_with_fact_prefix(
    memory: &Arc<MemoryManager>,
    search_engine: &Arc<HybridSearchEngine>,
    query: &str,
    limit: usize,
) -> anyhow::Result<String> {
    let effective_limit = limit.max(1);
    let mut combined = Vec::new();

    if let Ok(mut facts) = memory.retrieve_facts("default", None).await {
        facts.retain(|fact| fact_matches_memory_query(fact, query));
        facts.sort_by(|a, b| {
            fact_memory_score(b, query)
                .cmp(&fact_memory_score(a, query))
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });
        for fact in facts.into_iter().take(effective_limit) {
            combined.push(format!(
                "Fact ID: {}\nCategory: {}\nContent: {}",
                fact.id, fact.category, fact.content
            ));
        }
        if !combined.is_empty() {
            return Ok(combined.join("\n\n"));
        }
    }

    let results = search_engine
        .search(query, effective_limit)
        .map_err(|e| anyhow::anyhow!(e))?;
    let store = search_engine.engram_store();
    for r in results {
        if combined.len() >= effective_limit {
            break;
        }
        if let Ok(Some(content)) = store.get_content(&r.document) {
            if !combined.iter().any(|existing| existing == &content) {
                combined.push(content);
            }
        }
    }

    Ok(combined.join("\n"))
}

/// Global Registry of all OS-level services
pub struct KernelRegistry {
    coordinator: Arc<Coordinator>,
    memory: Arc<MemoryManager>,
    search_engine: Arc<HybridSearchEngine>,
    retriever: Arc<HierarchicalRetriever>,
    skill_loader: Arc<SkillLoader>,
    vault: Arc<Vault>,
    kv_engine: Arc<RwLock<KvEngine>>,
    sensory: Arc<SensoryHub>,
    security: Arc<SecurityManager>,
    mcp: Arc<McpManager>,
    scheduler: Arc<Scheduler>,
    bus: Arc<benshu_infra::bus::MessageBus>,
    comm_hub: Arc<benshu_comm::transport::MemoryHub>,
    event_bus: Arc<benshu_infra::observable::EventDispatcher>,

    // New Kernel Services
    state_snapshot: Arc<SnapshotManager>,
    state_task: Arc<TaskManager>,
    state_artifact: Arc<ArtifactManager>,
    state_run: Arc<RunManager>,
    state_runtime_event: Arc<RuntimeEventManager>,
    state_session: Arc<benshu_state::session::SessionManager>,
    telemetry: Arc<TelemetryManager>,
    experience_store: Arc<ExperienceStore>,
    arbiter: Arc<ResourceArbiter>,
    nlu: Arc<dyn NluEngine>,
    fact_checker: Arc<dyn benshu_infra::traits::validation::FactChecker>,
    image_gen: Arc<dyn benshu_inference::backend::ImageGenBackend>,
    tactical_slm: Option<Arc<dyn benshu_inference::backend::ModelBackend>>,

    base_dir: std::path::PathBuf,
}

pub struct RuntimeMainlinePersistence {
    pub task: Option<TaskState>,
    pub witness_bundle: Option<WitnessBundle>,
}

impl KernelRegistry {
    pub fn coordinator(&self) -> &Arc<Coordinator> {
        &self.coordinator
    }
    pub fn memory(&self) -> &Arc<MemoryManager> {
        &self.memory
    }
    pub fn search_engine(&self) -> &Arc<HybridSearchEngine> {
        &self.search_engine
    }
    pub fn retriever(&self) -> &Arc<HierarchicalRetriever> {
        &self.retriever
    }
    pub fn skill_loader(&self) -> &Arc<SkillLoader> {
        &self.skill_loader
    }
    pub fn vault(&self) -> &Arc<Vault> {
        &self.vault
    }
    pub fn kv_engine(&self) -> &Arc<RwLock<KvEngine>> {
        &self.kv_engine
    }
    pub fn sensory(&self) -> &Arc<SensoryHub> {
        &self.sensory
    }
    pub fn security(&self) -> &Arc<SecurityManager> {
        &self.security
    }
    pub fn mcp(&self) -> &Arc<McpManager> {
        &self.mcp
    }
    pub fn scheduler(&self) -> &Arc<Scheduler> {
        &self.scheduler
    }
    pub fn bus(&self) -> &Arc<benshu_infra::bus::MessageBus> {
        &self.bus
    }
    pub fn comm_hub(&self) -> &Arc<benshu_comm::transport::MemoryHub> {
        &self.comm_hub
    }
    pub fn event_bus(&self) -> &Arc<benshu_infra::observable::EventDispatcher> {
        &self.event_bus
    }
    pub fn state_snapshot(&self) -> &Arc<SnapshotManager> {
        &self.state_snapshot
    }
    pub fn state_task(&self) -> &Arc<TaskManager> {
        &self.state_task
    }
    pub fn state_artifact(&self) -> &Arc<ArtifactManager> {
        &self.state_artifact
    }
    pub fn state_run(&self) -> &Arc<RunManager> {
        &self.state_run
    }
    pub fn state_runtime_event(&self) -> &Arc<RuntimeEventManager> {
        &self.state_runtime_event
    }
    pub fn state_session(&self) -> &Arc<benshu_state::session::SessionManager> {
        &self.state_session
    }
    pub fn telemetry(&self) -> &Arc<TelemetryManager> {
        &self.telemetry
    }
    pub fn experience_store(&self) -> &Arc<ExperienceStore> {
        &self.experience_store
    }
    pub fn arbiter(&self) -> &Arc<ResourceArbiter> {
        &self.arbiter
    }
    pub fn nlu(&self) -> &Arc<dyn NluEngine> {
        &self.nlu
    }
    pub fn fact_checker(&self) -> &Arc<dyn benshu_infra::traits::validation::FactChecker> {
        &self.fact_checker
    }
    pub fn image_gen(&self) -> &Arc<dyn benshu_inference::backend::ImageGenBackend> {
        &self.image_gen
    }
    pub fn tactical_slm(&self) -> Option<&Arc<dyn benshu_inference::backend::ModelBackend>> {
        self.tactical_slm.as_ref()
    }
    pub fn base_dir(&self) -> &std::path::PathBuf {
        &self.base_dir
    }

    pub fn build(params: KernelRegistryBuilder) -> Self {
        Self {
            coordinator: params.coordinator,
            memory: params.memory,
            search_engine: params.search_engine,
            retriever: params.retriever,
            skill_loader: params.skill_loader,
            vault: params.vault,
            kv_engine: params.kv_engine,
            sensory: params.sensory,
            security: params.security,
            mcp: params.mcp,
            scheduler: params.scheduler,
            bus: params.bus,
            comm_hub: params.comm_hub,
            event_bus: params.event_bus,
            state_snapshot: params.state_snapshot,
            state_task: params.state_task,
            state_artifact: params.state_artifact,
            state_run: params.state_run,
            state_runtime_event: params.state_runtime_event,
            state_session: params.state_session,
            telemetry: params.telemetry,
            experience_store: params.experience_store,
            arbiter: params.arbiter,
            nlu: params.nlu,
            fact_checker: params.fact_checker,
            image_gen: params.image_gen,
            tactical_slm: params.tactical_slm,
            base_dir: params.base_dir,
        }
    }

    pub async fn persist_runtime_mainline(
        &self,
        mut task: Option<TaskState>,
        extra_tasks: Vec<TaskState>,
        mut run_trace: Option<&mut RunTrace>,
        suite_id: Option<&str>,
    ) -> anyhow::Result<RuntimeMainlinePersistence> {
        let witness_bundle = if let Some(trace) = run_trace.as_deref_mut() {
            Some(self.telemetry.capture_evaluation_tap(trace, suite_id))
        } else {
            None
        };

        if let Some(task_state) = task.as_mut() {
            task_state.witness_id = witness_bundle.as_ref().map(|bundle| bundle.witness_id);
            self.state_task.save(task_state.clone()).await?;
            register_task_artifacts(&self.state_artifact, task_state).await?;
        }

        for extra_task in &extra_tasks {
            self.state_task.save(extra_task.clone()).await?;
            register_task_artifacts(&self.state_artifact, extra_task).await?;
        }

        if let Some(trace) = run_trace.as_deref() {
            let trace_artifact_refs = register_trace_artifacts(&self.state_artifact, trace).await?;
            if let Some(task_id) = trace.task_id {
                if let Some(updated_task) = self
                    .state_task
                    .upsert_artifact_refs(task_id, trace_artifact_refs)
                    .await?
                {
                    if task.as_ref().map(|item| item.id) == Some(updated_task.id) {
                        task = Some(updated_task);
                    }
                }
            }
            let profiler = self.telemetry.get_run_profiler_artifact(&trace.run_id);
            self.state_run
                .save(run_record_from_mainline(
                    trace,
                    task.as_ref(),
                    witness_bundle.as_ref(),
                    profiler.as_ref(),
                ))
                .await?;
        }

        Ok(RuntimeMainlinePersistence {
            task,
            witness_bundle,
        })
    }

    pub async fn run_real_harness_case<F, Fut>(
        &self,
        case: RealHarnessCase,
        run: F,
    ) -> anyhow::Result<RealHarnessResult>
    where
        F: FnOnce(&RealHarnessCase) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<RunTrace>>,
    {
        let mut run_trace = run(&case).await?;
        let witness = self
            .telemetry
            .capture_evaluation_tap(&mut run_trace, Some(&case.suite_id));
        let profiler = self.telemetry.get_run_profiler_artifact(&run_trace.run_id);
        self.state_run
            .save(run_record_from_mainline(
                &run_trace,
                None,
                Some(&witness),
                profiler.as_ref(),
            ))
            .await?;
        let scorecard = self
            .telemetry
            .get_scorecard(&case.suite_id)
            .ok_or_else(|| anyhow::anyhow!("missing scorecard after harness capture"))?;
        Ok(RealHarnessResult {
            case,
            witness,
            scorecard,
        })
    }

    pub async fn run_real_harness_suite<F, Fut>(
        &self,
        cases: Vec<RealHarnessCase>,
        mut run: F,
    ) -> anyhow::Result<RealHarnessSuiteResult>
    where
        F: FnMut(RealHarnessCase) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<RunTrace>>,
    {
        let Some(first_case) = cases.first() else {
            anyhow::bail!("real harness suite requires at least one case");
        };
        let suite_id = first_case.suite_id.clone();
        let total_cases = cases.len();
        let mut results = Vec::with_capacity(total_cases);

        for case in cases {
            if case.suite_id != suite_id {
                anyhow::bail!(
                    "real harness suite expected suite_id '{}' but got '{}'",
                    suite_id,
                    case.suite_id
                );
            }

            let result = self
                .run_real_harness_case(case.clone(), |_| run(case.clone()))
                .await?;
            results.push(result);
        }

        let scorecard = self
            .telemetry
            .get_scorecard(&suite_id)
            .ok_or_else(|| anyhow::anyhow!("missing scorecard after harness suite"))?;
        Ok(RealHarnessSuiteResult {
            suite_id,
            total_cases,
            results,
            scorecard,
        })
    }
}

async fn register_trace_artifacts(
    manager: &ArtifactManager,
    trace: &RunTrace,
) -> anyhow::Result<Vec<TaskArtifactRef>> {
    let mut refs = Vec::new();
    for artifact in &trace.artifacts {
        manager
            .save(artifact_record_from_run_trace(trace, artifact))
            .await?;
        refs.push(task_artifact_ref_from_trace_artifact(artifact));
    }
    Ok(refs)
}

async fn register_task_artifacts(
    manager: &ArtifactManager,
    task: &TaskState,
) -> anyhow::Result<()> {
    for artifact in &task.artifacts {
        manager
            .save(artifact_record_from_task_state(task, artifact))
            .await?;
    }
    Ok(())
}

fn artifact_record_from_run_trace(trace: &RunTrace, artifact: &ArtifactRef) -> ArtifactRecord {
    let scope = ArtifactManager::classify_scope(&artifact.uri, None);
    let now = Utc::now();
    ArtifactRecord {
        artifact_id: artifact.artifact_id.clone(),
        kind: artifact.kind.clone(),
        uri: artifact.uri.clone(),
        scope,
        lifecycle: ArtifactLifecycle::Session,
        created_at: now,
        updated_at: now,
        agent_id: trace.agent_id.clone(),
        task_id: trace.task_id,
        run_id: Some(trace.run_id),
        trace_id: Some(trace.run_id),
        session_id: Some(trace.session_id.to_string()),
        thread_id: trace.thread_id.clone(),
        tool_name: None,
        media_type: artifact.media_type.clone(),
        virtual_path: None,
        source_kind: "run_trace".to_string(),
        metadata: trace.metadata.clone(),
    }
}

fn artifact_record_from_task_state(task: &TaskState, artifact: &TaskArtifactRef) -> ArtifactRecord {
    let scope = ArtifactManager::classify_scope(&artifact.uri, None);
    let now = Utc::now();
    ArtifactRecord {
        artifact_id: artifact.artifact_id.clone(),
        kind: artifact.kind.clone(),
        uri: artifact.uri.clone(),
        scope,
        lifecycle: ArtifactLifecycle::Session,
        created_at: now,
        updated_at: now,
        agent_id: task.agent_id.clone(),
        task_id: Some(task.id),
        run_id: task.run_id,
        trace_id: task.trace_id,
        session_id: task.session_id.clone(),
        thread_id: task.thread_id.clone(),
        tool_name: None,
        media_type: artifact.media_type.clone(),
        virtual_path: None,
        source_kind: "task_state".to_string(),
        metadata: std::collections::HashMap::new(),
    }
}

fn task_artifact_ref_from_trace_artifact(artifact: &ArtifactRef) -> TaskArtifactRef {
    TaskArtifactRef {
        artifact_id: artifact.artifact_id.clone(),
        kind: artifact.kind.clone(),
        uri: artifact.uri.clone(),
        media_type: artifact.media_type.clone(),
    }
}

fn run_record_from_mainline(
    run_trace: &RunTrace,
    task: Option<&TaskState>,
    witness_bundle: Option<&WitnessBundle>,
    profiler: Option<&ProfilerArtifact>,
) -> RunRecord {
    let benchmark_fingerprint = witness_bundle
        .as_ref()
        .map(|bundle| bundle.benchmark_fingerprint.fingerprint.clone())
        .or_else(|| {
            run_trace
                .witness
                .as_ref()
                .and_then(|witness| witness.benchmark_fingerprint.clone())
        });

    RunRecord {
        run_id: run_trace.run_id,
        trace_id: run_trace.run_id,
        session_id: run_trace.session_id,
        agent_id: run_trace.agent_id.clone(),
        trace_status: trace_status_label(&run_trace.status).to_string(),
        started_at: run_trace.started_at,
        finished_at: run_trace.finished_at,
        task_id: task.map(|task| task.id).or(run_trace.task_id),
        thread_id: run_trace.thread_id.clone(),
        provider: run_trace.provider.clone(),
        model: run_trace.model.clone(),
        witness_id: witness_bundle.map(|bundle| bundle.witness_id),
        trial_id: witness_bundle.map(|bundle| bundle.trial.trial_id),
        suite_id: witness_bundle.map(|bundle| bundle.task.suite_id.clone()),
        benchmark_fingerprint,
        profiler_id: profiler.map(|artifact| artifact.profiler_id.clone()),
        artifact_ids: run_trace
            .artifacts
            .iter()
            .map(|artifact| artifact.artifact_id.clone())
            .collect(),
        metadata: run_trace.metadata.clone(),
    }
}

fn trace_status_label(status: &TraceStatus) -> &'static str {
    match status {
        TraceStatus::Started => "started",
        TraceStatus::Succeeded => "succeeded",
        TraceStatus::Failed => "failed",
        TraceStatus::Cancelled => "cancelled",
        TraceStatus::Degraded => "degraded",
        TraceStatus::TimedOut => "timed_out",
    }
}

#[async_trait]
impl benshu_infra::traits::kernel::KernelCapability for KernelRegistry {
    async fn request_resource(
        &self,
        request: benshu_infra::traits::resource::AllocationRequest,
    ) -> benshu_infra::traits::resource::AllocationResponse {
        self.arbiter.request_allocation(request).await
    }

    async fn report_usage(&self, agent_id: &str, vram_mb: usize) {
        // Phase 10: Feedback Loop to the Arbiter for dynamic throttling
        self.arbiter.update_allocation(agent_id, vram_mb).await;
    }

    async fn get_secret(&self, key: &str) -> anyhow::Result<Option<String>> {
        // Vault is audited at the service level
        self.vault.get(key).map_err(|e| anyhow::anyhow!(e))
    }

    async fn query_memory(&self, query: &str, limit: usize) -> anyhow::Result<String> {
        query_memory_with_fact_prefix(&self.memory, &self.search_engine, query, limit).await
    }

    async fn record_fact(&self, fact: &str, category: &str) -> anyhow::Result<()> {
        let mut memory_fact = Fact::new(fact.trim(), category.trim());
        memory_fact.source = Some("kernel_tool.remember_this".to_string());
        memory_fact.verified = true;
        memory_fact.status = FactStatus::Verified;
        let slot_key = fact_slot_key(&memory_fact.content);
        if let Some(slot_key) = slot_key.as_deref() {
            delete_same_slot_facts(
                &self.memory,
                &memory_fact.category,
                slot_key,
                &memory_fact.id,
            )
            .await?;
        }

        self.memory
            .store_fact("default", None, memory_fact)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    fn check_permission(&self, _action: &str) -> bool {
        // TODO: Implement granular permission check
        true
    }

    async fn spawn_sub_agent(
        &self,
        _role_name: &str,
        _restricted: bool,
        _tool_whitelist: Option<Vec<String>>,
        _vram_quota_mb: usize,
    ) -> anyhow::Result<()> {
        // Integration with Coordinator and Factory for restricted spawn
        anyhow::bail!("Fission protocol - Permission/Resource Restriction is being prioritized in the next build.")
    }
}

/// A lightweight proxy that implements KernelCapability without holding a strong
/// reference to the entire KernelRegistry. This prevents circular dependencies.
pub struct KernelProxy {
    arbiter: Arc<benshu_orchestrator::ResourceArbiter>,
    memory: Arc<benshu_brain::agent::memory::MemoryManager>,
    search_engine: Arc<benshu_engram::HybridSearchEngine>,
    vault: Arc<benshu_auth::Vault>,
    security: Arc<benshu_security::SecurityManager>,
}

impl KernelProxy {
    pub fn new(registry: &KernelRegistry) -> Self {
        Self {
            arbiter: registry.arbiter().clone(),
            memory: registry.memory().clone(),
            search_engine: registry.search_engine().clone(),
            vault: registry.vault().clone(),
            security: registry.security().clone(),
        }
    }
}

#[async_trait]
impl benshu_infra::traits::kernel::KernelCapability for KernelProxy {
    async fn request_resource(
        &self,
        request: benshu_infra::traits::resource::AllocationRequest,
    ) -> benshu_infra::traits::resource::AllocationResponse {
        self.arbiter.request_allocation(request).await
    }

    async fn report_usage(&self, agent_id: &str, vram_mb: usize) {
        self.arbiter.update_allocation(agent_id, vram_mb).await;
    }

    async fn get_secret(&self, key: &str) -> anyhow::Result<Option<String>> {
        self.vault.get(key).map_err(|e| anyhow::anyhow!(e))
    }

    async fn query_memory(&self, query: &str, limit: usize) -> anyhow::Result<String> {
        query_memory_with_fact_prefix(&self.memory, &self.search_engine, query, limit).await
    }

    async fn record_fact(&self, fact: &str, category: &str) -> anyhow::Result<()> {
        let mut memory_fact = Fact::new(fact.trim(), category.trim());
        memory_fact.source = Some("kernel_proxy.remember_this".to_string());
        memory_fact.verified = true;
        memory_fact.status = FactStatus::Verified;

        self.memory
            .store_fact("default", None, memory_fact)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    fn check_permission(&self, _action: &str) -> bool {
        // TODO: Implement granular permission check in SecurityManager
        true
    }

    async fn spawn_sub_agent(
        &self,
        _: &str,
        _: bool,
        _: Option<Vec<String>>,
        _: usize,
    ) -> anyhow::Result<()> {
        anyhow::bail!("Restricted spawn not available via proxy handles yet")
    }
}

pub struct KernelRegistryBuilder {
    pub coordinator: Arc<Coordinator>,
    pub memory: Arc<MemoryManager>,
    pub search_engine: Arc<HybridSearchEngine>,
    pub retriever: Arc<HierarchicalRetriever>,
    pub skill_loader: Arc<SkillLoader>,
    pub vault: Arc<Vault>,
    pub kv_engine: Arc<RwLock<KvEngine>>,
    pub sensory: Arc<SensoryHub>,
    pub security: Arc<SecurityManager>,
    pub mcp: Arc<McpManager>,
    pub scheduler: Arc<Scheduler>,
    pub bus: Arc<benshu_infra::bus::MessageBus>,
    pub comm_hub: Arc<benshu_comm::transport::MemoryHub>,
    pub event_bus: Arc<benshu_infra::observable::EventDispatcher>,
    pub state_snapshot: Arc<SnapshotManager>,
    pub state_task: Arc<TaskManager>,
    pub state_artifact: Arc<ArtifactManager>,
    pub state_run: Arc<RunManager>,
    pub state_runtime_event: Arc<RuntimeEventManager>,
    pub state_session: Arc<benshu_state::session::SessionManager>,
    pub telemetry: Arc<TelemetryManager>,
    pub experience_store: Arc<ExperienceStore>,
    pub arbiter: Arc<ResourceArbiter>,
    pub nlu: Arc<dyn NluEngine>,
    pub fact_checker: Arc<dyn benshu_infra::traits::validation::FactChecker>,
    pub image_gen: Arc<dyn benshu_inference::backend::ImageGenBackend>,
    pub tactical_slm: Option<Arc<dyn benshu_inference::backend::ModelBackend>>,
    pub base_dir: std::path::PathBuf,
}
