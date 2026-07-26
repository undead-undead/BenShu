//! Multi-agent coordination system
//!
//! Enables multiple specialized agents to work together.

use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::agent::agent_identity::AgentIdentity;
use crate::agent::memory::Memory;
use crate::agent::message::{Message, Role};
use crate::agent::protocol::{DelegationMode, DelegationRecord, TaskOwnership};
use crate::error::{Error, Result};
use crate::skills::tool::{
    capability_route_prefers_direct_tool_surface_for_query, classify_query_capability_route,
    select_coordinator_task_mode, CoordinatorTaskMode,
};
pub use benshu_infra::agent::{AgentMessage, AgentRole, MessageType};
#[cfg(feature = "cron")]
pub use benshu_scheduler::{
    CronJob, JobHandler, JobPayload, JobSchedule, Scheduler, SchedulerError,
};

/// Trait for agents that can participate in multi-agent systems
#[async_trait]
pub trait MultiAgent: Send + Sync {
    /// Get this agent's role
    fn role(&self) -> AgentRole;

    /// Signal that this runtime instance is being replaced or removed.
    ///
    /// Coordinators call this before unregistering an agent so long-lived
    /// background loops can observe shutdown before the final Arc is dropped.
    fn signal_shutdown(&self) {}

    /// Handle an incoming message from another agent
    async fn handle_message(&self, message: AgentMessage) -> Result<Option<AgentMessage>>;

    /// Process a user request
    async fn process(&self, input: &str) -> Result<String>;

    /// Run an isolated complexity-analysis request that must not reuse the
    /// normal frontstage tool-routing surface.
    async fn analyze_complexity(&self, prompt: &str) -> Result<String> {
        self.process(prompt).await
    }

    /// Generate an isolated text artifact without exposing the worker's tool
    /// surface. Long-running step executors use this to prevent a worker from
    /// recursively delegating when the caller only needs the step output.
    async fn generate_text_only(&self, prompt: &str) -> Result<String> {
        self.process(prompt).await
    }

    /// Generate an isolated text artifact with an executor-provided output
    /// ceiling. Implementations that cannot enforce the ceiling should still
    /// execute the isolated text path.
    async fn generate_text_only_with_max_tokens(
        &self,
        prompt: &str,
        max_tokens: Option<u64>,
    ) -> Result<String> {
        let _ = max_tokens;
        self.generate_text_only(prompt).await
    }

    /// Generate isolated text while reporting visible progress to the runtime.
    /// Implementations without streaming support may fall back to the bounded
    /// text path and emit a final progress update.
    async fn generate_text_only_with_progress(
        &self,
        prompt: &str,
        max_tokens: Option<u64>,
        progress: Option<TextGenerationProgressSink>,
    ) -> Result<String> {
        if let Some(progress) = progress.as_ref() {
            progress(TextGenerationProgress {
                stage: TextGenerationProgressStage::Started,
                generated_chars: 0,
                preview: None,
                snapshot: None,
            });
        }
        let text = self
            .generate_text_only_with_max_tokens(prompt, max_tokens)
            .await?;
        if let Some(progress) = progress.as_ref() {
            progress(TextGenerationProgress {
                stage: TextGenerationProgressStage::Completed,
                generated_chars: text.chars().count(),
                preview: Some(text.chars().take(240).collect()),
                snapshot: Some(text.clone()),
            });
        }
        Ok(text)
    }

    /// Generate isolated text with both provider-token and runtime character
    /// constraints. The character limits are enforced by the runtime while the
    /// provider request is streaming, so long artifact steps can stop close to
    /// their contract instead of waiting for the provider token ceiling.
    async fn generate_text_only_with_limits(
        &self,
        prompt: &str,
        limits: TextGenerationLimits,
        progress: Option<TextGenerationProgressSink>,
    ) -> Result<String> {
        self.generate_text_only_with_progress(prompt, limits.max_tokens, progress)
            .await
    }

    /// Process a chat conversation
    async fn chat(
        &self,
        messages: Vec<Message>,
        session_id: Option<String>,
    ) -> Result<crate::agent::ChatOutcome>;

    /// Run one explicit sleep-consolidation cycle when the frontstage user
    /// asks for memory maintenance. Most agents do not own a consolidator.
    async fn run_memory_consolidation_once(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Get the agent's agent (if supported)
    fn agent_identity(&self) -> Option<Arc<parking_lot::RwLock<Option<AgentIdentity>>>>;

    /// Get a receiver for agent events
    fn events(&self) -> tokio::sync::broadcast::Receiver<crate::agent::AgentEvent>;

    /// Get the agent's security handler (if supported)
    fn security(&self) -> Option<Arc<dyn crate::security::SecurityHandler>>;

    /// Cancel the current ongoing task
    fn cancel(&self);

    /// Cancel the current foreground task for a specific session when supported.
    fn cancel_foreground_task(&self, _session_id: Option<&str>) {
        self.cancel();
    }

    /// Soft-pause the current foreground task for a specific session when supported.
    async fn pause_foreground_task(&self, _session_id: Option<&str>, _note: Option<&str>) -> bool {
        false
    }

    /// Resume a soft-paused foreground task for a specific session when supported.
    async fn resume_foreground_task(
        &self,
        _session_id: Option<&str>,
        _instruction: Option<&str>,
    ) -> bool {
        false
    }

    /// Whether a foreground task for this session is currently soft-paused.
    async fn is_foreground_task_paused(&self, _session_id: Option<&str>) -> bool {
        false
    }

    /// Ensure the agent is ready for a new task (reset tokens if needed)
    fn ensure_active_token(&self);

    /// Whether this agent currently has a live foreground reasoning task.
    fn has_active_foreground_task(&self) -> bool {
        false
    }

    /// Whether this agent currently has a live foreground reasoning task for a specific session.
    fn has_active_foreground_task_for_session(&self, _session_id: Option<&str>) -> bool {
        self.has_active_foreground_task()
    }

    /// Set the list of all available roles in the swarm
    fn set_all_roles(&self, _roles: Vec<AgentRole>) {}

    /// Get the communication client
    fn comm_client(&self) -> Option<benshu_comm::client::CommClient> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextGenerationProgressStage {
    Started,
    Streaming,
    Completed,
}

#[derive(Debug, Clone)]
pub struct TextGenerationProgress {
    pub stage: TextGenerationProgressStage,
    pub generated_chars: usize,
    pub preview: Option<String>,
    pub snapshot: Option<String>,
}

pub type TextGenerationProgressSink = Arc<dyn Fn(TextGenerationProgress) + Send + Sync>;

#[derive(Debug, Clone, Copy, Default)]
pub struct TextGenerationLimits {
    pub max_tokens: Option<u64>,
    pub target_chars: Option<usize>,
    pub hard_max_chars: Option<usize>,
}

/// Shared lazy worker spawner used by the coordinator to materialize blueprinted workers.
#[async_trait]
pub trait WorkerSpawner: Send + Sync {
    async fn ensure_worker(&self, role: &AgentRole) -> Result<bool>;
}

/// Coordinator for multi-agent systems
pub struct Coordinator {
    /// Registered agents
    agents: DashMap<AgentRole, Arc<dyn MultiAgent>>,
    /// Worker profiles that can be spawned lazily on demand.
    worker_catalog: DashMap<String, WorkerBlueprint>,
    /// Shared lazy worker spawner installed by the host runtime.
    worker_spawner: parking_lot::RwLock<Option<Arc<dyn WorkerSpawner>>>,
    /// Last activity timestamp for live worker instances.
    worker_last_used: DashMap<String, Instant>,
    /// Active agent per session (for persistence)
    active_agents: DashMap<String, AgentRole>,
    /// Max rounds of coordination
    max_rounds: usize,
    /// Scheduler for proactive tasks
    #[cfg(feature = "cron")]
    pub scheduler: tokio::sync::OnceCell<Arc<benshu_scheduler::Scheduler>>,
    /// Shared memory for the system
    pub memory: tokio::sync::OnceCell<Arc<dyn Memory>>,
    /// Shared metrics registry
    pub metrics: Arc<crate::infra::observable::MetricsRegistry>,
    /// System-wide approval handler
    pub approval_handler: tokio::sync::OnceCell<Arc<dyn crate::agent::ApprovalHandler>>,
    /// Phase 8: Capability Sensor for autonomous resource governance
    pub sensor: Arc<parking_lot::RwLock<Box<dyn benshu_infra::resource::ResourceSensor>>>,
    /// Shared application configuration
    pub config: tokio::sync::OnceCell<Arc<parking_lot::RwLock<crate::config::AppConfig>>>,
    /// Durable session tracking (Phase 11.4 stateless fix)
    pub session_manager: tokio::sync::OnceCell<Arc<benshu_state::session::SessionManager>>,
}

#[derive(Debug, Clone)]
pub struct WorkerBlueprint {
    pub role: AgentRole,
    pub agent_path: PathBuf,
    pub display_name: String,
    pub description: Option<String>,
    pub tools: Vec<String>,
    pub artifact_policy: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerCapabilityMatchSource {
    ExactRole,
    DisplayName,
    Tool,
    ArtifactPolicy,
    Description,
    TextOverlap,
}

impl WorkerCapabilityMatchSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExactRole => "exact_role",
            Self::DisplayName => "display_name",
            Self::Tool => "equipped_tool",
            Self::ArtifactPolicy => "artifact_policy",
            Self::Description => "description",
            Self::TextOverlap => "text_overlap",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCapabilityCandidate {
    pub role: AgentRole,
    pub display_name: String,
    pub tools: Vec<String>,
    pub capabilities: Vec<String>,
    pub score: i32,
    pub source: WorkerCapabilityMatchSource,
    pub reasons: Vec<String>,
}

impl Coordinator {
    fn primary_role_fallback() -> AgentRole {
        AgentRole::Custom("benshu".to_string())
    }

    fn parse_role_name(&self, role_name: &str) -> AgentRole {
        match role_name.to_lowercase().as_str() {
            "benshu" => self.primary_role(),
            "researcher" => AgentRole::Researcher,
            "trader" => AgentRole::Trader,
            "risk_analyst" => AgentRole::RiskAnalyst,
            "strategist" => AgentRole::Strategist,
            _ => AgentRole::Custom(role_name.to_string()),
        }
    }

    fn role_key(role: &AgentRole) -> String {
        role.name().to_lowercase()
    }

    fn broadcast_available_roles(&self) {
        let current_roles = self.roles();
        for entry in self.agents.iter() {
            entry.value().set_all_roles(current_roles.clone());
        }
    }

    /// Create a new coordinator
    pub fn new() -> Self {
        let sensor = crate::infra::CapabilitySensor::new();
        Self {
            agents: DashMap::new(),
            worker_catalog: DashMap::new(),
            worker_spawner: parking_lot::RwLock::new(None),
            worker_last_used: DashMap::new(),
            active_agents: DashMap::new(),
            max_rounds: 10,
            #[cfg(feature = "cron")]
            scheduler: tokio::sync::OnceCell::new(),
            memory: tokio::sync::OnceCell::new(),
            metrics: Arc::new(crate::infra::observable::MetricsRegistry::new()),
            approval_handler: tokio::sync::OnceCell::new(),
            sensor: Arc::new(parking_lot::RwLock::new(Box::new(sensor))),
            config: tokio::sync::OnceCell::new(),
            session_manager: tokio::sync::OnceCell::new(),
        }
    }

    /// Get all registered agent roles
    pub fn get_active_roles(&self) -> Vec<AgentRole> {
        self.agents.iter().map(|e| e.key().clone()).collect()
    }

    /// Register a worker profile without spawning a live agent instance.
    pub fn register_worker_blueprint(&self, blueprint: WorkerBlueprint) {
        let key = Self::role_key(&blueprint.role);
        self.worker_catalog.insert(key, blueprint);
        self.broadcast_available_roles();
    }

    /// Remove a worker profile and any live instance for the same role.
    pub fn unregister_worker_blueprint(&self, role: &AgentRole) {
        let key = Self::role_key(role);
        self.worker_catalog.remove(&key);
        self.worker_last_used.remove(&key);
        self.unregister_agent(role);
        self.broadcast_available_roles();
    }

    /// Install the runtime worker spawner used for blueprint materialization.
    pub fn set_worker_spawner(&self, spawner: Arc<dyn WorkerSpawner>) {
        *self.worker_spawner.write() = Some(spawner);
    }

    async fn ensure_worker_ready(&self, role: &AgentRole) -> bool {
        if self.get(role).is_some() {
            self.touch_worker(role);
            return true;
        }
        if !self.has_worker_blueprint(role) {
            return false;
        }
        let Some(spawner) = self.worker_spawner.read().clone() else {
            return false;
        };

        match spawner.ensure_worker(role).await {
            Ok(true) => {
                self.touch_worker(role);
                self.get(role).is_some()
            }
            Ok(false) => false,
            Err(err) => {
                error!("Failed to lazily spawn worker {}: {}", role.name(), err);
                false
            }
        }
    }

    /// Get an agent by role, lazily spawning a worker if only a blueprint exists.
    pub async fn get_or_spawn(&self, role: &AgentRole) -> Option<Arc<dyn MultiAgent>> {
        if let Some(agent) = self.get(role) {
            return Some(agent);
        }
        if !self.ensure_worker_ready(role).await {
            return None;
        }
        self.get(role)
    }

    /// Return the worker-only roles available for delegation.
    pub fn worker_roles(&self) -> Vec<AgentRole> {
        self.worker_catalog
            .iter()
            .map(|entry| entry.value().role.clone())
            .collect()
    }

    /// Fetch all worker blueprints currently registered with the coordinator.
    pub fn worker_blueprints(&self) -> Vec<WorkerBlueprint> {
        self.worker_catalog
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Fetch a worker blueprint by role.
    pub fn worker_blueprint(&self, role: &AgentRole) -> Option<WorkerBlueprint> {
        self.worker_catalog
            .get(&Self::role_key(role))
            .map(|entry| entry.value().clone())
    }

    pub fn has_worker_blueprint(&self, role: &AgentRole) -> bool {
        self.worker_catalog.contains_key(&Self::role_key(role))
    }

    /// Rank worker blueprints against a requested role hint and the actual task.
    ///
    /// This is a derived runtime view over `worker_catalog`; it deliberately does
    /// not become a second source of truth for worker tools or policies.
    pub fn worker_capability_candidates(
        &self,
        requested_role: Option<&str>,
        task: &str,
    ) -> Vec<WorkerCapabilityCandidate> {
        let requested = requested_role.unwrap_or_default().trim();
        let requested_normalized = normalize_capability_text(requested);
        let task_normalized = normalize_capability_text(task);
        let auto_request = requested.is_empty()
            || matches!(
                requested_normalized.as_str(),
                "auto" | "worker" | "specialist" | "best_worker" | "best_specialist"
            );

        let mut candidates = self
            .worker_blueprints()
            .into_iter()
            .filter_map(|blueprint| {
                let role_name = blueprint.role.name().to_string();
                let normalized_role = normalize_capability_text(&role_name);
                let normalized_display = normalize_capability_text(&blueprint.display_name);
                let normalized_description =
                    normalize_capability_text(blueprint.description.as_deref().unwrap_or_default());
                let normalized_tools = blueprint
                    .tools
                    .iter()
                    .map(|tool| normalize_capability_text(tool))
                    .collect::<Vec<_>>();

                let mut score = 0i32;
                let mut source = WorkerCapabilityMatchSource::TextOverlap;
                let mut reasons = Vec::new();

                if !auto_request && requested_normalized == normalized_role {
                    score += 100_000;
                    source = WorkerCapabilityMatchSource::ExactRole;
                    reasons.push("requested role matched worker role".to_string());
                }
                if !auto_request && requested_normalized == normalized_display {
                    score += 8_000;
                    if score < 10_000 {
                        source = WorkerCapabilityMatchSource::DisplayName;
                    }
                    reasons.push("requested role matched worker display name".to_string());
                }
                if !auto_request
                    && normalized_tools
                        .iter()
                        .any(|tool| tool == &requested_normalized)
                {
                    score += 7_000;
                    if score < 10_000 {
                        source = WorkerCapabilityMatchSource::Tool;
                    }
                    reasons.push("requested role matched equipped tool".to_string());
                }

                let policy_score = worker_artifact_policy_match_score(
                    blueprint.artifact_policy.as_ref(),
                    if auto_request {
                        ""
                    } else {
                        &requested_normalized
                    },
                    &task_normalized,
                );
                if policy_score > 0 {
                    score += policy_score;
                    if policy_score >= 1_200 && score < 10_000 {
                        source = WorkerCapabilityMatchSource::ArtifactPolicy;
                    }
                    reasons.push("artifact policy matched task or role hint".to_string());
                }

                if !task_normalized.is_empty() {
                    for tool in &normalized_tools {
                        if !tool.is_empty()
                            && (task_normalized.contains(tool) || tool.contains(&task_normalized))
                        {
                            score += 900;
                            if score < 10_000 {
                                source = WorkerCapabilityMatchSource::Tool;
                            }
                            reasons.push(format!("task mentioned equipped tool `{}`", tool));
                        }
                    }
                    if !normalized_description.is_empty()
                        && task_text_overlaps(&task_normalized, &normalized_description)
                    {
                        score += 500;
                        if score < 10_000 {
                            source = WorkerCapabilityMatchSource::Description;
                        }
                        reasons.push("task overlapped worker description".to_string());
                    }
                    if task_text_overlaps(&task_normalized, &normalized_role)
                        || task_text_overlaps(&task_normalized, &normalized_display)
                    {
                        score += 350;
                        reasons.push("task overlapped worker role or display name".to_string());
                    }
                }

                if score <= 0 {
                    return None;
                }

                Some(WorkerCapabilityCandidate {
                    role: blueprint.role,
                    display_name: blueprint.display_name,
                    tools: blueprint.tools,
                    capabilities: worker_artifact_policy_capabilities(
                        blueprint.artifact_policy.as_ref(),
                        6,
                    ),
                    score,
                    source,
                    reasons,
                })
            })
            .collect::<Vec<_>>();

        candidates.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.role.name().cmp(right.role.name()))
        });
        candidates
    }

    pub fn best_worker_capability_match(
        &self,
        requested_role: Option<&str>,
        task: &str,
    ) -> Option<WorkerCapabilityCandidate> {
        self.worker_capability_candidates(requested_role, task)
            .into_iter()
            .next()
    }

    pub fn touch_worker(&self, role: &AgentRole) {
        if self.has_worker_blueprint(role) {
            self.worker_last_used
                .insert(Self::role_key(role), Instant::now());
        }
    }

    /// Drop idle worker instances while keeping the prime agent resident.
    pub fn reap_idle_workers(&self, max_idle: Duration) -> Vec<AgentRole> {
        let now = Instant::now();
        let active_roles: Vec<String> = self
            .active_agents
            .iter()
            .map(|entry| Self::role_key(entry.value()))
            .collect();
        let candidates: Vec<String> = self
            .worker_last_used
            .iter()
            .filter_map(|entry| {
                let idle_for = now.saturating_duration_since(*entry.value());
                if idle_for >= max_idle {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();

        let mut reaped = Vec::new();
        for key in candidates {
            if active_roles.iter().any(|active| active == &key) {
                continue;
            }
            let Some(role) = self
                .worker_catalog
                .get(&key)
                .map(|entry| entry.value().role.clone())
            else {
                continue;
            };
            if let Some(agent) = self.get(&role) {
                if agent.has_active_foreground_task() {
                    continue;
                }
                self.agents.remove(&role);
                self.worker_last_used.remove(&key);
                reaped.push(role);
            }
        }

        if !reaped.is_empty() {
            self.broadcast_available_roles();
        }

        reaped
    }

    /// Get current metabolic pressure
    pub fn get_metabolic_pressure(&self) -> benshu_infra::MetabolicStats {
        let resources = self.sensor.write().check_resources(false);
        benshu_infra::MetabolicStats {
            cpu_usage: resources.cpu_usage,
            vram_pressure: resources.vram_pressure_pct(),
            mem_pressure: 100.0 - resources.free_memory_pct,
            is_throttled: resources.cpu_usage > 90.0 || resources.free_memory_pct < 5.0,
            token_usage: None,
            ..Default::default()
        }
    }

    /// Return the canonical prime agent role for the system.
    pub fn primary_role(&self) -> AgentRole {
        Self::primary_role_fallback()
    }

    /// Set throttle limit for a tenant
    pub fn set_tenant_throttle(&self, tenant_id: &str, limit: u32) {
        for entry in self.agents.iter() {
            if let Some(comm) = entry.value().comm_client() {
                let tid = tenant_id.to_string();
                tokio::spawn(async move {
                    comm.set_tenant_limit(&tid, limit).await;
                });
            }
        }
    }

    /// Set throttle limit for an agent role
    pub fn set_agent_throttle(&self, role: &AgentRole, limit: u32) {
        if let Some(agent) = self.get(role) {
            if let Some(comm) = agent.comm_client() {
                let aid = role.name().to_string();
                tokio::spawn(async move {
                    comm.set_agent_limit(&aid, limit).await;
                });
            }
        }
    }

    /// Set max coordination rounds
    pub fn with_max_rounds(mut self, rounds: usize) -> Self {
        self.max_rounds = rounds;
        self
    }

    /// Register an agent
    pub fn register(&self, agent: Arc<dyn MultiAgent>) {
        let role = agent.role();
        let role_name = Self::role_key(&role);
        let duplicate_roles: Vec<AgentRole> = self
            .agents
            .iter()
            .filter_map(|entry| {
                let existing = entry.key();
                if Self::role_key(existing) == role_name && existing != &role {
                    Some(existing.clone())
                } else {
                    None
                }
            })
            .collect();
        for duplicate_role in duplicate_roles {
            if let Some((_, replaced_agent)) = self.agents.remove(&duplicate_role) {
                replaced_agent.signal_shutdown();
                Self::drop_agent_off_runtime_stack(replaced_agent);
            }
        }
        if let Some(replaced_agent) = self.agents.insert(role.clone(), agent) {
            replaced_agent.signal_shutdown();
            Self::drop_agent_off_runtime_stack(replaced_agent);
        }
        self.touch_worker(&role);
        self.broadcast_available_roles();
    }

    /// Remove a live agent by role without touching its worker blueprint.
    pub fn unregister_agent(&self, role: &AgentRole) {
        let role_name = Self::role_key(role);
        let matching_roles: Vec<AgentRole> = self
            .agents
            .iter()
            .filter_map(|entry| {
                (Self::role_key(entry.key()) == role_name).then(|| entry.key().clone())
            })
            .collect();
        for matching_role in matching_roles {
            if let Some((_, removed_agent)) = self.agents.remove(&matching_role) {
                removed_agent.signal_shutdown();
                Self::drop_agent_off_runtime_stack(removed_agent);
            }
        }
        self.broadcast_available_roles();
    }

    fn drop_agent_off_runtime_stack(agent: Arc<dyn MultiAgent>) {
        const AGENT_DROP_STACK_SIZE: usize = 32 * 1024 * 1024;
        match std::thread::Builder::new()
            .name("benshu-agent-drop".to_string())
            .stack_size(AGENT_DROP_STACK_SIZE)
            .spawn(move || drop(agent))
        {
            Ok(handle) => {
                if let Err(error) = handle.join() {
                    warn!(
                        ?error,
                        "agent cleanup thread panicked while dropping a replaced runtime agent"
                    );
                }
            }
            Err(error) => {
                warn!(
                    %error,
                    "failed to spawn large-stack agent cleanup thread; dropping agent on current stack"
                );
            }
        }
    }

    /// Get an agent by role
    pub fn get(&self, role: &AgentRole) -> Option<Arc<dyn MultiAgent>> {
        if let Some(agent) = self.agents.get(role) {
            return Some(Arc::clone(&agent));
        }

        let target_key = Self::role_key(role);
        self.agents.iter().find_map(|entry| {
            (Self::role_key(entry.key()) == target_key).then(|| Arc::clone(entry.value()))
        })
    }

    /// Load persisted session mappings from durable store (Phase 11.4 Stateless Fix)
    pub async fn load_sessions(&self) -> Result<()> {
        if let Some(mgr) = self.session_manager.get() {
            match mgr.list_sessions().await {
                Ok(sessions) => {
                    info!("Restoring {} sessions from durable store", sessions.len());
                    for (session_id, role_name) in sessions {
                        let role = self.parse_role_name(&role_name);
                        self.active_agents.insert(session_id, role);
                    }
                }
                Err(e) => {
                    error!("Failed to load persisted sessions: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start the background scheduler
    #[cfg(feature = "cron")]
    pub async fn start_scheduler(self: &Arc<Self>) -> Arc<benshu_scheduler::Scheduler> {
        let scheduler = self
            .scheduler
            .get_or_init(|| async {
                let store = benshu_scheduler::RedbCronStore::new("data/cron.redb")
                    .ok()
                    .map(|s| Box::new(s) as Box<dyn benshu_scheduler::CronStore>);
                let scheduler = benshu_scheduler::Scheduler::new(
                    Arc::downgrade(self) as Weak<dyn JobHandler>,
                    store,
                )
                .await;

                // Load existing jobs from store
                let _ = scheduler.load_jobs().await;

                // Link scheduler to memory if available
                if let Some(memory) = self.memory.get() {
                    memory.link_scheduler(Arc::downgrade(&scheduler));
                }

                let s_clone = Arc::clone(&scheduler);
                tokio::spawn(async move {
                    s_clone.run().await;
                });
                scheduler
            })
            .await
            .clone();

        scheduler
    }

    /// Route a message to the appropriate agent
    pub async fn route(&self, message: AgentMessage) -> Result<Option<AgentMessage>> {
        self.metrics.counter_inc(
            &format!(
                "agent_message_routed_from_{}_to_{}",
                message.from.name(),
                message.to.as_ref().map(|r| r.name()).unwrap_or("broadcast")
            ),
            1,
        );

        if let Some(target_role) = message.to.clone() {
            // Directed message
            if let Some(agent) = self.get_or_spawn(&target_role).await {
                let role_name = target_role.name().to_string();
                match agent.handle_message(message).await {
                    Ok(res) => return Ok(res),
                    Err(e) => {
                        self.metrics
                            .counter_inc(&format!("agent_message_error_{}", role_name), 1);
                        return Err(e);
                    }
                }
            } else {
                return Err(Error::AgentCommunication(format!(
                    "No agent with role: {:?}",
                    target_role
                )));
            }
        }

        // Broadcast message - send to all agents except sender
        let from_role = message.from.clone();
        let mut responses = Vec::new();

        for entry in self.agents.iter() {
            if entry.key() != &from_role {
                if let Some(response) = entry.value().handle_message(message.clone()).await? {
                    responses.push(response);
                }
            }
        }

        // Return first response for now (could aggregate in future)
        Ok(responses.into_iter().next())
    }

    /// Orchestrate a task through a dynamic workflow of agents
    pub async fn orchestrate(&self, task: &str, workflow: Vec<AgentRole>) -> Result<String> {
        let mut _window =
            crate::agent::evolution::observation::ObservationWindow::with_duration_and_threshold(
                std::time::Duration::from_secs(60),
                5,
            );

        tokio::time::timeout(std::time::Duration::from_secs(600), async {
            self.orchestrate_internal(task, workflow).await
        })
        .await
        .map_err(|_| {
            Error::AgentCoordination("Orchestration timeout after 10 minutes".to_string())
        })?
    }

    async fn orchestrate_internal(&self, task: &str, workflow: Vec<AgentRole>) -> Result<String> {
        self.metrics.counter_inc("agent_orchestrate_rounds", 1);

        if workflow.is_empty() {
            return Err(Error::AgentCoordination(
                "Workflow cannot be empty".to_string(),
            ));
        }

        let lead_role = &workflow[0];
        let lead = self.get_or_spawn(lead_role).await.ok_or_else(|| {
            Error::AgentCoordination(format!("No lead agent found for role: {:?}", lead_role))
        })?;

        // Phase 6.1: Integral Approval Flow
        if let Some(approval_handler) = self.approval_handler.get() {
            if !approval_handler
                .approve(
                    "Orchestrate",
                    task,
                    crate::skills::tool::SafetyLevel::Yellow,
                )
                .await?
            {
                return Err(Error::AgentCoordination(
                    "Task rejected by approval handler".to_string(),
                ));
            }
        }

        // 1. Initial processing by lead agent
        let mut current_result = lead.process(task).await?;
        let mut current_role = lead_role.clone();

        // 2. Pass result through the rest of the workflow chain OR follow handovers
        let mut i = 1;
        let mut rounds = 0;
        const MAX_ROUNDS_INNER: usize = 20;

        while i < workflow.len() || rounds < MAX_ROUNDS_INNER {
            rounds += 1;
            if rounds >= MAX_ROUNDS_INNER {
                return Err(Error::AgentCoordination(
                    "Maximum coordination rounds reached".to_string(),
                ));
            }

            let next_role = if i < workflow.len() {
                &workflow[i]
            } else {
                &current_role
            };

            if let Some(agent) = self.get_or_spawn(next_role).await {
                let msg_type = if i == workflow.len() - 1 && i > 0 {
                    MessageType::Approval
                } else {
                    MessageType::Request
                };

                let message = AgentMessage {
                    from: current_role.clone(),
                    to: Some(next_role.clone()),
                    content: current_result.clone(),
                    msg_type,
                };

                // Enhanced Error Handling: Retry logic
                let mut retries = 3;
                let mut response_opt = None;
                while retries > 0 {
                    match agent.handle_message(message.clone()).await {
                        Ok(res) => {
                            response_opt = res;
                            break;
                        }
                        Err(e) => {
                            retries -= 1;
                            if retries == 0 {
                                return Err(e);
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                    }
                }

                if let Some(response) = response_opt {
                    // Check for Handover (Dynamic or Content-based)
                    if matches!(response.msg_type, MessageType::Handover) {
                        // Priority 1: Direct target in AgentMessage
                        if let Some(handover_to) = response.to {
                            if self.ensure_worker_ready(&handover_to).await {
                                tracing::info!(
                                    "Dynamic Handover from {:?} to {:?}",
                                    next_role,
                                    handover_to
                                );
                                current_result = response.content;
                                current_role = handover_to;
                                continue;
                            }
                        }

                        // Priority 2: Content-based parsing (e.g. "Handover to Explorer")
                        if let Some(parsed_role) = self.parse_handover_role(&response.content) {
                            if self.ensure_worker_ready(&parsed_role).await {
                                tracing::info!(
                                    "Content-based Handover from {:?} to {:?}",
                                    next_role,
                                    parsed_role
                                );
                                current_result = response.content;
                                current_role = parsed_role;
                                continue;
                            }
                        }
                    }

                    // Check for strict denial/stop signal
                    if matches!(response.msg_type, MessageType::Denial) {
                        return Err(Error::AgentCoordination(format!(
                            "Agent {:?} denied processing: {}",
                            next_role, response.content
                        )));
                    }

                    // Standard Step Advancement
                    current_result = response.content;
                    current_role = next_role.clone();
                }
            } else if i >= workflow.len() {
                // We were in a dynamic handover loop and reached the end
                break;
            } else {
                return Err(Error::AgentCoordination(format!(
                    "Agent {:?} not found",
                    next_role
                )));
            }

            if i < workflow.len() {
                i += 1;
            }
        }

        Ok(current_result)
    }

    /// Phase 12.1: Parallel Orchestration (Cellular Fission)
    /// Spawns multiple specialized agents to work on a task simultaneously.
    pub async fn orchestrate_parallel(
        &self,
        task: &str,
        roles: Vec<AgentRole>,
    ) -> Result<Vec<String>> {
        let mut handlers = Vec::new();

        for role in roles {
            let task_clone = task.to_string();
            let agent = self
                .get_or_spawn(&role)
                .await
                .ok_or_else(|| Error::AgentCoordination(format!("Agent {:?} not found", role)))?;

            handlers.push(tokio::spawn(
                async move { agent.process(&task_clone).await },
            ));
        }

        let mut results = Vec::new();
        for h in handlers {
            results.push(
                h.await
                    .map_err(|e| Error::Internal(format!("Task panic: {}", e)))??,
            );
        }

        Ok(results)
    }

    /// Heuristic to parse handover role from message content (Enhanced with fuzzy matching)
    fn parse_handover_role(&self, content: &str) -> Option<AgentRole> {
        let lower = content.to_lowercase();

        // 1. Precise/Regex-lite parsing
        // We look for "handover to [target]" or "delegate to [target]"
        let keywords = ["handover to ", "delegate to "];
        let mut target = None;

        for kw in keywords {
            if let Some(idx) = lower.find(kw) {
                let start = idx + kw.len();
                target = lower[start..]
                    .split_whitespace()
                    .next()
                    .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()));
                break;
            }
        }

        if let Some(t) = target {
            // 2. Fuzzy Matching against registered roles
            // Supports: "research", "trader", "risk", etc.
            for entry in self.agents.iter() {
                let role_name = entry.key().name().to_lowercase();
                if role_name.contains(t) || t.contains(&role_name) {
                    return Some(entry.key().clone());
                }
            }
        }

        None
    }

    /// Process a chat session, managing active agent and handovers automatically
    pub async fn chat_session(
        &self,
        session_id: &str,
        messages: Vec<Message>,
    ) -> Result<crate::agent::ChatOutcome> {
        // 1. Determine active agent for this session
        let mut active_role = if let Some(mgr) = self.session_manager.get() {
            if let Ok(Some(role_name)) = mgr.load_session_mapping(session_id).await {
                self.parse_role_name(&role_name)
            } else {
                self.active_agents
                    .entry(session_id.to_string())
                    .or_insert_with(|| self.primary_role())
                    .clone()
            }
        } else {
            self.active_agents
                .entry(session_id.to_string())
                .or_insert_with(|| self.primary_role())
                .clone()
        };

        let prime_role = self.primary_role();
        if active_role != prime_role {
            active_role = prime_role.clone();
            self.switch_session_agent(session_id, prime_role.clone());
        }

        let agent = self.get_or_spawn(&active_role).await.ok_or_else(|| {
            Error::AgentCoordination(format!("Active agent {:?} not found", active_role))
        })?;

        // 2. Call agent chat
        let mut outcome = agent.chat(messages, Some(session_id.to_string())).await?;
        outcome.ownership = TaskOwnership::prime_owned(
            prime_role.clone(),
            active_role.clone(),
            Some(session_id.to_string()),
        );

        // 3. Detect Handover (Explicit from ChatOutcome)
        if let Some(target_role) = outcome.handover.clone() {
            if self.ensure_worker_ready(&target_role).await {
                if active_role == prime_role {
                    tracing::info!(
                        "Prime-owned delegation detected in session {}: {} -> {:?}",
                        session_id,
                        prime_role.name(),
                        target_role
                    );
                    outcome.delegation = Some(DelegationRecord {
                        delegated_by: prime_role.clone(),
                        delegated_to: target_role,
                        mode: DelegationMode::InternalRecommendation,
                        task_owner: prime_role.clone(),
                        session_id: Some(session_id.to_string()),
                        summary: Some("Prime agent retained user-facing ownership while specialist execution was recommended.".to_string()),
                    });
                    self.switch_session_agent(session_id, prime_role);
                } else {
                    tracing::info!(
                        "Auto-handover detected in session {}: stepping into {:?}",
                        session_id,
                        target_role
                    );
                    outcome.delegation = Some(DelegationRecord {
                        delegated_by: active_role.clone(),
                        delegated_to: target_role.clone(),
                        mode: DelegationMode::SessionTransfer,
                        task_owner: active_role.clone(),
                        session_id: Some(session_id.to_string()),
                        summary: Some("Legacy specialist session ownership transferred to another specialist.".to_string()),
                    });
                    self.switch_session_agent(session_id, target_role);
                }
            }
        }

        Ok(outcome)
    }

    /// Explicitly switch the active agent for a session
    pub fn switch_session_agent(&self, session_id: &str, role: AgentRole) {
        self.active_agents
            .insert(session_id.to_string(), role.clone());
        if let Some(mgr) = self.session_manager.get() {
            let role_name = role.name().to_string();
            let sid = session_id.to_string();
            let mgr_clone = mgr.clone();
            tokio::spawn(async move {
                let _ = mgr_clone.save_session_mapping(&sid, &role_name).await;
            });
        }
    }

    /// Get list of registered agent roles
    pub fn roles(&self) -> Vec<AgentRole> {
        let mut roles = self.get_active_roles();
        for worker_role in self.worker_roles() {
            if !roles
                .iter()
                .any(|existing| existing.name() == worker_role.name())
            {
                roles.push(worker_role);
            }
        }
        roles
    }

    /// Snapshot of all active session → agent-role mappings
    pub fn active_agents(&self) -> Vec<(String, AgentRole)> {
        self.active_agents
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    /// Cancel the active foreground task bound to a specific session.
    pub fn cancel_session(&self, session_id: &str) -> bool {
        let Some(role) = self
            .active_agents
            .get(session_id)
            .map(|entry| entry.value().clone())
        else {
            return false;
        };
        let Some(agent) = self.get(&role) else {
            return false;
        };
        agent.cancel();
        agent.ensure_active_token();
        true
    }

    /// Remove a session (returns true if it existed)
    pub fn remove_session(&self, session_id: &str) -> bool {
        if let Some(mgr) = self.session_manager.get() {
            let sid = session_id.to_string();
            let mgr_clone = mgr.clone();
            tokio::spawn(async move {
                let _ = mgr_clone.remove_session(&sid).await;
            });
        }
        self.active_agents.remove(session_id).is_some()
    }

    /// Set the shared memory for the coordinator
    pub fn set_memory(&self, memory: Arc<dyn Memory>) {
        #[cfg(feature = "cron")]
        if let Some(scheduler) = self.scheduler.get() {
            memory.link_scheduler(Arc::downgrade(scheduler));
        }
        let _ = self.memory.set(memory);
    }

    /// Set the shared config for the coordinator
    pub fn set_config(&self, config: Arc<parking_lot::RwLock<crate::config::AppConfig>>) {
        let _ = self.config.set(config);
    }
}

fn normalize_capability_text(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch) {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn task_text_overlaps(task: &str, candidate: &str) -> bool {
    if task.is_empty() || candidate.is_empty() {
        return false;
    }
    if task.contains(candidate) || candidate.contains(task) {
        return true;
    }
    candidate
        .split('_')
        .filter(|part| part.chars().count() >= 2)
        .any(|part| task.contains(part))
}

fn worker_artifact_policy_capabilities(
    policy: Option<&serde_json::Value>,
    limit: usize,
) -> Vec<String> {
    let mut capabilities = Vec::new();
    let Some(policy) = policy else {
        return capabilities;
    };
    let Some(handles) = policy.get("handles").and_then(serde_json::Value::as_array) else {
        return capabilities;
    };
    for handle in handles {
        if let Some(artifact) = handle.get("artifact").and_then(serde_json::Value::as_str) {
            push_unique_capability(&mut capabilities, artifact.to_string());
        }
        if capabilities.len() >= limit {
            break;
        }
    }
    capabilities
}

fn worker_artifact_policy_match_score(
    policy: Option<&serde_json::Value>,
    requested_role: &str,
    task: &str,
) -> i32 {
    let Some(policy) = policy else {
        return 0;
    };

    let mut strings = Vec::new();
    collect_policy_strings(policy, &mut strings);
    let mut score = 0;
    for value in strings {
        let normalized = normalize_capability_text(&value);
        if normalized.chars().count() < 2 {
            continue;
        }
        if !requested_role.is_empty() {
            if requested_role == normalized {
                score += 5_000;
            } else if requested_role.contains(&normalized) || normalized.contains(requested_role) {
                score += 2_500;
            }
        }
        if !task.is_empty() && task.contains(&normalized) {
            score += 1_200;
        }
    }
    score.min(8_000)
}

fn collect_policy_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => {
            let text = text.trim();
            if !text.is_empty() {
                out.push(text.to_string());
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_policy_strings(value, out);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values() {
                collect_policy_strings(value, out);
            }
        }
        _ => {}
    }
}

fn push_unique_capability(target: &mut Vec<String>, value: String) {
    if !target
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&value))
    {
        target.push(value);
    }
}

#[cfg(feature = "cron")]
#[async_trait]
impl JobHandler for Coordinator {
    async fn execute(
        &self,
        name: &str,
        payload: &JobPayload,
    ) -> std::result::Result<Option<String>, SchedulerError> {
        info!("Executing scheduled job: {}", name);

        match payload {
            JobPayload::AgentTurn { role, prompt } => {
                if let Some(agent) = self.get(role) {
                    agent
                        .process(prompt)
                        .await
                        .map(|s| Some(s))
                        .map_err(|e| SchedulerError::Execution(e.to_string()))
                } else {
                    Err(SchedulerError::Execution(format!(
                        "Target agent {:?} not found",
                        role
                    )))
                }
            }
            JobPayload::SummarizeDoc {
                collection,
                path,
                content,
            } => {
                let agent = self
                    .get(&self.primary_role())
                    .or_else(|| self.get(&AgentRole::Researcher))
                    .ok_or_else(|| {
                        SchedulerError::Execution(
                            "No agent available for summarization".to_string(),
                        )
                    })?;

                let prompt = format!(
                    "Summarize the following document in about 200 words. Focus on core concepts and key information.\n\nDocument Content:\n{}", 
                    content
                );

                let summary = agent
                    .process(&prompt)
                    .await
                    .map_err(|e| SchedulerError::Execution(e.to_string()))?;

                if let Some(memory) = self.memory.get() {
                    memory
                        .update_summary(collection, path, &summary)
                        .await
                        .map_err(|e| SchedulerError::Execution(e.to_string()))?;
                }
                Ok(Some(format!("Summary generated ({} chars)", summary.len())))
            }
            JobPayload::DistillLogs { limit } => {
                let agent = self
                    .get(&self.primary_role())
                    .or_else(|| self.get(&AgentRole::Researcher))
                    .ok_or_else(|| {
                        SchedulerError::Execution(
                            "No agent available for log distillation".to_string(),
                        )
                    })?;

                let security = agent.security().ok_or_else(|| {
                    SchedulerError::Internal(
                        "Agent has no security handler for log access".to_string(),
                    )
                })?;

                let logs = security
                    .retrieve_audit_logs(*limit)
                    .await
                    .map_err(|e| SchedulerError::Execution(e.to_string()))?;
                if logs.is_empty() {
                    return Ok(Some("No logs to distill".to_string()));
                }

                let mut log_text = String::from("### SYSTEM EXECUTION LOGS FOR DISTILLATION\n\n");
                for log in logs {
                    log_text.push_str(&format!(
                        "- Time: {}\n  Tool: {}\n  Success: {}\n  Arguments: {}\n  Output Preview: {}\n\n",
                        log.timestamp, log.tool_name, log.success, log.arguments, log.output_preview
                    ));
                }

                let prompt = format!(
                    "{}\n\n### MISSION\nYou are performing a background session distillation. Analyze the logs above.\n\
                    1. Identify recurring patterns or successful tool combinations.\n\
                    2. Note any repeated failures or tricky bottlenecks.\n\
                    3. Distill these into 3-5 'Core Insights'.\n\
                    4. Use `upsert_knowledge` to persist these insights.",
                    log_text
                );

                let result = agent
                    .process(&prompt)
                    .await
                    .map_err(|e| SchedulerError::Execution(e.to_string()))?;
                Ok(Some(format!("Distilled insights: {}", result)))
            }
            JobPayload::ConsolidateMemory {
                limit,
                agent_id,
                global_context,
            } => {
                let agent = self
                    .get(&self.primary_role())
                    .or_else(|| self.get(&AgentRole::Researcher))
                    .ok_or_else(|| {
                        SchedulerError::Execution(
                            "No agent available for memory consolidation".to_string(),
                        )
                    })?;

                if let Some(memory) = self.memory.get() {
                    let unverified = memory
                        .list_unverified(agent_id.as_deref(), *limit)
                        .await
                        .map_err(|e| SchedulerError::Execution(e.to_string()))?;
                    if unverified.is_empty() {
                        return Ok(Some("No memories to consolidate".to_string()));
                    }

                    let global_status = global_context
                        .clone()
                        .unwrap_or_else(|| "Healthy".to_string());
                    let pool_text = unverified
                        .iter()
                        .enumerate()
                        .map(|(i, fact)| {
                            format!(
                                "[{}] Category: {}, Importance: {}, Content: {}\n",
                                i + 1,
                                fact.category,
                                fact.importance,
                                fact.content
                            )
                        })
                        .collect::<String>();

                    let prompt = format!(
                        "### CONSOLIDATION MISSION\n\nUNVERIFIED KNOWLEDGE POOL:\n{}\n\nGLOBAL CONTEXT:\n{}\n\nPerform cognitive unification and crystallize facts.",
                        pool_text, global_status
                    );

                    let result = agent
                        .process(&prompt)
                        .await
                        .map_err(|e| SchedulerError::Execution(e.to_string()))?;
                    Ok(Some(format!("Consolidation result: {}", result)))
                } else {
                    Ok(Some("No memory found".to_string()))
                }
            }
        }
    }
}
/// A context injector that informs an agent about other agents in the swarm
pub struct SwarmInjector {
    coordinator: Weak<Coordinator>,
}

impl SwarmInjector {
    /// Create a new SwarmInjector
    pub fn new(coordinator: Weak<Coordinator>) -> Self {
        Self { coordinator }
    }

    fn latest_user_text(history: &[Message]) -> Option<String> {
        history
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .map(|message| message.content.as_text())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }
}

#[async_trait]
impl crate::agent::context::ContextInjector for SwarmInjector {
    async fn inject(&self, history: &[Message]) -> Result<Vec<Message>> {
        let Some(query) = Self::latest_user_text(history) else {
            return Ok(Vec::new());
        };
        let route = classify_query_capability_route(&query);
        let mode = select_coordinator_task_mode(route, false);
        if matches!(mode, CoordinatorTaskMode::ChatLite) {
            return Ok(Vec::new());
        }
        if route.is_some_and(|route| {
            capability_route_prefers_direct_tool_surface_for_query(route, &query)
        }) {
            return Ok(Vec::new());
        }

        if let Some(coordinator) = self.coordinator.upgrade() {
            let mut info = String::from("### A2A Coordinator Guidance\n");
            info.push_str("You are BenShu, the single visible frontstage agent. Use the `delegate` tool to assign execution to internal workers, then return one clean user-facing answer.\n\n");
            info.push_str("Default to the narrowest matching worker first. If one worker is not enough, coordinate additional workers explicitly with `delegate` and `shared_board`.\n");
            info.push_str("Do not narrate internal worker topology unless the user explicitly asks for system details.\n");
            info.push_str("Keep worker selection implicit unless dispatch is actually needed.\n");
            info.push_str(
                "If you are unsure which worker fits, use `tool_search` before `delegate`.\n",
            );
            let _ = coordinator;

            return Ok(vec![Message::system(info)]);
        }
        Ok(Vec::new())
    }
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::context::ContextInjector;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockAgent {
        role: AgentRole,
        response: String,
    }

    #[async_trait]
    impl MultiAgent for MockAgent {
        fn role(&self) -> AgentRole {
            self.role.clone()
        }

        async fn handle_message(&self, _message: AgentMessage) -> Result<Option<AgentMessage>> {
            Ok(Some(AgentMessage {
                from: self.role.clone(),
                to: None,
                content: self.response.clone(),
                msg_type: MessageType::Response,
            }))
        }

        async fn process(&self, _input: &str) -> Result<String> {
            Ok(self.response.clone())
        }

        async fn chat(
            &self,
            _messages: Vec<Message>,
            _session_id: Option<String>,
        ) -> Result<crate::agent::ChatOutcome> {
            Ok(crate::agent::protocol::ChatOutcome {
                response: self.response.clone(),
                thoughts: vec![],
                tool_calls: vec![],
                metabolic_stats: None,
                ownership: crate::agent::protocol::TaskOwnership::direct(self.role.clone(), None),
                delegation: None,
                handover: None,
                runtime_task: None,
                run_trace: None,
            })
        }

        fn agent_identity(&self) -> Option<Arc<parking_lot::RwLock<Option<AgentIdentity>>>> {
            None
        }

        fn events(&self) -> tokio::sync::broadcast::Receiver<crate::agent::AgentEvent> {
            let (_, rx) = tokio::sync::broadcast::channel(1);
            rx
        }

        fn security(&self) -> Option<Arc<dyn crate::security::SecurityHandler>> {
            None
        }

        fn cancel(&self) {}
        fn ensure_active_token(&self) {}
        /// Set the list of all available roles in the swarm
        fn set_all_roles(&self, _roles: Vec<AgentRole>) {}
    }

    struct MockWorkerSpawner {
        coordinator: Arc<Coordinator>,
        response: String,
        spawn_count: AtomicUsize,
    }

    #[async_trait]
    impl WorkerSpawner for MockWorkerSpawner {
        async fn ensure_worker(&self, role: &AgentRole) -> Result<bool> {
            self.spawn_count.fetch_add(1, Ordering::SeqCst);
            self.coordinator.register(Arc::new(MockAgent {
                role: role.clone(),
                response: self.response.clone(),
            }));
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_coordinator() {
        let coordinator = Coordinator::new();

        coordinator.register(Arc::new(MockAgent {
            role: AgentRole::Researcher,
            response: "Research complete".to_string(),
        }));

        coordinator.register(Arc::new(MockAgent {
            role: AgentRole::Trader,
            response: "Trade executed".to_string(),
        }));

        assert_eq!(coordinator.roles().len(), 2);
    }

    #[test]
    fn worker_blueprint_is_available_without_becoming_live_agent() {
        let coordinator = Coordinator::new();
        coordinator.register_worker_blueprint(WorkerBlueprint {
            role: AgentRole::Custom("coder".to_string()),
            agent_path: PathBuf::from("/tmp/coder"),
            display_name: "Coder".to_string(),
            description: Some("Code execution specialist.".to_string()),
            tools: vec!["fs".to_string(), "shell".to_string()],
            artifact_policy: None,
        });

        assert_eq!(coordinator.worker_roles().len(), 1);
        assert_eq!(coordinator.get_active_roles().len(), 0);
        assert_eq!(coordinator.roles().len(), 1);
    }

    #[test]
    fn worker_capability_index_ranks_policy_matches_without_new_route_enum() {
        let coordinator = Coordinator::new();
        coordinator.register_worker_blueprint(WorkerBlueprint {
            role: AgentRole::Custom("researcher".to_string()),
            agent_path: PathBuf::from("/tmp/researcher"),
            display_name: "Researcher".to_string(),
            description: Some("General lookup worker.".to_string()),
            tools: vec!["web_search".to_string()],
            artifact_policy: None,
        });
        coordinator.register_worker_blueprint(WorkerBlueprint {
            role: AgentRole::Custom("medical_researcher".to_string()),
            agent_path: PathBuf::from("/tmp/medical_researcher"),
            display_name: "Medical Researcher".to_string(),
            description: Some("Evidence review worker.".to_string()),
            tools: vec!["web_search".to_string(), "knowledge_import_url".to_string()],
            artifact_policy: Some(serde_json::json!({
                "handles": [{
                    "artifact": "clinical_literature_review",
                    "triggers": ["医学论文", "clinical trial", "治疗心脏病"],
                    "intents": ["evidence review", "knowledge import"]
                }]
            })),
        });

        let best = coordinator
            .best_worker_capability_match(Some("auto"), "查找治疗心脏病的医学论文并存入知识库")
            .expect("policy-backed worker should match");

        assert_eq!(best.role.name(), "medical_researcher");
        assert_eq!(best.source, WorkerCapabilityMatchSource::ArtifactPolicy);
        assert!(best.score > 0);
    }

    #[test]
    fn explicit_worker_role_request_is_not_overridden_by_artifact_policy() {
        let coordinator = Coordinator::new();
        coordinator.register_worker_blueprint(WorkerBlueprint {
            role: AgentRole::Custom("researcher".to_string()),
            agent_path: PathBuf::from("/tmp/researcher"),
            display_name: "Researcher".to_string(),
            description: Some("Search and knowledge import specialist.".to_string()),
            tools: vec![
                "web_search".to_string(),
                "fetch_document".to_string(),
                "knowledge_import_url".to_string(),
            ],
            artifact_policy: Some(serde_json::json!({
                "handles": [{
                    "artifact": "research_material",
                    "triggers": ["search", "knowledge import"]
                }]
            })),
        });
        coordinator.register_worker_blueprint(WorkerBlueprint {
            role: AgentRole::Custom("writer".to_string()),
            agent_path: PathBuf::from("/tmp/writer"),
            display_name: "Writer".to_string(),
            description: Some("Longform writing specialist.".to_string()),
            tools: vec!["writing".to_string()],
            artifact_policy: Some(serde_json::json!({
                "handles": [{
                    "artifact": "longform_fiction",
                    "triggers": ["novel", "write", "500000 words", "knowledge base"]
                }]
            })),
        });

        let best = coordinator
            .best_worker_capability_match(
                Some("researcher"),
                "Search for material, import it into the knowledge base, then use it for a 500000 word novel.",
            )
            .expect("explicit researcher route should match");

        assert_eq!(best.role.name(), "researcher");
        assert_eq!(best.source, WorkerCapabilityMatchSource::ExactRole);
    }

    #[tokio::test]
    async fn coordinator_lazily_spawns_blueprinted_worker_on_route() {
        let coordinator = Arc::new(Coordinator::new());
        coordinator.register_worker_blueprint(WorkerBlueprint {
            role: AgentRole::Custom("coder".to_string()),
            agent_path: PathBuf::from("/tmp/coder"),
            display_name: "Coder".to_string(),
            description: Some("Code execution specialist.".to_string()),
            tools: vec!["fs".to_string(), "shell".to_string()],
            artifact_policy: None,
        });
        let spawner = Arc::new(MockWorkerSpawner {
            coordinator: coordinator.clone(),
            response: "Lazy worker ready".to_string(),
            spawn_count: AtomicUsize::new(0),
        });
        coordinator.set_worker_spawner(spawner.clone());

        let response = coordinator
            .route(AgentMessage {
                from: AgentRole::Custom("benshu".to_string()),
                to: Some(AgentRole::Custom("coder".to_string())),
                content: "help".to_string(),
                msg_type: MessageType::Request,
            })
            .await
            .expect("route should succeed")
            .expect("worker should respond");

        assert_eq!(response.content, "Lazy worker ready");
        assert_eq!(spawner.spawn_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn coordinator_orchestrate_parallel_lazily_spawns_blueprinted_worker() {
        let coordinator = Arc::new(Coordinator::new());
        coordinator.register_worker_blueprint(WorkerBlueprint {
            role: AgentRole::Custom("researcher".to_string()),
            agent_path: PathBuf::from("/tmp/researcher"),
            display_name: "Researcher".to_string(),
            description: Some("Information retrieval specialist.".to_string()),
            tools: vec!["web_search".to_string(), "web_fetch".to_string()],
            artifact_policy: None,
        });
        let spawner = Arc::new(MockWorkerSpawner {
            coordinator: coordinator.clone(),
            response: "Spawned researcher".to_string(),
            spawn_count: AtomicUsize::new(0),
        });
        coordinator.set_worker_spawner(spawner.clone());

        let outcomes = coordinator
            .orchestrate_parallel("status", vec![AgentRole::Custom("researcher".to_string())])
            .await
            .expect("parallel orchestration should succeed");

        assert_eq!(outcomes, vec!["Spawned researcher".to_string()]);
        assert_eq!(spawner.spawn_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn swarm_injector_keeps_worker_roster_compact() {
        let coordinator = Arc::new(Coordinator::new());
        coordinator.register_worker_blueprint(WorkerBlueprint {
            role: AgentRole::Custom("pdf".to_string()),
            agent_path: PathBuf::from("/tmp/pdf"),
            display_name: "PDF".to_string(),
            description: Some("PDF parsing specialist.".to_string()),
            tools: vec!["pdf_parse".to_string()],
            artifact_policy: None,
        });

        let injector = SwarmInjector::new(Arc::downgrade(&coordinator));
        let injected = injector
            .inject(&[Message::user("帮我修一下这个 Rust 仓库里的 bug 并提交补丁")])
            .await
            .expect("inject should succeed");
        let rendered = injected
            .iter()
            .map(|message| message.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("A2A Coordinator Guidance"));
        assert!(rendered.contains("tool_search"));
        assert!(rendered.contains("single visible frontstage agent"));
        assert!(!rendered.contains("Currently registered specialist roles"));
        assert!(!rendered.contains("PDF parsing specialist"));
        assert!(!rendered.contains("pdf_parse"));
        assert!(!rendered.contains("pdf."));
    }

    #[tokio::test]
    async fn swarm_injector_stays_silent_for_plain_chat() {
        let coordinator = Arc::new(Coordinator::new());
        let injector = SwarmInjector::new(Arc::downgrade(&coordinator));

        let injected = injector
            .inject(&[Message::user("你好，随便聊两句")])
            .await
            .expect("inject should succeed");

        assert!(injected.is_empty());
    }
}
