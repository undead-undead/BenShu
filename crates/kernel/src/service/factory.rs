use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{collections::HashMap, sync::Weak};
use tracing::{error, info, warn};

use benshu_inference::backend::{BackendCapability, InferenceFactory};

use benshu_brain::agent::agent_identity::AgentIdentity;
use benshu_brain::agent::multi_agent::{AgentRole, WorkerBlueprint, WorkerSpawner};
use benshu_brain::agent::provider::{CircuitBreakerConfig, Provider, ResilientProvider};
use benshu_brain::agent::Agent;
use benshu_brain::config::vault::{KeyringVault, SecretVault};
use benshu_brain::config::{AgentConfigOverrides, AppConfig};
use benshu_brain::env::EnvManager;
use benshu_brain::skills::tool::ToolCatalogOverride;
use benshu_providers;

use benshu_brain::agent::namespaced_memory::NamespacedMemory;
use benshu_builtin_tools::tool::filesystem::{
    EditFileTool, ListDirTool, ReadFileTool, WriteFileTool,
};
use benshu_builtin_tools::tool::{
    ChartTool, CipherTool, CommandExecTool, CronTool, DataTransformTool, DelegateTool,
    DesktopSenseTool, DocumentUnderstandTool, ExtractAudioTrackTool, ExtractVideoFramesTool,
    FactManagementTool, FetchDocumentTool, FxLookupTool, GitOpsTool, HandoverTool,
    KnowledgeImportUrlTool, KnowledgeManageDocumentTool, LatestInfoLookupTool, MailerTool,
    MultiAgentAuditTool, MultimodalMemoryTool, NormalizeAudioTool, NotifierTool, NovelStudioTool,
    OfficeParseTool, PdfParseTool, PriceLookupTool, ProbeMediaTool, RefineSkill, RememberThisTool,
    RenderVideoThumbnailTool, RuntimeSurfaceTool, SearchHistoryTool, SharedBoardTool,
    SkillManagerTool, SpeakTool, SwarmBroadcastTool, SystemMonitorTool, TextExtractTool,
    TieredSearchTool, TranscribeTool, WeatherLookupTool, WebSearchTool, WindowsControlTool,
    WritingStudioTool,
};
use benshu_engram::KnowledgeSearchTool;
use benshu_infra::CapabilitySensor;

use benshu_builtin_tools::tool::forge::ForgeDynamicThresholds;

use crate::registry::KernelRegistry;

/// Factory for creating agents based on agent configurations, powered by Kernel Registry
pub struct AgentFactory {
    pub kernel: Arc<KernelRegistry>,
    pub app_config: Arc<parking_lot::RwLock<AppConfig>>,
    pub enabled_tools: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    pub evolution_manager:
        Option<Arc<benshu_brain::agent::evolution::evolution_manager::EvolutionManager>>,
    pub uv_env_cache:
        Arc<parking_lot::RwLock<std::collections::HashMap<String, (PathBuf, std::time::Instant)>>>,
    pub shared_provider_pool: Arc<parking_lot::RwLock<HashMap<String, Weak<dyn Provider>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedRuntimeBudgetProfile {
    max_tokens: u64,
    response_reserve: usize,
    token_budget: u32,
    jit_token_budget: u32,
}

impl AgentFactory {
    const LOCAL_SPECIALIST_RESPONSE_RESERVE_CAP: usize = 2048;
    const REMOTE_RUNTIME_OVERHEAD_VRAM_MB: u64 = 256;
    const LOCAL_RUNTIME_OVERHEAD_VRAM_MB: u64 = 1024;

    fn normalized_role_name(role_name: &str) -> String {
        role_name.to_lowercase()
    }

    fn role_from_name(role_name: &str) -> AgentRole {
        match role_name {
            "benshu" => AgentRole::Custom("benshu".to_string()),
            "researcher" => AgentRole::Researcher,
            "trader" => AgentRole::Trader,
            "risk_analyst" => AgentRole::RiskAnalyst,
            "strategist" => AgentRole::Strategist,
            _ => AgentRole::Custom(role_name.to_string()),
        }
    }

    fn role_agent_path(&self, normalized_role_name: &str) -> PathBuf {
        self.app_config
            .read()
            .agent_path
            .clone()
            .unwrap_or_else(|| self.kernel.base_dir().join("agents"))
            .join(normalized_role_name)
    }

    fn is_primary_role_with(primary_role: &AgentRole, role: &AgentRole) -> bool {
        role == primary_role
    }

    fn is_primary_role(&self, role: &AgentRole) -> bool {
        Self::is_primary_role_with(&self.kernel.coordinator().primary_role(), role)
    }

    fn use_full_coordination_toolkit_for(primary_role: &AgentRole, role: &AgentRole) -> bool {
        Self::is_primary_role_with(primary_role, role)
    }

    fn use_full_coordination_toolkit(&self, role: &AgentRole) -> bool {
        Self::use_full_coordination_toolkit_for(&self.kernel.coordinator().primary_role(), role)
    }

    fn auto_add_structured_realtime_lookup_tools_for(
        primary_role: &AgentRole,
        role: &AgentRole,
    ) -> bool {
        Self::is_primary_role_with(primary_role, role)
    }

    fn auto_add_structured_realtime_lookup_tools(&self, role: &AgentRole) -> bool {
        Self::auto_add_structured_realtime_lookup_tools_for(
            &self.kernel.coordinator().primary_role(),
            role,
        )
    }

    fn auto_add_tool_discovery_surface_for(primary_role: &AgentRole, role: &AgentRole) -> bool {
        Self::is_primary_role_with(primary_role, role)
    }

    fn auto_add_skill_loading_surface_for(primary_role: &AgentRole, role: &AgentRole) -> bool {
        Self::is_primary_role_with(primary_role, role)
    }

    fn auto_add_skill_loading_surface(&self, role: &AgentRole) -> bool {
        Self::auto_add_skill_loading_surface_for(&self.kernel.coordinator().primary_role(), role)
    }

    fn auto_add_tool_discovery_surface(&self, role: &AgentRole) -> bool {
        Self::auto_add_tool_discovery_surface_for(&self.kernel.coordinator().primary_role(), role)
    }

    fn compiled_agent_name(role: &AgentRole, ovr: &AgentConfigOverrides) -> String {
        ovr.name.clone().unwrap_or_else(|| role.name().to_string())
    }

    fn has_explicit_local_runtime_binding(overrides: &AgentConfigOverrides) -> bool {
        overrides.local_model_artifact.is_some()
            || overrides.local_mmproj_artifact.is_some()
            || overrides.local_runtime_family.is_some()
    }

    fn runtime_inheritance_uses_legacy_local_model_only_binding(
        overrides: &AgentConfigOverrides,
        is_local_path: impl Fn(&str) -> bool,
    ) -> bool {
        let Some(model) = overrides.model.as_deref() else {
            return false;
        };

        overrides.provider.is_none()
            && overrides.base_url.is_none()
            && !Self::has_explicit_local_runtime_binding(overrides)
            && is_local_path(model)
    }

    fn inherit_primary_runtime_for_worker(
        worker_role_name: &str,
        primary_role_name: &str,
        worker_overrides: AgentConfigOverrides,
        primary_runtime: Option<&AgentConfigOverrides>,
        is_local_path: impl Fn(&str) -> bool,
    ) -> AgentConfigOverrides {
        if worker_role_name == primary_role_name {
            return worker_overrides;
        }

        let Some(primary_runtime) = primary_runtime.filter(|ovr| !ovr.is_runtime_empty()) else {
            return worker_overrides;
        };

        let runtime_empty = worker_overrides.is_runtime_empty();
        let legacy_local_model_only =
            Self::runtime_inheritance_uses_legacy_local_model_only_binding(
                &worker_overrides,
                is_local_path,
            );

        if runtime_empty || legacy_local_model_only {
            let mut resolved = worker_overrides;
            resolved.provider = primary_runtime.provider.clone();
            resolved.base_url = primary_runtime.base_url.clone();
            resolved.model = primary_runtime.model.clone();
            resolved.local_model_artifact = primary_runtime.local_model_artifact.clone();
            resolved.local_mmproj_artifact = primary_runtime.local_mmproj_artifact.clone();
            resolved.local_runtime_family = primary_runtime.local_runtime_family.clone();
            return resolved;
        }

        if Self::has_explicit_local_runtime_binding(&worker_overrides) {
            return worker_overrides;
        }

        let mut resolved = worker_overrides;
        if resolved.provider.is_none() {
            resolved.provider = primary_runtime.provider.clone();
        }
        if resolved.base_url.is_none() {
            resolved.base_url = primary_runtime.base_url.clone();
        }
        if resolved.model.is_none() {
            resolved.model = primary_runtime.model.clone();
        }
        resolved
    }

    fn resolve_runtime_overrides_for_role(
        &self,
        role: &AgentRole,
        overrides: AgentConfigOverrides,
    ) -> AgentConfigOverrides {
        let primary_role_name = self.kernel.coordinator().primary_role().name().to_string();
        let worker_role_name = role.name().to_string();
        let primary_runtime = self
            .app_config
            .read()
            .agents
            .get(&primary_role_name)
            .cloned();

        let runtime_empty = overrides.is_runtime_empty();
        let legacy_local_model_only =
            Self::runtime_inheritance_uses_legacy_local_model_only_binding(&overrides, |model| {
                self.is_local_path(model)
            });
        let needs_partial_defaults = !runtime_empty
            && !legacy_local_model_only
            && !Self::has_explicit_local_runtime_binding(&overrides)
            && (overrides.provider.is_none()
                || overrides.base_url.is_none()
                || overrides.model.is_none());

        let inherited = Self::inherit_primary_runtime_for_worker(
            &worker_role_name,
            &primary_role_name,
            overrides.clone(),
            primary_runtime.as_ref(),
            |model| self.is_local_path(model),
        );

        if runtime_empty || legacy_local_model_only || needs_partial_defaults {
            let reason = if runtime_empty {
                "worker runtime was empty"
            } else if legacy_local_model_only {
                "worker runtime only carried a legacy local model path"
            } else {
                "worker runtime was partial and needed prime binding defaults"
            };
            info!(
                "Inheriting prime runtime binding for worker {} because {}",
                worker_role_name, reason
            );
        }

        inherited
    }

    fn configured_tool_catalog_override(tool_name: &str) -> Option<ToolCatalogOverride> {
        match tool_name {
            "git" => Some(ToolCatalogOverride {
                source: Some("builtin".to_string()),
                scope: Some("agent".to_string()),
                capability_domain: Some("external_cli_tools".to_string()),
                tags: vec![
                    "external_cli_tools".to_string(),
                    "cli".to_string(),
                    "git".to_string(),
                ],
            }),
            _ => None,
        }
    }

    fn tactical_slm_extra_params(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        let model_id = self.app_config.read().sensory.tactical_model.clone()?;
        if model_id.trim().is_empty() {
            return None;
        }
        let binding = InferenceFactory::describe_binding(
            &PathBuf::from(&model_id),
            None,
            BackendCapability::LLM,
        )
        .ok()?;

        let mut params = serde_json::Map::new();
        params.insert(
            "tactical_slm_model_id".to_string(),
            serde_json::Value::String(model_id),
        );
        params.insert(
            "tactical_slm_factory_id".to_string(),
            serde_json::Value::String(binding.factory_id),
        );
        params.insert(
            "tactical_slm_source".to_string(),
            serde_json::Value::String(binding.source.as_str().to_string()),
        );
        params.insert(
            "tactical_slm_roles".to_string(),
            serde_json::Value::String(
                binding
                    .declared_roles
                    .into_iter()
                    .map(|role| role.as_str().to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        );
        params.insert(
            "tactical_slm_present".to_string(),
            serde_json::Value::Bool(true),
        );
        Some(params)
    }

    fn compile_agent_identity(
        primary_role: &AgentRole,
        role: &AgentRole,
        ovr: &AgentConfigOverrides,
    ) -> Option<AgentIdentity> {
        if !Self::is_primary_role_with(primary_role, role) {
            return None;
        }

        let has_identity_overrides = ovr.name.is_some()
            || ovr.description.is_some()
            || ovr.tone.is_some()
            || ovr.backstory.is_some()
            || ovr.traits.is_some()
            || ovr.auto_consolidation.is_some()
            || ovr
                .constraints
                .as_ref()
                .is_some_and(|constraints| !constraints.is_empty());

        if !has_identity_overrides {
            return None;
        }

        let role_summary = ovr
            .description
            .clone()
            .unwrap_or_else(|| role.name().to_string());

        Some(AgentIdentity {
            name: ovr.name.clone(),
            role: role_summary,
            traits: ovr.traits.clone().unwrap_or_default(),
            tone: ovr
                .tone
                .clone()
                .unwrap_or_else(|| "Calm, precise, and supportive.".to_string()),
            constraints: ovr.constraints.clone().unwrap_or_default(),
            backstory: ovr.backstory.clone(),
            auto_consolidation: ovr.auto_consolidation.unwrap_or(true),
        })
    }

    fn activate_built_agent<P: Provider + 'static>(agent: Agent<P>) -> Arc<Agent<P>> {
        agent.start_background_tasks();
        Arc::new(agent)
    }

    pub fn new(
        kernel: Arc<KernelRegistry>,
        app_config: Arc<parking_lot::RwLock<AppConfig>>,
        enabled_tools: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
        evolution_manager: Option<
            Arc<benshu_brain::agent::evolution::evolution_manager::EvolutionManager>,
        >,
    ) -> Self {
        Self {
            kernel,
            app_config,
            enabled_tools,
            evolution_manager,
            uv_env_cache: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            shared_provider_pool: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    fn shared_provider_cache_key(
        provider_name: &str,
        model: &str,
        base_url: Option<&str>,
        resilient_enabled: bool,
        fallback_provider_name: Option<&str>,
    ) -> String {
        format!(
            "provider={provider}|model={model}|base_url={base}|resilient={resilient}|fallback={fallback}",
            provider = provider_name.to_lowercase(),
            model = model,
            base = base_url.unwrap_or_default(),
            resilient = resilient_enabled,
            fallback = fallback_provider_name.unwrap_or_default().to_lowercase(),
        )
    }

    fn resolved_local_runtime_context_window(
        &self,
        provider: &Arc<dyn Provider>,
        _model: &str,
    ) -> Option<usize> {
        if !provider.is_local() {
            return None;
        }

        let ctx_size = self.app_config.read().llama_cpp_runtime.ctx_size as usize;
        (ctx_size > 0).then_some(ctx_size)
    }

    fn shared_worker_runtime_budget_profile(
        provider: &Arc<dyn Provider>,
        model: &str,
        runtime_context_window: Option<usize>,
    ) -> SharedRuntimeBudgetProfile {
        let provider_context_window = provider.get_context_window(model).max(1);
        let context_window = runtime_context_window
            .map(|runtime_window| {
                if provider.is_local() {
                    runtime_window.min(provider_context_window).max(1)
                } else {
                    runtime_window.max(1)
                }
            })
            .unwrap_or_else(|| provider_context_window.max(4096));
        let response_reserve = (context_window / 8).clamp(
            1024,
            if provider.is_local() {
                Self::LOCAL_SPECIALIST_RESPONSE_RESERVE_CAP
            } else {
                benshu_brain::agent::protocol::constants::DEFAULT_RESPONSE_RESERVE
            },
        );
        let session_quota = provider.runtime_policy().session_token_quota.max(4096);
        let token_budget = session_quota.min(u32::MAX as usize) as u32;
        let jit_token_budget = token_budget
            .saturating_div(2)
            .max(response_reserve.min(u32::MAX as usize) as u32);

        SharedRuntimeBudgetProfile {
            max_tokens: context_window as u64,
            response_reserve,
            token_budget,
            jit_token_budget,
        }
    }

    fn inferred_provider_from_model(
        model: &str,
        is_local_path: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            return None;
        }

        if is_local_path(trimmed) {
            return Some("native".to_string());
        }

        let api_ref = trimmed.strip_prefix("api:")?;
        let provider = api_ref.split('/').next().unwrap_or_default().trim();
        if provider.is_empty() {
            None
        } else if matches!(
            provider,
            "native" | "local" | "internal" | "gguf" | "candle"
        ) {
            Some("native".to_string())
        } else {
            Some(provider.to_string())
        }
    }

    fn file_size_mb(path: &str) -> Option<u64> {
        let metadata = std::fs::metadata(Path::new(path)).ok()?;
        metadata
            .is_file()
            .then(|| metadata.len().saturating_add(1024 * 1024 - 1) / (1024 * 1024))
    }

    fn estimate_total_layers_from_size_mb(size_mb: u64) -> u32 {
        match size_mb {
            0..=2_500 => 24,
            2_501..=6_000 => 32,
            6_001..=12_000 => 40,
            12_001..=24_000 => 64,
            _ => 80,
        }
    }

    fn estimate_kv_cache_vram_mb(ctx_size: u32) -> u64 {
        match ctx_size {
            0..=4_096 => 512,
            4_097..=8_192 => 1024,
            8_193..=16_384 => 2048,
            16_385..=32_768 => 4096,
            _ => 6144,
        }
    }

    fn estimate_agent_vram_mb(&self, overrides: &AgentConfigOverrides, model: &str) -> u64 {
        let provider_is_external_runtime = overrides
            .base_url
            .as_deref()
            .map(|url| !url.trim().is_empty())
            .unwrap_or(false)
            && overrides
                .provider
                .as_deref()
                .map(|provider| !self.is_local_model(provider))
                .unwrap_or(true);

        if provider_is_external_runtime || model.starts_with("api:") {
            return Self::REMOTE_RUNTIME_OVERHEAD_VRAM_MB;
        }

        let model_path = overrides
            .local_model_artifact
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .or_else(|| self.is_local_path(model).then_some(model));

        let Some(model_path) = model_path else {
            return Self::REMOTE_RUNTIME_OVERHEAD_VRAM_MB;
        };

        let model_mb = Self::file_size_mb(model_path).unwrap_or(0);
        if model_mb == 0 {
            return self
                .app_config
                .read()
                .knowledge
                .model_vram_limit_gb
                .checked_mul(1024)
                .map(u64::from)
                .filter(|budget| *budget > 0)
                .unwrap_or(Self::LOCAL_RUNTIME_OVERHEAD_VRAM_MB);
        }

        let cfg = self.app_config.read().llama_cpp_runtime.clone();
        let total_layers = Self::estimate_total_layers_from_size_mb(model_mb);
        let gpu_layers = cfg.gpu_layers.min(total_layers);
        let weight_vram_mb =
            ((model_mb as f64) * (gpu_layers as f64 / total_layers.max(1) as f64)).ceil() as u64;
        let kv_vram_mb = if cfg.kv_offload {
            Self::estimate_kv_cache_vram_mb(cfg.ctx_size)
        } else {
            0
        };
        let mmproj_mb = overrides
            .local_mmproj_artifact
            .as_deref()
            .and_then(Self::file_size_mb)
            .unwrap_or(0);

        weight_vram_mb
            .saturating_add(mmproj_mb)
            .saturating_add(kv_vram_mb)
            .saturating_add(Self::LOCAL_RUNTIME_OVERHEAD_VRAM_MB)
    }

    fn cached_shared_provider(&self, key: &str) -> Option<Arc<dyn Provider>> {
        self.shared_provider_pool
            .read()
            .get(key)
            .and_then(|provider| provider.upgrade())
    }

    fn remember_shared_provider(&self, key: String, provider: &Arc<dyn Provider>) {
        self.shared_provider_pool
            .write()
            .insert(key, Arc::downgrade(provider));
    }

    pub fn install_worker_spawner(self: &Arc<Self>) {
        self.kernel
            .coordinator()
            .set_worker_spawner(self.clone() as Arc<dyn WorkerSpawner>);
    }

    pub async fn reload_agent(&self, role_name: &str) -> Result<()> {
        let normalized_role_name = Self::normalized_role_name(role_name);
        let role = Self::role_from_name(&normalized_role_name);
        let agent_path = self.role_agent_path(&normalized_role_name);

        let overrides = self.read_agent_overrides(&agent_path);

        {
            let mut cfg = self.app_config.write();
            cfg.agents
                .insert(normalized_role_name.clone(), overrides.clone());
        }

        let agent = self.build_agent(role, agent_path, overrides).await?;
        self.kernel.coordinator().register(agent);
        info!("Reloaded agent for role: {}", normalized_role_name);
        Ok(())
    }

    pub async fn load_worker_blueprint(&self, role_name: &str) -> Result<()> {
        let normalized_role_name = Self::normalized_role_name(role_name);
        let role = Self::role_from_name(&normalized_role_name);
        let agent_path = self.role_agent_path(&normalized_role_name);
        let overrides = self.read_agent_overrides(&agent_path);

        {
            let mut cfg = self.app_config.write();
            cfg.agents
                .insert(normalized_role_name.clone(), overrides.clone());
        }

        let blueprint = WorkerBlueprint {
            role: role.clone(),
            agent_path,
            display_name: Self::compiled_agent_name(&role, &overrides),
            description: overrides.description.clone(),
            tools: overrides.tools.clone().unwrap_or_default(),
            artifact_policy: overrides.artifact_policy.clone(),
        };
        self.kernel.coordinator().unregister_agent(&role);
        self.kernel
            .coordinator()
            .register_worker_blueprint(blueprint);
        info!("Loaded worker blueprint for role: {}", normalized_role_name);
        Ok(())
    }

    pub fn unload_worker_blueprint(&self, role_name: &str) {
        let normalized_role_name = Self::normalized_role_name(role_name);
        let role = Self::role_from_name(&normalized_role_name);
        self.kernel.coordinator().unregister_worker_blueprint(&role);
        info!(
            "Unloaded worker blueprint for role: {}",
            normalized_role_name
        );
    }

    pub async fn spawn_worker(&self, role_name: &str) -> Result<()> {
        let normalized_role_name = Self::normalized_role_name(role_name);
        let role = Self::role_from_name(&normalized_role_name);

        if self.kernel.coordinator().get(&role).is_some() {
            self.kernel.coordinator().touch_worker(&role);
            return Ok(());
        }

        let blueprint = self
            .kernel
            .coordinator()
            .worker_blueprint(&role)
            .ok_or_else(|| {
                anyhow!(
                    "Worker blueprint not found for role '{}'",
                    normalized_role_name
                )
            })?;
        let overrides = self.read_agent_overrides(&blueprint.agent_path);

        let agent = self
            .build_agent(role.clone(), blueprint.agent_path.clone(), overrides)
            .await?;
        self.kernel.coordinator().register(agent);
        self.kernel.coordinator().touch_worker(&role);
        info!("Spawned worker for role: {}", normalized_role_name);
        Ok(())
    }

    fn read_agent_overrides(&self, agent_path: &std::path::Path) -> AgentConfigOverrides {
        let role_name = agent_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let config = self.app_config.read();
        Self::read_agent_overrides_from_path(agent_path, role_name, &config)
    }

    fn read_agent_overrides_from_path(
        agent_path: &std::path::Path,
        role_name: &str,
        app_config: &AppConfig,
    ) -> AgentConfigOverrides {
        let agent_file = agent_path.join("AGENT.md");
        let mut file_overrides = if agent_file.exists() {
            match std::fs::read_to_string(&agent_file) {
                Ok(content) => {
                    let (ovr, _) = AgentConfigOverrides::parse_frontmatter(&content);
                    app_config.apply_hidden_agent_overrides(role_name, ovr)
                }
                Err(_) => AgentConfigOverrides::default(),
            }
        } else {
            AgentConfigOverrides::default()
        };

        if let Some(policy) = Self::read_agent_artifact_policy_file(agent_path) {
            file_overrides.artifact_policy = Some(policy);
        }

        file_overrides
    }

    fn read_agent_artifact_policy_file(agent_path: &std::path::Path) -> Option<serde_json::Value> {
        let path = agent_path.join("artifact_policy.yaml");
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "failed to read worker artifact policy"
                );
                return None;
            }
        };

        match AgentConfigOverrides::parse_artifact_policy_yaml(&content) {
            Ok(policy) => policy,
            Err(error) => {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "failed to parse worker artifact policy"
                );
                None
            }
        }
    }

    pub fn is_local_model(&self, provider_name: &str) -> bool {
        let name = provider_name.to_lowercase();
        let cfg = self.app_config.read();
        name.contains("ollama")
            || name.contains("llama")
            || name.contains("candle")
            || name.contains("local")
            || name.contains("native")
            || cfg
                .providers
                .active_provider
                .as_ref()
                .map(|p| p.to_lowercase().contains("ollama") || p.to_lowercase().contains("candle"))
                .unwrap_or(false)
    }

    pub fn get_forge_thresholds(&self, provider_name: &str) -> ForgeDynamicThresholds {
        if self.is_local_model(provider_name) {
            ForgeDynamicThresholds {
                forge_retry_limit: 4,
                complexity_trigger: 0.5,
                token_min: 16384,
                token_max: None,
                efficiency_trigger_secs: 10,
            }
        } else {
            ForgeDynamicThresholds::default()
        }
    }

    /// Validates the agent configuration before construction (Stage 5 Fix)
    pub fn validate_blueprint(&self, ovr: &AgentConfigOverrides) -> Result<()> {
        if ovr.model.is_none() && self.app_config.read().providers.active_provider.is_none() {
            return Err(anyhow!(
                "Agent configuration must specify a model or have a global active provider"
            ));
        }

        // Check for forbidden tools if any
        if let Some(tools) = &ovr.tools {
            for t in tools {
                if t == "root" || t == "os_exec" {
                    return Err(anyhow!("Tool '{}' is forbidden by security policy", t));
                }
            }
        }
        Ok(())
    }

    async fn build_agent(
        &self,
        role: AgentRole,
        agent_path: PathBuf,
        ovr: AgentConfigOverrides,
    ) -> Result<Arc<Agent<Arc<dyn Provider>>>> {
        let ovr = self.resolve_runtime_overrides_for_role(&role, ovr);

        // 0. Pre-Flight Validation (P5 Fix)
        self.validate_blueprint(&ovr)?;

        // 1. Resource Arbitration (P2 Fix - Anti-OOM)
        use benshu_infra::traits::resource::{
            AllocationRequest, AllocationResponse, ResourceArbiterProvider, ThrottleLevel,
        };

        let model_str = ovr.model.as_deref().unwrap_or("benshu-unconfigured-model");
        let estimated_vram = self.estimate_agent_vram_mb(&ovr, model_str);

        let request = AllocationRequest {
            agent_id: role.name().to_string(),
            role: if role.name() == "benshu" {
                ThrottleLevel::High
            } else {
                ThrottleLevel::Medium
            },
            vram_mb: estimated_vram,
            ram_mb: 1024,
            cpu_cores: Some(1.0),
        };

        match self.kernel.arbiter().request_allocation(request).await {
            AllocationResponse::Granted { .. } => {
                info!("✓ Resource allocation granted for agent: {}", role.name());
            }
            AllocationResponse::Throttled { wait_ms, .. } => {
                warn!(
                    "⚠️ Resource pressure detected, waiting {}ms for agent {}...",
                    wait_ms,
                    role.name()
                );
                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
            }
            AllocationResponse::Denied(reason) => {
                return Err(anyhow!(
                    "Failed to spawn agent {}: Resource Denied - {}",
                    role.name(),
                    reason
                ));
            }
        }

        let (provider_name, api_key) = {
            let app_cfg = self.app_config.read();
            let explicit_provider = ovr
                .provider
                .as_deref()
                .map(str::trim)
                .filter(|provider| !provider.is_empty())
                .map(str::to_string);
            let configured_provider = app_cfg
                .providers
                .active_provider
                .as_deref()
                .map(str::trim)
                .filter(|provider| !provider.is_empty())
                .map(str::to_string);
            let p_name = explicit_provider
                .or(configured_provider)
                .or_else(|| {
                    Self::inferred_provider_from_model(model_str, |model| self.is_local_path(model))
                })
                .ok_or_else(|| {
                    anyhow!(
                        "No runtime provider is configured for agent {}. Select a local model or cloud provider in the panel before starting this agent.",
                        role.name()
                    )
                })?;
            let vault = KeyringVault::new("benshu");
            let a_key = match vault.get(&format!("{}_API_KEY", p_name.to_uppercase())) {
                Ok(Some(key)) => Some(key),
                _ => {
                    let from_cfg = match p_name.as_str() {
                        "openai" => app_cfg.providers.openai_api_key.clone(),
                        "anthropic" => app_cfg.providers.anthropic_api_key.clone(),
                        "gemini" => app_cfg.providers.gemini_api_key.clone(),
                        "deepseek" => app_cfg.providers.deepseek_api_key.clone(),
                        "minimax" => app_cfg.providers.minimax_api_key.clone(),
                        "openrouter" => app_cfg.providers.openrouter_api_key.clone(),
                        "moonshot" => app_cfg.providers.moonshot_api_key.clone(),
                        "doubao" => app_cfg.providers.doubao_api_key.clone(),
                        _ => None,
                    };
                    let env_key = format!("{}_API_KEY", p_name.to_uppercase());
                    let from_env = std::env::var(&env_key).ok();

                    // Phase 10: Masked Key Logging (Security Fix - Anti-Panic)
                    if let Some(ref key) = from_env {
                        let mask = key.get(0..4).unwrap_or(key);
                        info!(
                            "🔑 Loaded API key for {} from environment (masked: {}***)",
                            p_name, mask
                        );
                    }

                    from_cfg.or(from_env)
                }
            };

            // Mandatory Validation for Critical Cloud Providers (Security Fix)
            if ["openai", "anthropic", "gemini"].contains(&p_name.as_str())
                && a_key.is_none()
                && ovr
                    .base_url
                    .as_ref()
                    .map(|url| !url.trim().is_empty())
                    .unwrap_or(false)
                    == false
                && !self.is_local_model(&p_name)
                && !self.is_local_path(model_str)
                && !model_str.starts_with("api:")
            {
                return Err(anyhow!(
                    "Critical API key for {} is missing from Vault/Env/Config",
                    p_name
                ));
            }

            (p_name, a_key)
        };

        let create_provider = |provider_name: &str,
                               base_url: Option<String>,
                               api_key: Option<String>|
         -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Arc<dyn Provider>>> + Send>,
        > {
            let provider_name = provider_name.to_string();
            let kernel = self.kernel.clone();
            let is_local_path = self.is_local_path(model_str);
            let model = model_str.to_string();
            Box::pin(async move {
                if is_local_path || model.starts_with("api:") {
                    let path = std::path::PathBuf::from(model);
                    let backend = InferenceFactory::create_backend(&path, None)
                        .await
                        .map_err(|e| {
                            anyhow!(
                                "Failed to load agent-specific model {}: {}",
                                path.display(),
                                e
                            )
                        })?;
                    Ok(Arc::new(benshu_providers::native::NativeProvider::new(
                        backend,
                        kernel.kv_engine().clone(),
                    )) as Arc<dyn Provider>)
                } else {
                    benshu_providers::create_provider(
                        &provider_name,
                        base_url,
                        api_key,
                        Some(kernel.kv_engine().clone()),
                    )
                    .await
                    .map_err(|e| anyhow!("Failed to create provider '{}': {:?}", provider_name, e))
                }
            })
        };

        let (
            resilient_enabled,
            fallback_provider_name,
            failure_threshold,
            reset_timeout_secs,
            request_timeout_secs,
        ) = {
            let app_cfg = self.app_config.read();
            (
                app_cfg.providers.resilient_enabled,
                app_cfg.providers.fallback_provider.clone(),
                app_cfg.providers.failure_threshold.unwrap_or(3),
                app_cfg.providers.reset_timeout_secs.unwrap_or(60),
                app_cfg.providers.request_timeout_secs.unwrap_or(30),
            )
        };

        let provider_cache_key = Self::shared_provider_cache_key(
            &provider_name,
            model_str,
            ovr.base_url.as_deref(),
            resilient_enabled,
            fallback_provider_name.as_deref(),
        );
        let provider = if let Some(cached) = self.cached_shared_provider(&provider_cache_key) {
            info!(
                "Reusing shared provider/backend for agent {} with key {}",
                role.name(),
                provider_cache_key
            );
            cached
        } else {
            let base_provider: Arc<dyn Provider> = if self.is_local_path(model_str)
                || model_str.starts_with("api:")
            {
                info!(
                    "🚀 Using Unified Inference for Agent (Role={:?}, Model={})",
                    role, model_str
                );
                let path = std::path::PathBuf::from(model_str);
                let backend = InferenceFactory::create_backend(&path, None)
                    .await
                    .map_err(|e| {
                        anyhow!("Failed to load agent-specific model {}: {}", model_str, e)
                    })?;
                Arc::new(benshu_providers::native::NativeProvider::new(
                    backend,
                    self.kernel.kv_engine().clone(),
                ))
            } else {
                benshu_providers::create_provider(
                    &provider_name,
                    ovr.base_url.clone(),
                    api_key.clone(),
                    Some(self.kernel.kv_engine().clone()),
                )
                .await
                .map_err(|e| anyhow!("Failed to create provider '{}': {:?}", provider_name, e))?
            };

            let provider = if resilient_enabled {
                if let Some(fallback_name) = fallback_provider_name.clone() {
                    if fallback_name.to_lowercase() != provider_name.to_lowercase()
                        && !self.is_local_path(model_str)
                        && !model_str.starts_with("api:")
                    {
                        let fallback_api_key = {
                            let vault = KeyringVault::new("benshu");
                            match vault.get(&format!("{}_API_KEY", fallback_name.to_uppercase())) {
                                Ok(Some(key)) => Some(key),
                                _ => {
                                    let env_key =
                                        format!("{}_API_KEY", fallback_name.to_uppercase());
                                    std::env::var(&env_key).ok()
                                }
                            }
                        };
                        let fallback =
                            create_provider(&fallback_name, ovr.base_url.clone(), fallback_api_key)
                                .await?;

                        let config = CircuitBreakerConfig {
                            failure_threshold,
                            reset_timeout: std::time::Duration::from_secs(reset_timeout_secs),
                            request_timeout: std::time::Duration::from_secs(request_timeout_secs),
                        };

                        info!(
                            "🛡️ ResilientProvider enabled for agent {}: primary={} fallback={}",
                            role.name(),
                            provider_name,
                            fallback_name
                        );

                        Arc::new(ResilientProvider::new(
                            base_provider.clone(),
                            fallback,
                            config,
                        )) as Arc<dyn Provider>
                    } else {
                        base_provider
                    }
                } else {
                    base_provider
                }
            } else {
                base_provider
            };

            self.remember_shared_provider(provider_cache_key.clone(), &provider);
            provider
        };

        let provider_locality = if provider.is_local() {
            "local"
        } else {
            "remote"
        };
        let provider_contract_mode = provider.tool_contract_mode();
        let provider_mainline_stability = provider.mainline_stability();
        info!(
            "Provider route for agent {}: provider={} locality={} contract={} stability={}",
            role.name(),
            provider.name(),
            provider_locality,
            provider_contract_mode,
            provider_mainline_stability
        );
        if provider_mainline_stability != "stable" {
            warn!(
                "Provider {} for agent {} is not yet a stable mainline path (contract={}); keeping orchestration on shared tool-capable contract",
                provider.name(),
                role.name(),
                provider_contract_mode
            );
        }

        let compiled_agent_name = Self::compiled_agent_name(&role, &ovr);
        let compiled_identity =
            Self::compile_agent_identity(&self.kernel.coordinator().primary_role(), &role, &ovr);

        let mut builder = Agent::builder(provider.clone())
            .name(compiled_agent_name)
            .role(role.clone())
            .agent_path(agent_path)
            .with_memory(self.kernel.memory().clone())
            .with_security(self.kernel.security().clone())
            .with_fact_checker(self.kernel.fact_checker().clone())
            .with_image_gen(self.kernel.image_gen().clone())
            .with_sensory_hub(self.kernel.sensory().clone());

        let mut extra_params = serde_json::Map::new();
        let runtime_context_window =
            self.resolved_local_runtime_context_window(&provider, model_str);
        let runtime_budget = Self::shared_worker_runtime_budget_profile(
            &provider,
            model_str,
            runtime_context_window,
        );

        builder = builder
            .with_max_tokens(runtime_budget.max_tokens)
            .with_response_reserve(runtime_budget.response_reserve)
            .with_token_budget(runtime_budget.token_budget)
            .with_jit_token_budget(runtime_budget.jit_token_budget);

        extra_params.insert(
            "runtime_context_budget_tokens".to_string(),
            serde_json::Value::Number(runtime_budget.max_tokens.into()),
        );
        extra_params.insert(
            "runtime_response_reserve_tokens".to_string(),
            serde_json::Value::Number((runtime_budget.response_reserve as u64).into()),
        );
        extra_params.insert(
            "runtime_token_budget".to_string(),
            serde_json::Value::Number((runtime_budget.token_budget as u64).into()),
        );
        extra_params.insert(
            "runtime_jit_token_budget".to_string(),
            serde_json::Value::Number((runtime_budget.jit_token_budget as u64).into()),
        );

        if role != self.kernel.coordinator().primary_role() {
            builder = builder
                .with_efficiency_trigger(0)
                .with_status_recap_threshold_steps(12)
                .with_status_recap_threshold_chars(200_000);
            extra_params.insert(
                "shared_backend_provider".to_string(),
                serde_json::Value::String(provider.name().to_string()),
            );
            extra_params.insert(
                "shared_backend_locality".to_string(),
                serde_json::Value::String(provider.runtime_policy().locality.to_string()),
            );
            extra_params.insert(
                "shared_worker_context_budget_tokens".to_string(),
                serde_json::Value::Number(runtime_budget.max_tokens.into()),
            );
            extra_params.insert(
                "shared_worker_response_reserve_tokens".to_string(),
                serde_json::Value::Number((runtime_budget.response_reserve as u64).into()),
            );
            extra_params.insert(
                "shared_worker_token_budget".to_string(),
                serde_json::Value::Number((runtime_budget.token_budget as u64).into()),
            );
            extra_params.insert(
                "shared_worker_jit_token_budget".to_string(),
                serde_json::Value::Number((runtime_budget.jit_token_budget as u64).into()),
            );
        }

        if let Some(identity) = compiled_identity {
            builder = builder.with_agent_identity(identity);
        }

        if let Some(slm) = self.kernel.tactical_slm() {
            let model_info = slm.model_info();
            if let Some(tactical_params) = self.tactical_slm_extra_params() {
                extra_params.extend(tactical_params);
            }
            let orchestrator = benshu_brain::agent::tactical::GlobalTacticalOrchestrator::new(
                Some(slm.clone()),
                model_info,
            );
            builder = builder.with_tactical_orchestrator(Arc::new(
                benshu_brain::agent::tactical::SpeculativeTacticalOrchestrator::new(Arc::new(
                    orchestrator,
                )),
            ));
        }

        if !extra_params.is_empty() {
            builder = builder.with_extra_params(serde_json::Value::Object(extra_params));
        }

        if let Some(em) = &self.evolution_manager {
            builder = builder.evolution_manager(em.fork());
        }

        let agent_resource_sensor = Arc::new(parking_lot::RwLock::new(CapabilitySensor::new()));
        builder = builder.with_sensor(agent_resource_sensor.clone());

        // Phase 10: Capability Proxy Injection (P1 Fix - Anti-Circularity)
        let capability_proxy: Arc<dyn benshu_infra::traits::kernel::KernelCapability> =
            Arc::new(crate::registry::KernelProxy::new(&self.kernel));

        if self.use_full_coordination_toolkit(&role) {
            builder = builder.tool(SharedBoardTool::new(Arc::new(NamespacedMemory::new(
                self.kernel.memory().clone(),
            ))));
        }

        if self.auto_add_skill_loading_surface(&role) {
            builder = builder
                .tool(benshu_builtin_tools::ReadSkillDoc::new(
                    self.kernel.skill_loader().clone(),
                ))
                .tool(benshu_builtin_tools::ReadSkillAsset::new(
                    self.kernel.skill_loader().clone(),
                ))
                .context_injector(self.kernel.skill_loader().clone());
        }

        if self.use_full_coordination_toolkit(&role) {
            builder = builder
                .tool(
                    DelegateTool::with_knowledge_import(
                        Arc::downgrade(self.kernel.coordinator()),
                        self.kernel.search_engine().clone(),
                        self.kernel.memory().clone(),
                    )
                    .with_skill_management(
                        self.kernel.skill_loader().clone(),
                        self.kernel.base_dir().clone(),
                        self.enabled_tools.clone(),
                    )
                    .with_runtime_state(
                        self.kernel.state_task().clone(),
                        self.kernel.state_runtime_event().clone(),
                    ),
                )
                .tool(HandoverTool::new(Arc::downgrade(self.kernel.coordinator())))
                .tool(FactManagementTool::new(self.kernel.memory().clone())) // TODO: Migrate FactManagement
                .tool(MultimodalMemoryTool::new(self.kernel.memory().clone()))
                .tool(SearchHistoryTool::new(capability_proxy.clone()))
                .tool(RememberThisTool::new(capability_proxy.clone()));
        }

        // Phase 4: Infra Fusion - Hybrid Communication Routing (Memory + Bus)
        let addr_str = role.name().to_string();
        let self_addr = benshu_comm::protocol::Address::Agent(addr_str.clone());

        // 1. Memory Transport & Hub registration (for fast local A2A)
        let (mem_transport, mem_tx) =
            benshu_comm::transport::MemoryTransport::new(addr_str.clone(), 1024);
        let hub = self.kernel.comm_hub().clone();
        let _hub_reg_handle = {
            let hub_clone = hub.clone();
            let addr_clone = addr_str.clone();
            tokio::spawn(async move {
                if let Err(e) = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    hub_clone.register(addr_clone.clone(), mem_tx),
                )
                .await
                {
                    tracing::error!(
                        "❌ A2A Hub Registration Timeout for agent: {} ({})",
                        addr_clone,
                        e
                    );
                }
            })
        };

        // 2. Bus Transport (for global swarm events)
        let bus_transport = Arc::new(benshu_comm::transport::BusTransport::new(
            (**self.kernel.bus()).clone(),
            addr_str.clone(),
        ));

        // 3. Dispatcher Fusion
        let dispatcher = Arc::new(benshu_comm::transport::GatewayDispatcher::new(
            Arc::new(mem_transport),
            bus_transport,
            hub,
        ));

        let scheduler = Arc::new(benshu_comm::scheduler::A2AScheduler::new());
        let comm_client = benshu_comm::client::CommClient::new(
            scheduler,
            dispatcher,
            self_addr,
            Some(self.kernel.event_bus().clone()),
        )
        .with_runtime_profile(benshu_comm::client::CommRuntimeProfile::Embedded);

        builder = builder.with_comm_client(comm_client.clone());
        if self.use_full_coordination_toolkit(&role) {
            builder = builder.tool(MultiAgentAuditTool::new(Arc::downgrade(
                self.kernel.coordinator(),
            )));
        }

        let configured_tools = ovr.tools.clone().unwrap_or_default();
        for tool_name in &configured_tools {
            match tool_name.as_str() {
                "fs" => {
                    builder = builder
                        .tool(ReadFileTool::new(self.kernel.base_dir().clone()))
                        .tool(WriteFileTool::new(self.kernel.base_dir().clone()))
                        .tool(ListDirTool::new(self.kernel.base_dir().clone()))
                        .tool(EditFileTool::new(self.kernel.base_dir().clone()));
                }
                "knowledge" => {
                    builder = builder
                        .tool(
                            KnowledgeSearchTool::new(self.kernel.retriever().clone())
                                .with_security_handler(self.kernel.security().clone()),
                        )
                        .tool(TieredSearchTool::new(self.kernel.memory().clone()))
                        .tool(FetchDocumentTool::new(self.kernel.memory().clone()))
                        .tool(KnowledgeImportUrlTool::new(
                            self.kernel.search_engine().clone(),
                        ))
                        .tool(KnowledgeManageDocumentTool::new(
                            self.kernel.search_engine().clone(),
                        ));
                }
                "writing_studio" => {
                    builder = builder
                        .tool(ReadFileTool::new(self.kernel.base_dir().clone()))
                        .tool(WriteFileTool::new(self.kernel.base_dir().clone()))
                        .tool(ListDirTool::new(self.kernel.base_dir().clone()))
                        .tool(EditFileTool::new(self.kernel.base_dir().clone()))
                        .tool(
                            KnowledgeSearchTool::new(self.kernel.retriever().clone())
                                .with_security_handler(self.kernel.security().clone()),
                        )
                        .tool(TieredSearchTool::new(self.kernel.memory().clone()))
                        .tool(FetchDocumentTool::new(self.kernel.memory().clone()))
                        .tool(KnowledgeImportUrlTool::new(
                            self.kernel.search_engine().clone(),
                        ))
                        .tool(KnowledgeManageDocumentTool::new(
                            self.kernel.search_engine().clone(),
                        ))
                        .shared_tool_with_catalog(
                            Arc::new(
                                WritingStudioTool::new(self.kernel.base_dir().clone(), role.name())
                                    .with_artifact_manager(self.kernel.state_artifact().clone()),
                            ),
                            ToolCatalogOverride {
                                source: Some("builtin".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: Some("writing".to_string()),
                                tags: vec![
                                    "writing".to_string(),
                                    "written_document".to_string(),
                                    "artifact".to_string(),
                                    "knowledge_grounded".to_string(),
                                    "anti_drift".to_string(),
                                ],
                            },
                        );
                }
                "writing" => {
                    builder = builder
                        .tool(ReadFileTool::new(self.kernel.base_dir().clone()))
                        .tool(WriteFileTool::new(self.kernel.base_dir().clone()))
                        .tool(ListDirTool::new(self.kernel.base_dir().clone()))
                        .tool(EditFileTool::new(self.kernel.base_dir().clone()))
                        .tool(
                            KnowledgeSearchTool::new(self.kernel.retriever().clone())
                                .with_security_handler(self.kernel.security().clone()),
                        )
                        .tool(TieredSearchTool::new(self.kernel.memory().clone()))
                        .tool(FetchDocumentTool::new(self.kernel.memory().clone()))
                        .tool(KnowledgeImportUrlTool::new(
                            self.kernel.search_engine().clone(),
                        ))
                        .tool(KnowledgeManageDocumentTool::new(
                            self.kernel.search_engine().clone(),
                        ))
                        .shared_tool_with_catalog(
                            Arc::new(
                                WritingStudioTool::new(self.kernel.base_dir().clone(), role.name())
                                    .with_artifact_manager(self.kernel.state_artifact().clone()),
                            ),
                            ToolCatalogOverride {
                                source: Some("builtin".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: Some("writing".to_string()),
                                tags: vec![
                                    "writing".to_string(),
                                    "written_document".to_string(),
                                    "artifact".to_string(),
                                    "knowledge_grounded".to_string(),
                                    "anti_drift".to_string(),
                                ],
                            },
                        )
                        .shared_tool_with_catalog(
                            Arc::new(
                                NovelStudioTool::new(self.kernel.base_dir().clone(), role.name())
                                    .with_artifact_manager(self.kernel.state_artifact().clone()),
                            ),
                            ToolCatalogOverride {
                                source: Some("builtin".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: Some("writing".to_string()),
                                tags: vec![
                                    "writing".to_string(),
                                    "longform_writing".to_string(),
                                    "artifact".to_string(),
                                    "knowledge_grounded".to_string(),
                                    "continuity".to_string(),
                                ],
                            },
                        );
                }
                "git" | "git_ops" => {
                    if let Some(catalog_override) =
                        Self::configured_tool_catalog_override(tool_name)
                    {
                        builder = builder
                            .shared_tool_with_catalog(Arc::new(GitOpsTool), catalog_override);
                    } else {
                        builder = builder.tool(GitOpsTool);
                    }
                }
                "command_exec" => {
                    builder = builder.shared_tool_with_catalog(
                        Arc::new(CommandExecTool::new(self.kernel.base_dir().clone())),
                        ToolCatalogOverride {
                            source: Some("builtin".to_string()),
                            scope: Some("agent".to_string()),
                            capability_domain: Some("runtime_surface".to_string()),
                            tags: vec![
                                "runtime_surface".to_string(),
                                "command_exec".to_string(),
                                "windows_native".to_string(),
                            ],
                        },
                    );
                }
                "windows_control" => {
                    builder = builder.shared_tool_with_catalog(
                        Arc::new(WindowsControlTool::new(self.kernel.base_dir().clone())),
                        ToolCatalogOverride {
                            source: Some("builtin".to_string()),
                            scope: Some("agent".to_string()),
                            capability_domain: Some("windows_native".to_string()),
                            tags: vec![
                                "windows_native".to_string(),
                                "powershell".to_string(),
                                "runtime_surface".to_string(),
                                "host_control".to_string(),
                            ],
                        },
                    );
                }
                "chart" => {
                    builder = builder.tool(
                        ChartTool::new(role.name())
                            .with_artifact_manager(self.kernel.state_artifact().clone()),
                    );
                }
                "mailer" => {
                    builder = builder.tool(MailerTool);
                }
                "data" | "data_transform" => {
                    builder = builder.tool(
                        DataTransformTool::new(role.name())
                            .with_artifact_manager(self.kernel.state_artifact().clone()),
                    );
                }
                "novel" | "novel_studio" => {
                    builder = builder
                        .tool(ReadFileTool::new(self.kernel.base_dir().clone()))
                        .tool(WriteFileTool::new(self.kernel.base_dir().clone()))
                        .tool(ListDirTool::new(self.kernel.base_dir().clone()))
                        .tool(EditFileTool::new(self.kernel.base_dir().clone()))
                        .tool(
                            KnowledgeSearchTool::new(self.kernel.retriever().clone())
                                .with_security_handler(self.kernel.security().clone()),
                        )
                        .tool(TieredSearchTool::new(self.kernel.memory().clone()))
                        .tool(FetchDocumentTool::new(self.kernel.memory().clone()))
                        .tool(KnowledgeImportUrlTool::new(
                            self.kernel.search_engine().clone(),
                        ))
                        .tool(KnowledgeManageDocumentTool::new(
                            self.kernel.search_engine().clone(),
                        ))
                        .shared_tool_with_catalog(
                            Arc::new(
                                NovelStudioTool::new(self.kernel.base_dir().clone(), role.name())
                                    .with_artifact_manager(self.kernel.state_artifact().clone()),
                            ),
                            ToolCatalogOverride {
                                source: Some("builtin".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: Some("longform_writing".to_string()),
                                tags: vec![
                                    "longform_writing".to_string(),
                                    "fiction".to_string(),
                                    "artifact".to_string(),
                                    "continuity".to_string(),
                                ],
                            },
                        );
                }
                "ocr" | "text_extract" => {
                    builder = builder.tool(TextExtractTool::new(
                        Some(Arc::clone(&provider)),
                        ovr.model.clone(),
                        self.kernel.sensory().clone(),
                    ));
                }
                "document" | "document_understand" => {
                    builder = builder.tool(DocumentUnderstandTool::new(
                        Some(Arc::clone(&provider)),
                        ovr.model.clone(),
                        self.kernel.sensory().clone(),
                    ));
                }
                "runtime_surface" | "runtime" => {
                    builder = builder.shared_tool_with_catalog(
                        Arc::new(RuntimeSurfaceTool::new(Arc::new(EnvManager::new(
                            self.kernel.base_dir().join("runtimes"),
                        )))),
                        ToolCatalogOverride {
                            source: Some("builtin".to_string()),
                            scope: Some("agent".to_string()),
                            capability_domain: Some("runtime_surface".to_string()),
                            tags: vec![
                                "runtime_surface".to_string(),
                                "adapter".to_string(),
                                "wrapper".to_string(),
                                "managed".to_string(),
                            ],
                        },
                    );
                }
                "media" | "media_runtime" => {
                    builder = builder
                        .shared_tool_with_catalog(
                            Arc::new(ProbeMediaTool),
                            ToolCatalogOverride {
                                source: Some("builtin".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: Some("media_runtime".to_string()),
                                tags: vec![
                                    "media_runtime".to_string(),
                                    "probe".to_string(),
                                    "ffprobe".to_string(),
                                ],
                            },
                        )
                        .shared_tool_with_catalog(
                            Arc::new(
                                ExtractVideoFramesTool::new(role.name())
                                    .with_artifact_manager(self.kernel.state_artifact().clone()),
                            ),
                            ToolCatalogOverride {
                                source: Some("builtin".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: Some("video_preprocess".to_string()),
                                tags: vec![
                                    "media_runtime".to_string(),
                                    "video_preprocess".to_string(),
                                    "ffmpeg".to_string(),
                                ],
                            },
                        )
                        .shared_tool_with_catalog(
                            Arc::new(
                                RenderVideoThumbnailTool::new(role.name())
                                    .with_artifact_manager(self.kernel.state_artifact().clone()),
                            ),
                            ToolCatalogOverride {
                                source: Some("builtin".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: Some("video_preprocess".to_string()),
                                tags: vec![
                                    "media_runtime".to_string(),
                                    "video_preprocess".to_string(),
                                    "thumbnail".to_string(),
                                ],
                            },
                        )
                        .shared_tool_with_catalog(
                            Arc::new(
                                ExtractAudioTrackTool::new(role.name())
                                    .with_artifact_manager(self.kernel.state_artifact().clone()),
                            ),
                            ToolCatalogOverride {
                                source: Some("builtin".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: Some("audio_preprocess".to_string()),
                                tags: vec![
                                    "media_runtime".to_string(),
                                    "audio_preprocess".to_string(),
                                    "ffmpeg".to_string(),
                                ],
                            },
                        )
                        .shared_tool_with_catalog(
                            Arc::new(
                                NormalizeAudioTool::new(role.name())
                                    .with_artifact_manager(self.kernel.state_artifact().clone()),
                            ),
                            ToolCatalogOverride {
                                source: Some("builtin".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: Some("audio_preprocess".to_string()),
                                tags: vec![
                                    "media_runtime".to_string(),
                                    "audio_preprocess".to_string(),
                                    "normalize".to_string(),
                                ],
                            },
                        );
                }
                "probe_media" => {
                    builder = builder.shared_tool_with_catalog(
                        Arc::new(ProbeMediaTool),
                        ToolCatalogOverride {
                            source: Some("builtin".to_string()),
                            scope: Some("agent".to_string()),
                            capability_domain: Some("media_runtime".to_string()),
                            tags: vec![
                                "media_runtime".to_string(),
                                "probe".to_string(),
                                "ffprobe".to_string(),
                            ],
                        },
                    );
                }
                "extract_video_frames" => {
                    builder = builder.shared_tool_with_catalog(
                        Arc::new(
                            ExtractVideoFramesTool::new(role.name())
                                .with_artifact_manager(self.kernel.state_artifact().clone()),
                        ),
                        ToolCatalogOverride {
                            source: Some("builtin".to_string()),
                            scope: Some("agent".to_string()),
                            capability_domain: Some("video_preprocess".to_string()),
                            tags: vec![
                                "media_runtime".to_string(),
                                "video_preprocess".to_string(),
                                "ffmpeg".to_string(),
                            ],
                        },
                    );
                }
                "render_video_thumbnail" => {
                    builder = builder.shared_tool_with_catalog(
                        Arc::new(
                            RenderVideoThumbnailTool::new(role.name())
                                .with_artifact_manager(self.kernel.state_artifact().clone()),
                        ),
                        ToolCatalogOverride {
                            source: Some("builtin".to_string()),
                            scope: Some("agent".to_string()),
                            capability_domain: Some("video_preprocess".to_string()),
                            tags: vec![
                                "media_runtime".to_string(),
                                "video_preprocess".to_string(),
                                "thumbnail".to_string(),
                            ],
                        },
                    );
                }
                "extract_audio_track" => {
                    builder = builder.shared_tool_with_catalog(
                        Arc::new(
                            ExtractAudioTrackTool::new(role.name())
                                .with_artifact_manager(self.kernel.state_artifact().clone()),
                        ),
                        ToolCatalogOverride {
                            source: Some("builtin".to_string()),
                            scope: Some("agent".to_string()),
                            capability_domain: Some("audio_preprocess".to_string()),
                            tags: vec![
                                "media_runtime".to_string(),
                                "audio_preprocess".to_string(),
                                "ffmpeg".to_string(),
                            ],
                        },
                    );
                }
                "normalize_audio" => {
                    builder = builder.shared_tool_with_catalog(
                        Arc::new(
                            NormalizeAudioTool::new(role.name())
                                .with_artifact_manager(self.kernel.state_artifact().clone()),
                        ),
                        ToolCatalogOverride {
                            source: Some("builtin".to_string()),
                            scope: Some("agent".to_string()),
                            capability_domain: Some("audio_preprocess".to_string()),
                            tags: vec![
                                "media_runtime".to_string(),
                                "audio_preprocess".to_string(),
                                "normalize".to_string(),
                            ],
                        },
                    );
                }
                "pdf_parse" => {
                    builder = builder.tool(PdfParseTool::new(
                        Some(Arc::clone(&provider)),
                        ovr.model.clone(),
                        self.kernel.sensory().clone(),
                    ));
                }
                "office_parse" => {
                    builder = builder.tool(OfficeParseTool);
                }
                "crypto" | "cipher" => {
                    builder = builder.tool(CipherTool);
                }
                "notify" => {
                    builder = builder.tool(NotifierTool);
                }
                "voice" => {
                    let k = api_key
                        .as_deref()
                        .unwrap_or("local-voice-runtime")
                        .to_string();
                    builder = builder
                        .tool(TranscribeTool::new(
                            k.clone(),
                            ovr.base_url.clone(),
                            self.kernel.sensory().clone(),
                        ))
                        .tool(
                            SpeakTool::new(
                                k,
                                ovr.base_url.clone(),
                                self.kernel.base_dir().clone(),
                                self.kernel.sensory().clone(),
                            )
                            .with_artifact_manager(
                                self.kernel.state_artifact().clone(),
                                role.name(),
                            ),
                        );
                }
                "transcribe_audio" => {
                    let k = api_key
                        .as_deref()
                        .unwrap_or("local-voice-runtime")
                        .to_string();
                    builder = builder.tool(TranscribeTool::new(
                        k,
                        ovr.base_url.clone(),
                        self.kernel.sensory().clone(),
                    ));
                }
                "text_to_speech" => {
                    let k = api_key
                        .as_deref()
                        .unwrap_or("local-voice-runtime")
                        .to_string();
                    builder = builder.tool(
                        SpeakTool::new(
                            k,
                            ovr.base_url.clone(),
                            self.kernel.base_dir().clone(),
                            self.kernel.sensory().clone(),
                        )
                        .with_artifact_manager(self.kernel.state_artifact().clone(), role.name()),
                    );
                }
                "visual" | "visual_analysis" => {
                    #[cfg(feature = "browser")]
                    {
                        let browser_tool =
                            Arc::new(benshu_builtin_tools::tool::browser::BrowserTool::new(
                                None,
                                None,
                                self.kernel.sensory().clone(),
                            ));
                        builder = builder.tool(
                            benshu_builtin_tools::tool::visual::VisualAnalysisTool::new(
                                capability_proxy.clone(),
                                self.kernel.sensory().clone(),
                                Some(browser_tool),
                                Some(Arc::clone(&provider)),
                            ),
                        );
                    }
                    #[cfg(not(feature = "browser"))]
                    {
                        builder = builder.tool(
                            benshu_builtin_tools::tool::visual::VisualAnalysisTool::new(
                                capability_proxy.clone(),
                                self.kernel.sensory().clone(),
                                None,
                                Some(Arc::clone(&provider)),
                            ),
                        );
                    }
                }
                "browser" => {
                    builder = builder.tool(benshu_builtin_tools::tool::browser::BrowserTool::new(
                        None,
                        None,
                        self.kernel.sensory().clone(),
                    ));
                }
                "web_search" => {
                    builder = builder
                        .tool(benshu_builtin_tools::tool::browser::BrowserTool::new(
                            None,
                            None,
                            self.kernel.sensory().clone(),
                        ))
                        .tool(
                            benshu_builtin_tools::tool::web_fetch::WebFetchTool::with_defaults()
                                .unwrap(),
                        )
                        .tool(
                            benshu_builtin_tools::tool::web_search::WebSearchTool::from_env()
                                .unwrap(),
                        );
                }
                "web_fetch" => {
                    builder = builder.tool(
                        benshu_builtin_tools::tool::web_fetch::WebFetchTool::with_defaults()
                            .unwrap(),
                    );
                }
                "knowledge_import_url" => {
                    builder = builder.tool(KnowledgeImportUrlTool::new(
                        self.kernel.search_engine().clone(),
                    ));
                }
                "knowledge_manage_document" => {
                    builder = builder.tool(KnowledgeManageDocumentTool::new(
                        self.kernel.search_engine().clone(),
                    ));
                }
                "forge" | "forge_skill" => {
                    let thresholds = self.get_forge_thresholds(&provider_name);
                    let current_tools = builder.get_tools().clone();

                    // Phase 10: Cache Cleanup (Prevent Memory Leak - Capacity + Time)
                    {
                        let mut cache = self.uv_env_cache.write();
                        let now = std::time::Instant::now();
                        // 1. Clean by time
                        cache.retain(|_, (_, ts)| {
                            now.duration_since(*ts) < std::time::Duration::from_hours(1)
                        });
                        // 2. Clean by capacity (Limit to 50 active environments)
                        if cache.len() > 50 {
                            let keys_to_remove: Vec<String> =
                                cache.keys().take(cache.len() - 50).cloned().collect();
                            for k in keys_to_remove {
                                cache.remove(&k);
                            }
                        }
                    }

                    builder = builder.tool(benshu_builtin_tools::tool::forge::ForgeSkill::new(
                        self.kernel.skill_loader().clone(),
                        current_tools,
                        self.kernel.base_dir().join("skills"),
                        None,
                        self.uv_env_cache.clone(),
                        thresholds,
                        Arc::new(parking_lot::RwLock::new(None)),
                        self.is_local_model(&provider_name),
                    ));
                }
                "image_gen" | "generate_image" => {
                    builder =
                        builder.tool(benshu_builtin_tools::tool::image::GenerateImageTool::new(
                            self.kernel.image_gen().clone(),
                            self.kernel.base_dir().join("images"),
                        ));
                }
                "a2a_broadcast" | "swarm_broadcast" => {
                    builder = builder.tool(SwarmBroadcastTool::new(comm_client.clone()));
                }
                "system_monitor" => {
                    builder = builder.tool(SystemMonitorTool::new(agent_resource_sensor.clone()));
                }
                "desktop_sense" => {
                    builder = builder.tool(DesktopSenseTool {
                        sensory: self.kernel.sensory().clone(),
                    });
                }
                "refine_skill" => {
                    builder = builder.tool(RefineSkill::new(self.kernel.skill_loader().clone()));
                }
                "skill_manager" => {
                    builder = builder.tool(SkillManagerTool::new(
                        self.kernel.skill_loader().clone(),
                        self.kernel.base_dir().clone(),
                        self.enabled_tools.clone(),
                        Arc::downgrade(self.kernel.coordinator()),
                    ));
                }
                "cron" | "scheduler" => {
                    builder = builder.tool(CronTool::new(Arc::downgrade(self.kernel.scheduler())));
                }
                _ => {
                    if let Some(skill_entry) = self.kernel.skill_loader().skills.get(tool_name) {
                        builder = builder.shared_tool_with_catalog(
                            Arc::clone(skill_entry.value()) as Arc<dyn benshu_brain::prelude::Tool>,
                            ToolCatalogOverride {
                                source: Some("skill".to_string()),
                                scope: Some("agent".to_string()),
                                capability_domain: None,
                                tags: vec!["skill".to_string(), "per_agent_equipped".to_string()],
                            },
                        );
                    }
                }
            }
        }

        if self.auto_add_structured_realtime_lookup_tools(&role)
            && !builder.get_tools().contains("price_lookup")
        {
            builder = builder.shared_tool_with_catalog(
                Arc::new(PriceLookupTool::new().expect("price lookup tool")),
                ToolCatalogOverride {
                    source: Some("builtin".to_string()),
                    scope: Some("agent".to_string()),
                    capability_domain: Some("realtime_lookup.price".to_string()),
                    tags: vec![
                        "realtime_lookup".to_string(),
                        "price".to_string(),
                        "structured".to_string(),
                    ],
                },
            );
        }

        if self.auto_add_structured_realtime_lookup_tools(&role)
            && !builder.get_tools().contains("fx_lookup")
        {
            builder = builder.shared_tool_with_catalog(
                Arc::new(FxLookupTool::new().expect("fx lookup tool")),
                ToolCatalogOverride {
                    source: Some("builtin".to_string()),
                    scope: Some("agent".to_string()),
                    capability_domain: Some("realtime_lookup.fx".to_string()),
                    tags: vec![
                        "realtime_lookup".to_string(),
                        "fx".to_string(),
                        "structured".to_string(),
                    ],
                },
            );
        }

        if self.auto_add_structured_realtime_lookup_tools(&role)
            && !builder.get_tools().contains("weather_lookup")
        {
            builder = builder.shared_tool_with_catalog(
                Arc::new(WeatherLookupTool::new().expect("weather lookup tool")),
                ToolCatalogOverride {
                    source: Some("builtin".to_string()),
                    scope: Some("agent".to_string()),
                    capability_domain: Some("realtime_lookup.weather".to_string()),
                    tags: vec![
                        "realtime_lookup".to_string(),
                        "weather".to_string(),
                        "structured".to_string(),
                    ],
                },
            );
        }

        if self.auto_add_structured_realtime_lookup_tools(&role)
            && !builder.get_tools().contains("latest_info_lookup")
        {
            builder = builder.shared_tool_with_catalog(
                Arc::new(LatestInfoLookupTool::new().expect("latest info lookup tool")),
                ToolCatalogOverride {
                    source: Some("builtin".to_string()),
                    scope: Some("agent".to_string()),
                    capability_domain: Some("realtime_lookup.latest_info".to_string()),
                    tags: vec![
                        "realtime_lookup".to_string(),
                        "latest_info".to_string(),
                        "structured".to_string(),
                    ],
                },
            );
        }

        if self.auto_add_structured_realtime_lookup_tools(&role)
            && !builder.get_tools().contains("web_search")
        {
            builder = builder.shared_tool_with_catalog(
                Arc::new(WebSearchTool::from_env().expect("web search tool")),
                ToolCatalogOverride {
                    source: Some("builtin".to_string()),
                    scope: Some("agent".to_string()),
                    capability_domain: Some("realtime_lookup.web".to_string()),
                    tags: vec![
                        "realtime_lookup".to_string(),
                        "web_search".to_string(),
                        "structured".to_string(),
                    ],
                },
            );
        }

        if self.auto_add_tool_discovery_surface(&role) && builder.get_tools().len() > 6 {
            if !builder.get_tools().contains("tool_search") {
                let current_tools = builder.get_tools().clone();
                builder = builder.tool(benshu_builtin_tools::tool::ToolSearchTool::new(
                    current_tools,
                ));
            }
            if !builder.get_tools().contains("tool_catalog") {
                let current_tools = builder.get_tools().clone();
                builder = builder.tool(benshu_builtin_tools::tool::ToolCatalogTool::new(
                    current_tools,
                ));
            }
        } else if self.auto_add_tool_discovery_surface(&role)
            && builder.get_tools().contains("tool_search")
            && !builder.get_tools().contains("tool_catalog")
        {
            let current_tools = builder.get_tools().clone();
            builder = builder.tool(benshu_builtin_tools::tool::ToolCatalogTool::new(
                current_tools,
            ));
        }

        builder = builder.model(model_str.to_string());
        let thresholds = self.get_forge_thresholds(&provider_name);
        builder = builder.efficiency_trigger(thresholds.efficiency_trigger_secs);
        if let Some(t) = ovr.temperature {
            builder = builder.temperature(t as f64);
        }

        let agent = builder.build().map_err(anyhow::Error::from)?;
        let agent_arc = Self::activate_built_agent(agent);

        // Phase 10: Launch Runtime Resource Feedback Loop
        let capability_proxy_loop = capability_proxy.clone();
        let agent_weak = Arc::downgrade(&agent_arc);
        let agent_id = role.name().to_string();

        tokio::spawn(async move {
            let mut report_failures = 0;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                if let Some(a) = agent_weak.upgrade() {
                    let actual_vram = a.current_vram_usage().await;
                    capability_proxy_loop
                        .report_usage(&agent_id, actual_vram as usize)
                        .await;
                    report_failures = 0; // Success (it doesn't return Result, so we assume success if it didn't panic)
                } else {
                    break;
                }

                if report_failures >= 5 {
                    error!(
                        "❌ Too many failed VRAM reports for {}, exiting feedback loop for safety",
                        agent_id
                    );
                    break;
                }
            }
        });

        Ok(agent_arc)
    }

    fn is_local_path(&self, model: &str) -> bool {
        let p = Path::new(model);
        p.is_absolute() || model.starts_with("./") || model.starts_with("api:native/")
    }
}

#[async_trait]
impl WorkerSpawner for AgentFactory {
    async fn ensure_worker(&self, role: &AgentRole) -> benshu_brain::error::Result<bool> {
        if self.kernel.coordinator().get(role).is_some() {
            self.kernel.coordinator().touch_worker(role);
            return Ok(true);
        }
        if !self.kernel.coordinator().has_worker_blueprint(role) {
            return Ok(false);
        }
        self.spawn_worker(role.name())
            .await
            .map_err(|err| benshu_brain::error::Error::AgentCoordination(err.to_string()))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::AgentFactory;
    use async_trait::async_trait;
    use benshu_brain::agent::agent_identity::Traits;
    use benshu_brain::agent::multi_agent::AgentRole;
    use benshu_brain::agent::provider::{
        ChatRequest, Provider, ProviderMetadata, ProviderRuntimePolicy,
    };
    use benshu_brain::agent::streaming::StreamingResponse;
    use benshu_brain::config::{AgentConfigOverrides, AppConfig};
    use benshu_infra::error::Result as InfraResult;
    use std::sync::Arc;

    use benshu_brain::agent::core::Agent;
    use benshu_brain::agent::streaming::MockStreamBuilder;
    use benshu_brain::testing::{CommTestEnv, MockSecurityHandler, SequenceMockProvider};

    struct BudgetTestProvider;
    struct SmallContextBudgetTestProvider;

    #[async_trait]
    impl Provider for BudgetTestProvider {
        async fn stream_completion(&self, _request: ChatRequest) -> InfraResult<StreamingResponse> {
            Ok(MockStreamBuilder::new().done().build())
        }

        fn name(&self) -> &str {
            "budget-test"
        }

        fn is_local(&self) -> bool {
            true
        }

        fn runtime_policy(&self) -> ProviderRuntimePolicy {
            ProviderRuntimePolicy {
                locality: "local".to_string(),
                unlocks_full_context_window: true,
                session_token_quota: 320_000,
            }
        }

        fn get_context_window(&self, _model: &str) -> usize {
            131_072
        }

        fn metadata() -> ProviderMetadata
        where
            Self: Sized,
        {
            ProviderMetadata {
                id: "budget-test".to_string(),
                name: "budget-test".to_string(),
                description: "budget test provider".to_string(),
                icon: "🧪".to_string(),
                fields: vec![],
                capabilities: vec![],
                preferred_models: vec![],
            }
        }
    }

    #[async_trait]
    impl Provider for SmallContextBudgetTestProvider {
        async fn stream_completion(&self, _request: ChatRequest) -> InfraResult<StreamingResponse> {
            Ok(MockStreamBuilder::new().done().build())
        }

        fn name(&self) -> &str {
            "small-context-budget-test"
        }

        fn is_local(&self) -> bool {
            true
        }

        fn runtime_policy(&self) -> ProviderRuntimePolicy {
            ProviderRuntimePolicy {
                locality: "local".to_string(),
                unlocks_full_context_window: true,
                session_token_quota: 320_000,
            }
        }

        fn get_context_window(&self, _model: &str) -> usize {
            4_096
        }

        fn metadata() -> ProviderMetadata
        where
            Self: Sized,
        {
            ProviderMetadata {
                id: "small-context-budget-test".to_string(),
                name: "small-context-budget-test".to_string(),
                description: "test".to_string(),
                icon: "x".to_string(),
                fields: vec![],
                capabilities: vec!["runtime:local".to_string()],
                preferred_models: vec![],
            }
        }
    }

    #[tokio::test]
    async fn factory_build_starts_background_tasks_once() {
        let env = CommTestEnv::new();
        let responses = vec![MockStreamBuilder::new().message("ok").done().build()];
        let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));

        let agent = Agent::builder(provider)
            .name("factory-runtime")
            .with_comm_client(env.create_client("factory-runtime"))
            .with_security(Arc::new(MockSecurityHandler))
            .build()
            .expect("agent should build");

        let agent = AgentFactory::activate_built_agent(agent);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let task_count = agent.active_background_tasks();
        assert!(
            task_count > 0,
            "factory activation should start background runtime"
        );

        agent.start_background_tasks();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(
            agent.active_background_tasks(),
            task_count,
            "factory activation should not allow duplicate background runtime startup"
        );

        agent.shutdown().await;
    }

    #[test]
    fn factory_compiles_identity_from_overrides() {
        let overrides = AgentConfigOverrides {
            name: Some("BenShu".to_string()),
            description: Some("Grand Butler & Orchestrator".to_string()),
            tone: Some("Calm, efficient, and supportive".to_string()),
            constraints: Some(vec!["Protect the user's focus.".to_string()]),
            backstory: Some("The central intelligence of the system.".to_string()),
            traits: Some(Traits {
                openness: 9.0,
                conscientiousness: 10.0,
                extraversion: 6.0,
                agreeableness: 8.0,
                neuroticism: 1.0,
            }),
            auto_consolidation: Some(true),
            ..Default::default()
        };

        let identity = AgentFactory::compile_agent_identity(
            &AgentRole::Custom("benshu".to_string()),
            &AgentRole::Custom("benshu".to_string()),
            &overrides,
        )
        .expect("identity should be compiled from overrides");

        assert_eq!(identity.name.as_deref(), Some("BenShu"));
        assert_eq!(identity.role, "Grand Butler & Orchestrator");
        assert_eq!(identity.tone, "Calm, efficient, and supportive");
        assert_eq!(identity.constraints, vec!["Protect the user's focus."]);
        assert_eq!(
            identity.backstory.as_deref(),
            Some("The central intelligence of the system.")
        );
    }

    #[test]
    fn factory_does_not_compile_worker_description_into_identity() {
        let overrides = AgentConfigOverrides {
            name: Some("BenShu Knowledge".to_string()),
            description: Some("Internal knowledge base worker.".to_string()),
            temperature: Some(0.2),
            tools: Some(vec!["knowledge".to_string()]),
            ..Default::default()
        };

        let identity = AgentFactory::compile_agent_identity(
            &AgentRole::Custom("benshu".to_string()),
            &AgentRole::Custom("knowledge".to_string()),
            &overrides,
        );

        assert!(
            identity.is_none(),
            "worker descriptions are routing/profile metadata, not full AgentIdentity"
        );
    }

    #[test]
    fn worker_overrides_read_independent_artifact_policy_yaml() {
        let agent_path =
            std::env::temp_dir().join(format!("benshu-worker-policy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&agent_path).expect("create temp agent dir");
        std::fs::write(
            agent_path.join("AGENT.md"),
            "---\nname: Worker\ntools:\n  - write_file\nartifact_policy:\n  handles:\n    - artifact: stale\n---\n\n# Worker\n",
        )
        .expect("write agent file");
        std::fs::write(
            agent_path.join("artifact_policy.yaml"),
            "artifact_policy:\n  handles:\n    - artifact: research_paper\n      quality_contract:\n        min_chars: 9000\n",
        )
        .expect("write artifact policy");

        let overrides = AgentFactory::read_agent_overrides_from_path(
            &agent_path,
            "writer",
            &AppConfig::default(),
        );

        let policy = overrides.artifact_policy.expect("artifact policy");
        assert_eq!(policy["handles"][0]["artifact"], "research_paper");
        assert_eq!(
            policy["handles"][0]["quality_contract"]["min_chars"],
            serde_json::json!(9000)
        );
        assert_eq!(overrides.tools, Some(vec!["write_file".to_string()]));

        let _ = std::fs::remove_dir_all(agent_path);
    }

    #[test]
    fn shared_provider_cache_key_tracks_provider_model_and_fallback() {
        let key_a = AgentFactory::shared_provider_cache_key(
            "openai",
            "configured-model-a",
            Some("https://api.openai.com"),
            true,
            Some("deepseek"),
        );
        let key_b = AgentFactory::shared_provider_cache_key(
            "openai",
            "configured-model-a",
            Some("https://api.openai.com"),
            true,
            Some("moonshot"),
        );
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn shared_worker_runtime_budget_profile_uses_provider_policy_when_runtime_truth_missing() {
        let provider: Arc<dyn Provider> = Arc::new(BudgetTestProvider);
        let profile = AgentFactory::shared_worker_runtime_budget_profile(
            &provider,
            "api:native/test-model",
            None,
        );

        assert_eq!(profile.max_tokens, 131_072);
        assert_eq!(profile.response_reserve, 2048);
        assert_eq!(profile.token_budget, 320_000);
        assert_eq!(profile.jit_token_budget, 160_000);
    }

    #[test]
    fn inferred_provider_from_local_path_uses_native_runtime() {
        let provider = AgentFactory::inferred_provider_from_model("/models/brain.gguf", |model| {
            model.starts_with('/')
        });

        assert_eq!(provider.as_deref(), Some("native"));
    }

    #[test]
    fn inferred_provider_from_api_reference_uses_uri_provider() {
        let provider = AgentFactory::inferred_provider_from_model("api:qwen/qwen-max", |_| false);

        assert_eq!(provider.as_deref(), Some("qwen"));
    }

    #[test]
    fn shared_worker_runtime_budget_profile_prefers_runtime_context_window_truth() {
        let provider: Arc<dyn Provider> = Arc::new(BudgetTestProvider);
        let profile = AgentFactory::shared_worker_runtime_budget_profile(
            &provider,
            "benshu-main-brain",
            Some(8192),
        );

        assert_eq!(profile.max_tokens, 8192);
        assert_eq!(profile.response_reserve, 1024);
        assert_eq!(profile.token_budget, 320_000);
        assert_eq!(profile.jit_token_budget, 160_000);
    }

    #[test]
    fn shared_worker_runtime_budget_profile_does_not_recap_runtime_truth_to_local_default_cap() {
        let provider: Arc<dyn Provider> = Arc::new(BudgetTestProvider);
        let profile = AgentFactory::shared_worker_runtime_budget_profile(
            &provider,
            "benshu-main-brain",
            Some(16_384),
        );

        assert_eq!(profile.max_tokens, 16_384);
        assert_eq!(profile.response_reserve, 2048);
    }

    #[test]
    fn shared_worker_runtime_budget_profile_never_exceeds_provider_context_window() {
        let provider: Arc<dyn Provider> = Arc::new(SmallContextBudgetTestProvider);
        let profile = AgentFactory::shared_worker_runtime_budget_profile(
            &provider,
            "benshu-main-brain",
            Some(131_072),
        );

        assert_eq!(profile.max_tokens, 4_096);
        assert_eq!(profile.response_reserve, 1024);
    }

    #[test]
    fn configured_tool_catalog_override_marks_git_as_external_cli_tool() {
        let override_hint = AgentFactory::configured_tool_catalog_override("git")
            .expect("git should have explicit catalog override");

        assert_eq!(
            override_hint.capability_domain.as_deref(),
            Some("external_cli_tools")
        );
        assert!(override_hint
            .tags
            .iter()
            .any(|tag| tag == "external_cli_tools"));
        assert!(override_hint.tags.iter().any(|tag| tag == "cli"));
        assert!(override_hint.tags.iter().any(|tag| tag == "git"));
    }

    #[test]
    fn worker_runtime_inherits_prime_runtime_when_worker_runtime_is_empty() {
        let primary = AgentConfigOverrides {
            provider: Some("openai".to_string()),
            base_url: Some("http://127.0.0.1:8013/v1".to_string()),
            model: Some("benshu-main-brain".to_string()),
            local_model_artifact: Some("/models/brain.gguf".to_string()),
            local_runtime_family: Some("llama_cpp".to_string()),
            ..Default::default()
        };

        let resolved = AgentFactory::inherit_primary_runtime_for_worker(
            "researcher",
            "benshu",
            AgentConfigOverrides::default(),
            Some(&primary),
            |model| model.ends_with(".gguf"),
        );

        assert_eq!(resolved.provider.as_deref(), Some("openai"));
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("http://127.0.0.1:8013/v1")
        );
        assert_eq!(resolved.model.as_deref(), Some("benshu-main-brain"));
    }

    #[test]
    fn worker_runtime_replaces_legacy_local_model_only_binding_with_prime_runtime() {
        let primary = AgentConfigOverrides {
            provider: Some("openai".to_string()),
            base_url: Some("http://127.0.0.1:8013/v1".to_string()),
            model: Some("benshu-main-brain".to_string()),
            local_model_artifact: Some("/models/brain.gguf".to_string()),
            local_runtime_family: Some("llama_cpp".to_string()),
            ..Default::default()
        };
        let worker = AgentConfigOverrides {
            model: Some("/models/stale-worker.gguf".to_string()),
            ..Default::default()
        };

        let resolved = AgentFactory::inherit_primary_runtime_for_worker(
            "researcher",
            "benshu",
            worker,
            Some(&primary),
            |model| model.ends_with(".gguf"),
        );

        assert_eq!(resolved.provider.as_deref(), Some("openai"));
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("http://127.0.0.1:8013/v1")
        );
        assert_eq!(resolved.model.as_deref(), Some("benshu-main-brain"));
    }

    #[test]
    fn worker_runtime_keeps_explicit_local_binding_intact() {
        let primary = AgentConfigOverrides {
            provider: Some("openai".to_string()),
            base_url: Some("http://127.0.0.1:8013/v1".to_string()),
            model: Some("benshu-main-brain".to_string()),
            ..Default::default()
        };
        let worker = AgentConfigOverrides {
            model: Some("/models/worker.gguf".to_string()),
            local_model_artifact: Some("/models/worker.gguf".to_string()),
            local_runtime_family: Some("llama_cpp".to_string()),
            ..Default::default()
        };

        let resolved = AgentFactory::inherit_primary_runtime_for_worker(
            "researcher",
            "benshu",
            worker.clone(),
            Some(&primary),
            |model| model.ends_with(".gguf"),
        );

        assert_eq!(resolved.model, worker.model);
        assert_eq!(resolved.local_model_artifact, worker.local_model_artifact);
        assert_eq!(resolved.local_runtime_family, worker.local_runtime_family);
        assert!(resolved.base_url.is_none());
    }

    #[test]
    fn worker_runtime_fills_missing_provider_defaults_for_partial_remote_binding() {
        let primary = AgentConfigOverrides {
            provider: Some("openai".to_string()),
            base_url: Some("http://127.0.0.1:8013/v1".to_string()),
            model: Some("benshu-main-brain".to_string()),
            ..Default::default()
        };
        let worker = AgentConfigOverrides {
            model: Some("research-model".to_string()),
            ..Default::default()
        };

        let resolved = AgentFactory::inherit_primary_runtime_for_worker(
            "researcher",
            "benshu",
            worker,
            Some(&primary),
            |model| model.ends_with(".gguf"),
        );

        assert_eq!(resolved.provider.as_deref(), Some("openai"));
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("http://127.0.0.1:8013/v1")
        );
        assert_eq!(resolved.model.as_deref(), Some("research-model"));
    }

    #[test]
    fn worker_roles_do_not_receive_full_coordination_toolkit_by_default() {
        let primary = AgentRole::Custom("benshu".to_string());

        assert!(AgentFactory::use_full_coordination_toolkit_for(
            &primary,
            &AgentRole::Custom("benshu".to_string())
        ));
        assert!(!AgentFactory::use_full_coordination_toolkit_for(
            &primary,
            &AgentRole::Researcher
        ));
        assert!(!AgentFactory::use_full_coordination_toolkit_for(
            &primary,
            &AgentRole::Custom("knowledge".to_string())
        ));
    }

    #[test]
    fn worker_roles_skip_shared_board_surface_by_default() {
        let primary = AgentRole::Custom("benshu".to_string());

        assert!(AgentFactory::use_full_coordination_toolkit_for(
            &primary,
            &AgentRole::Custom("benshu".to_string())
        ));
        assert!(!AgentFactory::use_full_coordination_toolkit_for(
            &primary,
            &AgentRole::Researcher
        ));
        assert!(!AgentFactory::use_full_coordination_toolkit_for(
            &primary,
            &AgentRole::Custom("knowledge".to_string())
        ));
    }

    #[test]
    fn worker_roles_skip_auto_injected_realtime_lookup_and_tool_discovery_surfaces() {
        let primary = AgentRole::Custom("benshu".to_string());

        assert!(AgentFactory::auto_add_structured_realtime_lookup_tools_for(
            &primary,
            &AgentRole::Custom("benshu".to_string())
        ));
        assert!(AgentFactory::auto_add_tool_discovery_surface_for(
            &primary,
            &AgentRole::Custom("benshu".to_string())
        ));
        assert!(
            !AgentFactory::auto_add_structured_realtime_lookup_tools_for(
                &primary,
                &AgentRole::Researcher
            )
        );
        assert!(!AgentFactory::auto_add_tool_discovery_surface_for(
            &primary,
            &AgentRole::Researcher
        ));
    }

    #[test]
    fn worker_roles_skip_auto_injected_skill_loading_surface() {
        let primary = AgentRole::Custom("benshu".to_string());

        assert!(AgentFactory::auto_add_skill_loading_surface_for(
            &primary,
            &AgentRole::Custom("benshu".to_string())
        ));
        assert!(!AgentFactory::auto_add_skill_loading_surface_for(
            &primary,
            &AgentRole::Researcher
        ));
        assert!(!AgentFactory::auto_add_skill_loading_surface_for(
            &primary,
            &AgentRole::Custom("knowledge".to_string())
        ));
    }
}
