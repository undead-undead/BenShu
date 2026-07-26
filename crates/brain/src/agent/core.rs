//! Agent system - the core AI agent abstraction

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

use crate::agent::agent_identity::AgentIdentity;
use crate::agent::cache::Cache;
use crate::agent::context::ContextManager;
use crate::agent::governance::GovernanceContext;
use crate::agent::memory::{BackgroundEnvelope, Memory};
use crate::agent::multi_agent::AgentRole;
use crate::agent::protocol::*;
use crate::agent::provider::Provider;
use crate::agent::runtime_support::{ActiveForegroundTasks, RuntimeStageSignal};
use crate::hooks::{HookEngine, RuntimeHookCapture, RuntimeHookRefs};
use crate::infra::observable::MetricsRegistry;
use crate::notification::Notifier;
use crate::skills::tool::ToolSet;

use crate::agent::evolution::consolidation::SleepConsolidator;
use crate::agent::evolution::evolution_manager::EvolutionManager;
use crate::security::SecurityHandler;

/// Unique marker for preemptive interruptions
pub const MARKER_INTERJECTION: &str = "### HOT-INTERJECTION";

#[derive(Debug, Clone, Default)]
pub struct BackgroundRuntimeStats {
    pub total_attempts: usize,
    pub skip_count: usize,
    pub reject_count: usize,
    pub refresh_session_count: usize,
    pub promote_relationship_count: usize,
    pub rewrite_count: usize,
}

// Swarm types handled via benshu-comm

// Core anchor: canonical Agent runtime state.
/// The main Agent struct
pub struct Agent<P: Provider + 'static> {
    pub(crate) provider: Arc<P>,
    pub(crate) tools: ToolSet,
    pub(crate) config: AgentConfig,
    pub(crate) context_manager: ContextManager,
    pub(crate) events: broadcast::Sender<AgentEvent>,
    pub(crate) approval_handler: Arc<dyn ApprovalHandler>,
    pub(crate) cache: Option<Arc<dyn Cache>>,
    pub(crate) notifier: Option<Arc<dyn Notifier>>,
    pub(crate) memory: Option<Arc<dyn Memory>>,
    pub(crate) background_envelope: Arc<parking_lot::RwLock<Option<BackgroundEnvelope>>>,
    pub(crate) background_runtime_stats: Arc<parking_lot::RwLock<BackgroundRuntimeStats>>,
    pub(crate) session_id: Option<String>,
    pub(crate) metrics: Option<Arc<MetricsRegistry>>,

    pub(crate) enabled_tools: Option<Arc<parking_lot::RwLock<std::collections::HashSet<String>>>>,
    pub(crate) agent_identity: Arc<parking_lot::RwLock<Option<AgentIdentity>>>,
    pub(crate) security: Arc<dyn SecurityHandler>,
    pub(crate) lifecycle_token: Arc<parking_lot::RwLock<tokio_util::sync::CancellationToken>>,
    pub(crate) current_task_token: Arc<parking_lot::RwLock<tokio_util::sync::CancellationToken>>,
    pub(crate) seen_tools: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    pub(crate) evolution_manager: Option<Arc<EvolutionManager>>,
    pub(crate) sleep_consolidator: Option<Arc<SleepConsolidator>>,
    pub(crate) comm_client: Option<benshu_comm::client::CommClient>,
    /// Phase 8: Resource sensor for autonomous governance
    pub sensor: Option<Arc<parking_lot::RwLock<crate::infra::CapabilitySensor>>>,
    /// Phase 8.2: Meta-cognitive complexity estimator
    pub complexity_estimator: Option<Arc<dyn crate::agent::meta::ComplexityEstimator>>,
    /// Phase 12-D: Native Perception Hub
    pub(crate) sensory_hub: Option<Arc<dyn SensoryLiaison>>,
    /// Phase 16.1: Tactical Orchestrator (System 2)
    pub(crate) tactical_orchestrator: Arc<dyn crate::agent::tactical::TacticalOrchestrator>,
    pub(crate) cumulative_usage: Arc<parking_lot::RwLock<TokenUsage>>,
    /// Phase 18: Tracking background tasks for graceful shutdown/monitoring
    pub(crate) task_runner: Arc<dyn benshu_infra::traits::background::BackgroundTaskManager>,
    /// Phase 15.3: Active foreground reasoning tasks keyed by session (for hot-interjection)
    pub(crate) active_task: Arc<Mutex<ActiveForegroundTasks>>,
    /// Phase 16.3: Cognitive Autopilot (Behavioral Prediction)
    pub(crate) autopilot: Option<Arc<crate::agent::evolution::autopilot::Autopilot>>,
    /// Phase 25: Hallucination Guard (Fact Analysis)
    pub(crate) fact_checker: Option<Arc<dyn benshu_infra::traits::validation::FactChecker>>,
    /// Phase 15.2: Tracking the current conversational intent for JIT distillation
    pub(crate) current_intent: Arc<parking_lot::RwLock<Option<String>>>,
    /// Available roles in the swarm for routing decisions
    pub(crate) all_roles: Arc<parking_lot::RwLock<Vec<AgentRole>>>,
    /// Explicit governance context for spawned workers and sub-agents.
    pub(crate) governance: Arc<GovernanceContext>,
    /// Runtime hook engine for cross-cutting governance and trace surfaces.
    pub(crate) hook_engine: Arc<HookEngine>,
    /// Runtime hook refs injected into hook events.
    pub(crate) runtime_hook_refs: Arc<parking_lot::RwLock<Option<RuntimeHookRefs>>>,
    /// Runtime hook capture accumulated during a foreground run.
    pub(crate) runtime_hook_capture: Arc<parking_lot::RwLock<RuntimeHookCapture>>,
    /// Runtime stage signals captured from real execution events.
    pub(crate) runtime_stage_capture: Arc<parking_lot::RwLock<Vec<RuntimeStageSignal>>>,
    /// Guard against duplicate runtime startup.
    pub(crate) background_tasks_started: Arc<AtomicBool>,
}

// Core anchor: shared-clone semantics for the runtime shell.
impl<P: Provider + 'static> Clone for Agent<P> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            tools: self.tools.clone(),
            config: self.config.clone(),
            context_manager: self.context_manager.clone(),
            events: self.events.clone(),
            approval_handler: self.approval_handler.clone(),
            cache: self.cache.clone(),
            notifier: self.notifier.clone(),
            memory: self.memory.clone(),
            background_envelope: self.background_envelope.clone(),
            background_runtime_stats: self.background_runtime_stats.clone(),
            session_id: self.session_id.clone(),
            metrics: self.metrics.clone(),
            enabled_tools: self.enabled_tools.clone(),
            agent_identity: self.agent_identity.clone(),
            security: self.security.clone(),
            lifecycle_token: self.lifecycle_token.clone(),
            current_task_token: self.current_task_token.clone(),
            seen_tools: self.seen_tools.clone(),
            evolution_manager: self.evolution_manager.clone(),
            sleep_consolidator: self.sleep_consolidator.clone(),
            comm_client: self.comm_client.clone(),
            sensor: self.sensor.clone(),
            complexity_estimator: self.complexity_estimator.clone(),
            sensory_hub: self.sensory_hub.clone(),
            tactical_orchestrator: self.tactical_orchestrator.clone(),
            cumulative_usage: self.cumulative_usage.clone(),
            task_runner: self.task_runner.clone(),
            active_task: self.active_task.clone(),
            autopilot: self.autopilot.clone(),
            fact_checker: self.fact_checker.clone(),
            current_intent: self.current_intent.clone(),
            all_roles: self.all_roles.clone(),
            governance: self.governance.clone(),
            hook_engine: self.hook_engine.clone(),
            runtime_hook_refs: self.runtime_hook_refs.clone(),
            runtime_hook_capture: self.runtime_hook_capture.clone(),
            runtime_stage_capture: self.runtime_stage_capture.clone(),
            background_tasks_started: self.background_tasks_started.clone(),
        }
    }
}

// Core anchor: minimal public entrypoints that remain on core.
impl<P: Provider> Agent<P> {
    /// Create a new agent builder
    pub fn builder(provider: P) -> crate::agent::builder::AgentBuilder<P> {
        crate::agent::builder::AgentBuilder::new(provider)
    }

    // Redundant P2P/Old Swarm listen loop removed.
    // New A2A communication is handled via start_background_tasks using CommClient.
}

// Core anchor: tests live externally to keep runtime structure readable.
#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
