use crate::agent::agent_identity::AgentIdentity;
use crate::agent::agent_identity::{
    AgentConfigManager, AgentIdentityManager, UpdateAgentIdentityTool,
};
use crate::agent::cache::Cache;
use crate::agent::context::{ContextConfig, ContextInjector, ContextManager};
use crate::agent::core::Agent;
use crate::agent::evolution::consolidation::SleepConsolidator;
use crate::agent::evolution::evolution_manager::EvolutionManager;
use crate::agent::governance::GovernanceContext;
use crate::agent::memory::Memory;
use crate::agent::message::{Message, Role};
use crate::agent::middleware::install_default_runtime_middlewares;
use crate::agent::protocol::*;
use crate::agent::provider::Provider;
use crate::agent::runtime_support::ActiveForegroundTasks;
use crate::error::{Error, Result};
use crate::hooks::{discover_hooks, HookEngine, RuntimeHookCapture};
use crate::infra::observable::MetricsRegistry;
use crate::notification::Notifier;
use crate::security::SecurityHandler;
use crate::skills::tool::{ToolCatalogOverride, ToolSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, warn};

#[cfg(feature = "cron")]
use crate::skills::tool::CronTool;
#[cfg(feature = "cron")]
use benshu_scheduler::Scheduler;

pub struct AgentBuilder<P: Provider + 'static> {
    pub(crate) provider: P,
    pub(crate) tools: ToolSet,
    pub(crate) config: AgentConfig,
    pub(crate) injectors: Vec<Arc<dyn ContextInjector>>,
    pub(crate) approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub(crate) interaction_handler: Option<Arc<dyn InteractionHandler>>,
    pub(crate) notifier: Option<Arc<dyn Notifier>>,
    pub(crate) cache: Option<Arc<dyn Cache>>,
    pub(crate) memory: Option<Arc<dyn Memory>>,
    pub(crate) session_id: Option<String>,
    pub(crate) metrics: Option<Arc<MetricsRegistry>>,
    pub(crate) enabled_tools: Option<Arc<parking_lot::RwLock<std::collections::HashSet<String>>>>,
    pub(crate) agent_identity: Arc<parking_lot::RwLock<Option<AgentIdentity>>>,
    pub(crate) security: Option<Arc<dyn SecurityHandler>>,
    pub(crate) evolution_manager: Option<Arc<EvolutionManager>>,
    pub(crate) comm_client: Option<benshu_comm::client::CommClient>,
    pub(crate) sensor: Option<Arc<parking_lot::RwLock<crate::infra::CapabilitySensor>>>,
    pub(crate) complexity_estimator: Option<Arc<dyn crate::agent::meta::ComplexityEstimator>>,
    pub(crate) sensory_hub: Option<Arc<dyn SensoryLiaison>>,
    pub(crate) tactical_orchestrator: Option<Arc<dyn crate::agent::tactical::TacticalOrchestrator>>,
    pub(crate) governance_context: Option<Arc<GovernanceContext>>,
    pub(crate) lifecycle_token: Option<tokio_util::sync::CancellationToken>,
    pub(crate) cancel_token: Option<tokio_util::sync::CancellationToken>,
    pub(crate) active_task: Arc<tokio::sync::Mutex<ActiveForegroundTasks>>,
    pub(crate) fact_checker: Option<Arc<dyn benshu_infra::traits::validation::FactChecker>>,
    pub(crate) autopilot: Option<Arc<crate::agent::evolution::autopilot::Autopilot>>,
    pub(crate) image_gen: Option<Arc<dyn benshu_inference::backend::ImageGenBackend>>,
    pub(crate) memory_emitter: Option<Arc<dyn benshu_infra::traits::memory::MemoryEmitter>>,
    pub(crate) components: Vec<Arc<dyn crate::agent::component::AgentComponent>>,
    pub(crate) all_roles: Arc<parking_lot::RwLock<Vec<crate::agent::multi_agent::AgentRole>>>,
    pub(crate) initial_risk_score: f32,
}

impl<P: Provider + 'static> AgentBuilder<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            tools: ToolSet::new(),
            config: AgentConfig::default(),
            injectors: Vec::new(),
            approval_handler: None,
            interaction_handler: None,
            notifier: None,
            cache: None,
            memory: None,
            session_id: None,
            metrics: None,
            enabled_tools: None,
            agent_identity: Arc::new(parking_lot::RwLock::new(None)),
            security: None,
            evolution_manager: None,
            comm_client: None,
            sensor: None,
            complexity_estimator: None,
            sensory_hub: None,
            tactical_orchestrator: None,
            governance_context: None,
            lifecycle_token: None,
            cancel_token: None,
            active_task: Arc::new(tokio::sync::Mutex::new(ActiveForegroundTasks::new())),
            autopilot: None,
            fact_checker: None,
            image_gen: None,
            memory_emitter: None,
            components: Vec::new(),
            all_roles: Arc::new(parking_lot::RwLock::new(Vec::new())),
            initial_risk_score: 0.0,
        }
    }

    pub fn model(self, model: impl Into<String>) -> Self {
        self.with_model(model)
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.config.model = model.into();
        self
    }

    pub fn name(self, name: impl Into<String>) -> Self {
        self.with_name(name)
    }

    pub fn with_image_gen(
        mut self,
        image_gen: Arc<dyn benshu_inference::backend::ImageGenBackend>,
    ) -> Self {
        self.image_gen = Some(image_gen);
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    pub fn role(self, role: crate::agent::multi_agent::AgentRole) -> Self {
        self.with_role(role)
    }

    pub fn with_role(mut self, role: crate::agent::multi_agent::AgentRole) -> Self {
        self.config.role = role;
        self
    }

    pub fn agent_path(self, path: std::path::PathBuf) -> Self {
        self.with_agent_path(path)
    }

    pub fn with_agent_path(mut self, path: std::path::PathBuf) -> Self {
        self.config.agent_path = Some(path);
        self
    }

    pub fn system_prompt(self, prompt: impl Into<String>) -> Self {
        self.with_system_prompt(prompt)
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.preamble = prompt.into();
        self
    }

    pub fn preamble(self, prompt: impl Into<String>) -> Self {
        self.with_system_prompt(prompt)
    }

    pub fn with_preamble(self, prompt: impl Into<String>) -> Self {
        self.with_system_prompt(prompt)
    }

    pub fn temperature(self, temp: f64) -> Self {
        self.with_temperature(temp)
    }

    pub fn with_temperature(mut self, temp: f64) -> Self {
        self.config.temperature = Some(temp);
        self
    }

    pub fn max_tokens(self, tokens: u64) -> Self {
        self.with_max_tokens(tokens)
    }

    pub fn with_max_tokens(mut self, tokens: u64) -> Self {
        self.config.max_tokens = Some(tokens);
        self
    }

    pub fn efficiency_trigger(self, secs: u64) -> Self {
        self.with_efficiency_trigger(secs)
    }

    pub fn with_efficiency_trigger(mut self, secs: u64) -> Self {
        self.config.efficiency_trigger_secs = secs;
        self
    }

    pub fn with_max_parallel_tools(mut self, max: usize) -> Self {
        self.config.max_parallel_tools = max;
        self
    }

    pub fn with_default_max_steps(mut self, max: usize) -> Self {
        self.config.default_max_steps = max;
        self
    }

    pub fn extra_params(self, params: serde_json::Value) -> Self {
        self.with_extra_params(params)
    }

    pub fn with_extra_params(mut self, params: serde_json::Value) -> Self {
        self.config.extra_params = Some(params);
        self
    }

    pub fn tool_policy(self, policy: RiskyToolPolicy) -> Self {
        self.with_tool_policy(policy)
    }

    pub fn with_tool_policy(mut self, policy: RiskyToolPolicy) -> Self {
        self.config.tool_policy = policy;
        if let Some(ctx) = self.governance_context.take() {
            self.governance_context = Some(Arc::new(
                ctx.with_tool_policy(self.config.tool_policy.clone()),
            ));
        }
        self
    }

    pub fn with_response_reserve(mut self, reserve: usize) -> Self {
        self.config.response_reserve = reserve;
        self
    }

    pub fn with_tool_execution_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.config.tool_execution_timeout = timeout;
        self
    }

    pub fn with_enable_meta_cognition(mut self, enabled: bool) -> Self {
        self.config.enable_meta_cognition = enabled;
        self
    }

    pub fn with_all_roles(self, roles: Vec<crate::agent::multi_agent::AgentRole>) -> Self {
        *self.all_roles.write() = roles;
        self
    }

    pub fn with_status_recap_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.status_recap_prompt = Some(prompt.into());
        self
    }

    pub fn with_status_recap_threshold_steps(mut self, threshold: usize) -> Self {
        self.config.status_recap_threshold_steps = threshold;
        self
    }

    pub fn with_status_recap_threshold_chars(mut self, threshold: usize) -> Self {
        self.config.status_recap_threshold_chars = threshold;
        self
    }

    pub fn with_reflexion_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.reflexion_prompt = Some(prompt.into());
        self
    }

    pub fn with_token_budget(mut self, budget: u32) -> Self {
        self.config.token_budget = Some(budget);
        if let Some(ctx) = self.governance_context.take() {
            self.governance_context = Some(Arc::new(ctx.with_token_budget(Some(budget))));
        }
        self
    }

    pub fn with_jit_token_budget(mut self, budget: u32) -> Self {
        self.config.jit_token_budget = Some(budget);
        self
    }

    pub fn with_memory_emitter(
        mut self,
        emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>,
    ) -> Self {
        self.memory_emitter = Some(emitter);
        self
    }
    pub fn with_fact_checker(
        mut self,
        checker: Arc<dyn benshu_infra::traits::validation::FactChecker>,
    ) -> Self {
        self.fact_checker = Some(checker);
        self
    }

    pub fn with_approval_handler(mut self, handler: Arc<dyn ApprovalHandler>) -> Self {
        self.approval_handler = Some(handler);
        if let Some(ctx) = self.governance_context.take() {
            self.governance_context = Some(Arc::new(
                ctx.with_approval_handler(self.approval_handler.as_ref().unwrap().clone()),
            ));
        }
        self
    }

    pub fn with_cancel_token(mut self, token: tokio_util::sync::CancellationToken) -> Self {
        self.cancel_token = Some(token);
        self
    }

    pub fn with_lifecycle_token(mut self, token: tokio_util::sync::CancellationToken) -> Self {
        self.lifecycle_token = Some(token);
        self
    }

    pub fn with_interaction_handler(mut self, handler: Arc<dyn InteractionHandler>) -> Self {
        self.interaction_handler = Some(handler);
        self
    }

    pub fn with_notifier(mut self, notifier: Arc<dyn Notifier>) -> Self {
        self.notifier = Some(notifier);
        self
    }

    pub fn with_cache(mut self, cache: Arc<dyn Cache>) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn with_metrics(mut self, registry: Arc<MetricsRegistry>) -> Self {
        self.metrics = Some(registry);
        self
    }

    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn session_id(self, id: impl Into<String>) -> Self {
        self.with_session_id(id)
    }

    pub fn get_tools(&self) -> &ToolSet {
        &self.tools
    }

    pub fn with_enabled_tools(
        mut self,
        tools: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    ) -> Self {
        self.enabled_tools = Some(tools);
        self
    }

    pub fn with_agent_identity(self, identity: AgentIdentity) -> Self {
        *self.agent_identity.write() = Some(identity);
        self
    }

    pub fn add_injector<I: ContextInjector + 'static>(self, injector: I) -> Self {
        self.with_injector(Arc::new(injector))
    }

    pub fn with_injector(mut self, injector: Arc<dyn ContextInjector>) -> Self {
        self.injectors.push(injector);
        self
    }

    pub fn context_injector<I: ContextInjector + 'static>(self, injector: I) -> Self {
        self.with_injector(Arc::new(injector))
    }

    pub fn with_context_injector<I: ContextInjector + 'static>(self, injector: I) -> Self {
        self.with_injector(Arc::new(injector))
    }

    pub fn with_injectors<I: ContextInjector + 'static>(mut self, injectors: Vec<I>) -> Self {
        self.injectors
            .extend(injectors.into_iter().map(|i| Arc::new(i) as _));
        self
    }

    pub fn tool<T: crate::skills::tool::Tool + 'static>(self, tool: T) -> Self {
        self.with_tool(tool)
    }

    pub fn with_tool<T: crate::skills::tool::Tool + 'static>(self, tool: T) -> Self {
        self.with_tool_catalog(
            tool,
            ToolCatalogOverride {
                source: Some("builtin".to_string()),
                scope: Some("agent".to_string()),
                capability_domain: None,
                tags: vec!["builtin".to_string()],
            },
        )
    }

    pub fn tool_with_catalog<T: crate::skills::tool::Tool + 'static>(
        self,
        tool: T,
        catalog_override: ToolCatalogOverride,
    ) -> Self {
        self.with_tool_catalog(tool, catalog_override)
    }

    pub fn with_tool_catalog<T: crate::skills::tool::Tool + 'static>(
        self,
        tool: T,
        catalog_override: ToolCatalogOverride,
    ) -> Self {
        let name = tool.name();
        self.tools.add(tool);
        self.tools.annotate_catalog_entry(name, catalog_override);
        self
    }

    pub fn shared_tool(self, tool: Arc<dyn crate::skills::tool::Tool>) -> Self {
        self.with_shared_tool(tool)
    }

    pub fn with_shared_tool(self, tool: Arc<dyn crate::skills::tool::Tool>) -> Self {
        self.tools.add_shared(tool);
        self
    }

    pub fn shared_tool_with_catalog(
        self,
        tool: Arc<dyn crate::skills::tool::Tool>,
        catalog_override: ToolCatalogOverride,
    ) -> Self {
        self.with_shared_tool_catalog(tool, catalog_override)
    }

    pub fn with_shared_tool_catalog(
        self,
        tool: Arc<dyn crate::skills::tool::Tool>,
        catalog_override: ToolCatalogOverride,
    ) -> Self {
        self.tools.add_shared_with_catalog(tool, catalog_override);
        self
    }

    pub fn annotate_tool_catalog(
        self,
        name: impl Into<String>,
        catalog_override: ToolCatalogOverride,
    ) -> Self {
        self.tools.annotate_catalog_entry(name, catalog_override);
        self
    }

    pub fn evolution_manager(self, manager: Arc<EvolutionManager>) -> Self {
        self.with_evolution_manager(manager)
    }

    pub fn with_evolution_manager(mut self, manager: Arc<EvolutionManager>) -> Self {
        self.evolution_manager = Some(manager);
        self
    }

    pub fn with_sensor(
        mut self,
        sensor: Arc<parking_lot::RwLock<crate::infra::CapabilitySensor>>,
    ) -> Self {
        self.sensor = Some(sensor);
        self
    }

    pub fn with_complexity_estimator(
        mut self,
        estimator: Arc<dyn crate::agent::meta::ComplexityEstimator>,
    ) -> Self {
        self.complexity_estimator = Some(estimator);
        self
    }

    pub fn with_sensory_hub(mut self, hub: Arc<dyn SensoryLiaison>) -> Self {
        self.sensory_hub = Some(hub);
        self
    }

    pub fn with_tactical_orchestrator(
        mut self,
        orchestrator: Arc<dyn crate::agent::tactical::TacticalOrchestrator>,
    ) -> Self {
        self.tactical_orchestrator = Some(orchestrator);
        self
    }

    /// Phase 16.1: Load a Small Language Model (SLM) from path as a tactical balancer.
    /// If loading fails, it falls back to a passthrough orchestrator.
    pub async fn with_slm(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        let path_buf = path.into();
        match benshu_inference::backend::InferenceFactory::create_backend(&path_buf, None).await {
            Ok(backend) => {
                let model_name = path_buf
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown-slm")
                    .to_string();
                debug!("Tactical SLM loaded successfully: {}", model_name);
                self.tactical_orchestrator = Some(Arc::new(
                    crate::agent::tactical::GlobalTacticalOrchestrator::new(
                        Some(backend),
                        model_name,
                    ),
                ));
            }
            Err(e) => {
                warn!(
                    "Failed to load Tactical SLM from {:?}: {}. Using passthrough mode.",
                    path_buf, e
                );
                self.tactical_orchestrator = Some(Arc::new(
                    crate::agent::tactical::GlobalTacticalOrchestrator::passthrough(),
                ));
            }
        }
        self
    }

    pub fn tools(self, tools: ToolSet) -> Self {
        self.with_tools(tools)
    }

    pub fn with_tools(self, tools: ToolSet) -> Self {
        self.tools.merge_from(&tools);
        self
    }

    pub fn with_memory(mut self, memory: Arc<dyn crate::agent::memory::Memory>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_comm_client(mut self, client: benshu_comm::client::CommClient) -> Self {
        self.comm_client = Some(client);
        self
    }

    #[cfg(feature = "cron")]
    pub fn with_scheduler(self, scheduler: Arc<benshu_scheduler::Scheduler>) -> Self {
        self.tools.add(CronTool::new(Arc::downgrade(&scheduler)));
        self
    }

    pub fn with_security(mut self, security: Arc<dyn SecurityHandler>) -> Self {
        self.security = Some(security);
        if let Some(ctx) = self.governance_context.take() {
            self.governance_context = Some(Arc::new(
                ctx.with_security_handler(self.security.as_ref().unwrap().clone()),
            ));
        }
        self
    }

    pub fn with_trusted_workspaces(mut self, workspaces: Vec<PathBuf>) -> Self {
        self.config.trusted_workspaces = workspaces.clone();
        if let Some(ctx) = self.governance_context.take() {
            self.governance_context = Some(Arc::new(ctx.with_trusted_workspaces(workspaces)));
        }
        self
    }

    pub fn with_initial_risk_score(mut self, risk_score: f32) -> Self {
        self.initial_risk_score = risk_score;
        if let Some(ctx) = self.governance_context.take() {
            self.governance_context = Some(Arc::new(ctx.with_risk_score(risk_score)));
        }
        self
    }

    pub fn with_governance_context(mut self, context: Arc<GovernanceContext>) -> Self {
        self.config.tool_policy = context.tool_policy().clone();
        self.config.trusted_workspaces = context.trusted_workspaces().to_vec();
        self.config.token_budget = context.token_budget();
        self.approval_handler = Some(context.approval_handler());
        self.security = Some(context.security_handler());
        self.initial_risk_score = context.risk_score();
        self.governance_context = Some(context);
        self
    }

    pub fn with_component(
        mut self,
        component: Arc<dyn crate::agent::component::AgentComponent>,
    ) -> Self {
        self.components.push(component);
        self
    }

    pub fn build(mut self) -> Result<Agent<P>> {
        self.config.validate()?;

        if self.security.is_none() {
            return Err(Error::agent_config(
                "SecurityHandler must be provided via with_security()",
            ));
        }

        let (tx, _) = broadcast::channel(1000);

        // Stage 1: Component Registration
        let mut registration = crate::agent::component::ComponentContext::new();
        for component in &self.components {
            component.register(&mut registration)?;
        }

        // Add registered tools to the pool
        for tool in registration.tools {
            self.tools.add_shared(Arc::from(tool));
        }

        // Add registered injectors to the pool
        for injector in registration.injectors {
            self.injectors.push(injector);
        }

        let mut context_config = ContextConfig {
            max_tokens: self.config.max_tokens.unwrap_or(128000) as usize,
            max_history_messages: self.config.max_history_messages,
            response_reserve: self.config.response_reserve,
            enable_cache_control: self.config.enable_cache_control,
            smart_pruning: self.config.smart_pruning,
        };

        if context_config.response_reserve > context_config.max_tokens / 2 {
            context_config.response_reserve = context_config.max_tokens / 2;
        }

        let mut context_manager = ContextManager::new(context_config);

        let final_preamble = if (self.config.agent_path.is_some()
            || self.config.agent_identity.is_some())
            && self.config.preamble == "You are a helpful AI assistant."
        {
            "Follow the specific identity, tone, and mission directives defined in the Agent and AgentIdentity profiles provided below.".to_string()
        } else {
            self.config.preamble.clone()
        };

        context_manager.set_system_prompt(final_preamble);

        if let Some(agent_identity) = &self.config.agent_identity {
            *self.agent_identity.write() = Some(agent_identity.clone());
        }
        context_manager.add_injector(Arc::new(AgentIdentityManager::new(Arc::clone(
            &self.agent_identity,
        ))));

        if let Some(agent_path) = &self.config.agent_path {
            context_manager.add_injector(Arc::new(AgentConfigManager::new(agent_path.clone())));
        }

        if let Some(memory) = &self.memory {
            context_manager.add_injector(Arc::new(
                crate::agent::memory::LearnedMemoryInjector::new(Arc::clone(memory)),
            ));
        }

        for injector in self.injectors {
            context_manager.add_injector(injector);
        }

        let tools = self.tools.with_events(tx.clone(), self.session_id.clone());
        if let Some(handler) = &self.interaction_handler {
            use crate::agent::runtime_support::AskUserTool;
            tools.add(AskUserTool::new(Arc::clone(handler)));
        }

        tools.add(UpdateAgentIdentityTool::new(Arc::clone(
            &self.agent_identity,
        )));

        if let Some(em) = self.evolution_manager.as_ref() {
            if let Some(memory) = self.memory.as_ref() {
                em.try_set_memory(Arc::clone(memory))?;
                tracing::info!("EvolutionManager linked to Memory system");
            } else {
                return Err(Error::AgentConfig(
                    "EvolutionManager requires Memory to be set via with_memory()".to_string(),
                ));
            }
            em.try_set_metabolic_threshold(self.config.metabolic_threshold)?;
            tracing::info!(
                "EvolutionManager linked to Metabolic Threshold ({})",
                self.config.metabolic_threshold
            );
        }

        let provider = Arc::new(self.provider);
        let sleep_consolidator = match (self.evolution_manager.as_ref(), self.memory.as_ref()) {
            (Some(em), Some(mem)) => Some(Arc::new(
                SleepConsolidator::new(Arc::clone(mem), em.auditor())
                    .with_evolution(Arc::clone(em)),
            )),
            (None, Some(mem)) => {
                let audit_provider: Arc<dyn Provider> = provider.clone();
                let auditor = Arc::new(crate::agent::evolution::auditor::Auditor::new(
                    audit_provider,
                    self.config.model.clone(),
                ));
                Some(Arc::new(SleepConsolidator::new(Arc::clone(mem), auditor)))
            }
            _ => None,
        };

        let autopilot = self.autopilot.or_else(|| {
            self.memory.as_ref().map(|mem| {
                Arc::new(crate::agent::evolution::autopilot::Autopilot::new(
                    Arc::clone(mem),
                ))
            })
        });

        let approval_handler = self.approval_handler.unwrap_or_else(|| {
            Arc::new(crate::agent::protocol::DefaultApprovalHandler::new(
                self.config.tool_policy.clone(),
            ))
        });
        let security = self.security.unwrap();
        let governance_context = self.governance_context.unwrap_or_else(|| {
            Arc::new(GovernanceContext::new(
                self.config.tool_policy.clone(),
                approval_handler.clone(),
                self.config.trusted_workspaces.clone(),
                security.clone(),
                self.initial_risk_score,
                self.config.token_budget,
            ))
        });
        let runtime_hook_capture =
            Arc::new(parking_lot::RwLock::new(RuntimeHookCapture::default()));
        let hook_engine = {
            let mut engine = HookEngine::new();
            install_default_runtime_middlewares(&mut engine, runtime_hook_capture.clone());

            if let Some(agent_path) = &self.config.agent_path {
                let hooks_dir = agent_path.join("hooks");
                for hook in discover_hooks(&hooks_dir) {
                    engine.register(hook);
                }
            }

            Arc::new(engine)
        };

        let mut agent = Agent {
            provider,
            tools,
            config: self.config.clone(),
            context_manager,
            events: tx,
            approval_handler,
            comm_client: self.comm_client,
            cache: self.cache,
            notifier: self.notifier,
            memory: self.memory,
            background_envelope: Arc::new(parking_lot::RwLock::new(None)),
            background_runtime_stats: Arc::new(parking_lot::RwLock::new(
                crate::agent::core::BackgroundRuntimeStats::default(),
            )),
            session_id: self.session_id,
            metrics: self.metrics,
            enabled_tools: self.enabled_tools,
            agent_identity: self.agent_identity,
            security,
            lifecycle_token: Arc::new(parking_lot::RwLock::new(
                self.lifecycle_token
                    .unwrap_or_else(tokio_util::sync::CancellationToken::new),
            )),
            current_task_token: Arc::new(parking_lot::RwLock::new(
                self.cancel_token
                    .unwrap_or_else(tokio_util::sync::CancellationToken::new),
            )),
            seen_tools: Arc::new(parking_lot::RwLock::new(std::collections::HashSet::new())),
            evolution_manager: self.evolution_manager,
            sleep_consolidator,
            sensor: self.sensor,
            complexity_estimator: self.complexity_estimator,
            sensory_hub: self.sensory_hub,
            tactical_orchestrator: self.tactical_orchestrator.unwrap_or_else(|| {
                Arc::new(crate::agent::tactical::GlobalTacticalOrchestrator::passthrough())
            }),
            cumulative_usage: Arc::new(parking_lot::RwLock::new(TokenUsage::default())),
            task_runner: Arc::new(crate::runtime::task_runner::TaskRunner::new(
                self.config.max_background_tasks,
            )),
            active_task: self.active_task,
            autopilot,
            fact_checker: self.fact_checker,
            current_intent: Arc::new(parking_lot::RwLock::new(None)),
            all_roles: self.all_roles,
            governance: governance_context,
            hook_engine,
            runtime_hook_refs: Arc::new(parking_lot::RwLock::new(None)),
            runtime_hook_capture,
            runtime_stage_capture: Arc::new(parking_lot::RwLock::new(Vec::new())),
            background_tasks_started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        // Stage 3: Component Linking
        for component in &self.components {
            component.link(&agent.tools, agent.memory.as_ref())?;
        }

        // Phase 15.3: Memory Observability Linkage
        if let Some(emitter) = &self.memory_emitter {
            if let Some(memory) = &agent.memory {
                memory.set_emitter(Arc::clone(emitter));
                tracing::info!("Memory Observability Bus linked via AgentBuilder");
            }
        }

        if let Some(sop) = &agent.config.sop {
            let existing = agent.config.preamble.clone();
            agent.context_manager.set_system_prompt(format!(
                "{}\n\n### Standard Operating Procedure (SOP)\n{}\n",
                existing, sop
            ));
        }

        Ok(agent)
    }
}

#[cfg(test)]
mod tests {
    use super::AgentBuilder;
    use crate::agent::provider::MockProvider;
    use crate::skills::tool::{Tool, ToolCatalogOverride, ToolDefinition, ToolSet};
    use async_trait::async_trait;
    use benshu_infra::agent::SafetyLevel;
    use std::sync::Arc;

    struct InspectTool;

    #[async_trait]
    impl Tool for InspectTool {
        fn name(&self) -> String {
            "inspect_tool".to_string()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name(),
                description: "Inspect current tool registration metadata".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                }),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Green,
                usage_guidelines: None,
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".to_string())
        }
    }

    #[tokio::test]
    async fn builder_tool_defaults_to_builtin_catalog_metadata() {
        let builder = AgentBuilder::new(MockProvider::new("ok")).tool(InspectTool);
        let catalog = builder.get_tools().catalog().await;
        let entry = catalog
            .iter()
            .find(|entry| entry.name == "inspect_tool")
            .expect("inspect_tool should exist");

        assert_eq!(entry.source, "builtin");
        assert_eq!(entry.scope, "agent");
        assert!(entry.tags.iter().any(|tag| tag == "builtin"));
    }

    #[tokio::test]
    async fn builder_with_tools_preserves_catalog_overrides() {
        let tools = ToolSet::new();
        tools.add_shared_with_catalog(
            Arc::new(InspectTool),
            ToolCatalogOverride {
                source: Some("skill".to_string()),
                scope: Some("agent".to_string()),
                capability_domain: Some("runtime_surface".to_string()),
                tags: vec!["skill".to_string(), "runtime_surface".to_string()],
            },
        );

        let builder = AgentBuilder::new(MockProvider::new("ok")).with_tools(tools);
        let catalog = builder.get_tools().catalog().await;
        let entry = catalog
            .iter()
            .find(|entry| entry.name == "inspect_tool")
            .expect("inspect_tool should exist");

        assert_eq!(entry.source, "skill");
        assert_eq!(entry.scope, "agent");
        assert_eq!(entry.capability_domain, "runtime_surface");
        assert!(entry.tags.iter().any(|tag| tag == "skill"));
        assert!(entry.tags.iter().any(|tag| tag == "runtime_surface"));
    }
}
