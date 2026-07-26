use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

use super::registry::KernelRegistryBuilder;
use super::KernelRegistry;
use benshu_brain::agent::memory::{MemoryManager, ShortTermMemory};
use benshu_brain::agent::multi_agent::Coordinator;
use benshu_brain::config::AppConfig;
use benshu_builtin_tools::SkillLoader;
use benshu_engram::{
    EngramMemory, HierarchicalRetriever, HybridSearchConfig, HybridSearchEngine, ModelPool,
};
use benshu_experience_core::ExperienceStore;
use benshu_inference::backend::InferenceFactory;
use benshu_inference::KvEngine;
use benshu_infra::sensor::CapabilitySensor;
use benshu_mcp::manager::McpManager;
use benshu_orchestrator::{ArbitrationStrategy, ResourceArbiter};
use benshu_security::{SecurityConfig, SecurityManager};
use benshu_sensory::audio::UnifiedAudioPlugin;
use benshu_sensory::vision::UnifiedVisionPlugin;
use benshu_sensory::{SensoryConfig, SensoryHub};
use benshu_state::{
    ArtifactManager, RunManager, RuntimeEventManager, SessionManager, SnapshotManager, TaskManager,
};
use benshu_telemetry::{TelemetryLevel, TelemetryManager};

use redb::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub temperature: f32,
    pub tools: Vec<String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub agents: Vec<AgentTemplate>,
}

struct BootProgress {
    stages_completed: Vec<&'static str>,
}

impl BootProgress {
    fn new() -> Self {
        Self {
            stages_completed: Vec::new(),
        }
    }
    fn complete(&mut self, stage: &'static str) {
        self.stages_completed.push(stage);
        info!("✅ Stage completed: {}", stage);
    }
}

pub struct KernelBootstrapper {
    base_dir: PathBuf,
    config: AppConfig,
}

impl KernelBootstrapper {
    pub fn new(base_dir: PathBuf, config: AppConfig) -> Self {
        Self { base_dir, config }
    }

    pub async fn boot(self) -> Result<KernelRegistry> {
        info!(
            "🚀 Booting AgentOS Kernel at {}...",
            self.base_dir.display()
        );
        let mut progress = BootProgress::new();

        // 0. Initialize Backend Providers
        benshu_providers::init_backends();

        // 1. Env Setup (Logs, Workspace)
        let logs_dir = self.base_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).context("Failed to create logs directory")?;
        progress.complete("Environment Setup");

        // 2. Telemetry & Analytics
        let telemetry = Arc::new(TelemetryManager::new(TelemetryLevel::Production));
        telemetry
            .init_global()
            .context("Failed to initialize telemetry")?;
        progress.complete("Telemetry");

        // 3. Resource Management & Sensors
        let sensor = Arc::new(parking_lot::RwLock::new(CapabilitySensor::new()));
        let initial_vram_limit = (self.config.knowledge.model_vram_limit_gb as u64) * 1024;
        let arbiter = Arc::new(ResourceArbiter::new(
            ArbitrationStrategy::Balanced,
            initial_vram_limit,
            Some(sensor.clone()),
        ));
        progress.complete("Resource Management");

        // 4. Durable Storage (System DB)
        let db_path = self.base_dir.join("system.redb");
        let db = Arc::new(
            Database::create(&db_path)
                .with_context(|| format!("Failed to create system database at {:?}", db_path))?,
        );

        // Ensure tables exist
        {
            let write_txn = db.begin_write()?;
            {
                let _ = write_txn.open_table(benshu_state::snapshot::SNAPSHOTS_TABLE)?;
                let _ = write_txn.open_table(benshu_state::task::TASKS_TABLE)?;
                let _ = write_txn.open_table(benshu_state::session::SESSIONS_TABLE)?;
                let _ = write_txn.open_table(benshu_state::artifact::ARTIFACTS_TABLE)?;
                let _ = write_txn.open_table(benshu_state::run::RUNS_TABLE)?;
                let _ = write_txn.open_table(benshu_state::RUNTIME_EVENTS_TABLE)?;
            }
            write_txn.commit()?;
        }

        let state_snapshot = Arc::new(SnapshotManager::new(db.clone()));
        let state_task = Arc::new(TaskManager::new(db.clone()));
        let state_artifact = Arc::new(ArtifactManager::new(db.clone()));
        let state_run = Arc::new(RunManager::new(db.clone()));
        let state_runtime_event = Arc::new(RuntimeEventManager::new(db.clone()));
        let state_session = Arc::new(SessionManager::new(db.clone()));
        let experience_store = Arc::new(
            ExperienceStore::open(self.base_dir.join("experience.redb"))
                .context("Failed to initialize experience store")?,
        );
        progress.complete("Durable Storage");

        // 5. Inference Hub & Knowledge Base
        let model_pool = Arc::new(ModelPool::new(
            self.config.knowledge.model_ram_limit_gb as usize * 1024 * 1024 * 1024,
            self.config.knowledge.model_vram_limit_gb as usize * 1024 * 1024 * 1024,
        ));
        let embed_model = self
            .config
            .effective_global_model_binding("embedding", self.config.knowledge.embed_model.clone());
        let rerank_model = self
            .config
            .effective_global_model_binding("rerank", self.config.knowledge.rerank_model.clone());
        let engram_config = HybridSearchConfig {
            db_path: self.base_dir.join("search").join("engram.db"),
            embed_model,
            rerank_model,
            ..Default::default()
        };
        let search_engine = Arc::new(
            HybridSearchEngine::new(engram_config, Some(model_pool))
                .context("Failed to initialize Engram Search Engine")?,
        );
        let retriever = Arc::new(HierarchicalRetriever::new(search_engine.clone()));
        let kv_engine = Arc::new(parking_lot::RwLock::new(KvEngine::new(Default::default())));
        progress.complete("Inference & Knowledge");

        // 6. Perception (Sensory Hub)
        let sensory_config = SensoryConfig {
            vram_budget: self.config.sensory.vram_budget_mb.unwrap_or(2048) * 1024 * 1024,
            video_frame_buffer_size: self.config.sensory.video_buffer_size.unwrap_or(10),
            ..Default::default()
        };
        let sensory_hub = SensoryHub::new(sensory_config);
        self.register_local_sensory_models(&sensory_hub).await;
        let sensory = Arc::new(sensory_hub);
        progress.complete("Perception");

        // 7. Security & Auth
        let vault_path = self.base_dir.join("vault.redb");
        let vault = Arc::new(
            benshu_auth::Vault::open(&vault_path)
                .with_context(|| format!("Failed to open vault at {:?}", vault_path))?,
        );
        let security = Arc::new(SecurityManager::new(
            SecurityConfig::default(),
            Some(vault.clone()),
        ));
        progress.complete("Security & Auth");

        // 8. Execution (Skills & MCP)
        let skill_path = self.base_dir.join("skills");
        let skill_loader = Arc::new(
            SkillLoader::default_user_skill_path()
                .map(|user_skill_path| {
                    SkillLoader::new(skill_path.clone()).with_user_path(user_skill_path)
                })
                .unwrap_or_else(|| SkillLoader::new(skill_path)),
        );
        let mcp = Arc::new(McpManager::new());
        progress.complete("Execution Layer");

        // 9. Cognitive Core (Coordinator & Memory)
        let memory_path = self.base_dir.join("short_term_memory.redb");
        let hot_memory = Arc::new(ShortTermMemory::new(100, 1000, memory_path).await);
        let engram_memory_api: Arc<dyn benshu_memory_api::Memory> =
            Arc::new(EngramMemory::new(search_engine.clone()));
        let memory = Arc::new(MemoryManager::new_with_shared_engram(
            hot_memory,
            engram_memory_api,
        ));
        let coordinator = Arc::new(Coordinator::new());
        coordinator.set_memory(memory.clone());
        coordinator.session_manager.set(state_session.clone()).ok();

        // Load persisted sessions! (Statelessness Fix Part II)
        if let Err(e) = coordinator.load_sessions().await {
            warn!(
                "⚠️ [State] Failed to restore sessions (non-critical): {}",
                e
            );
        }
        progress.complete("Cognitive Core");

        // 10. Seeding & Agent Skeleton
        let base_agent_path = self
            .config
            .agent_path
            .clone()
            .unwrap_or_else(|| self.base_dir.join("agents"));
        if !base_agent_path.exists() {
            let _ = std::fs::create_dir_all(&base_agent_path);
        }
        self.seed_default_agents(&base_agent_path);
        progress.complete("Agent Seeding");

        // 11. Infrastructure Bus
        let bus = Arc::new(benshu_infra::bus::MessageBus::new(1024));
        let comm_hub = Arc::new(benshu_comm::transport::MemoryHub::new());
        let event_bus = Arc::new(benshu_infra::observable::EventDispatcher::new());

        // 12. Job Scheduling (with Durable Store)
        let cron_path = self.base_dir.join("cron.redb");
        let cron_store = benshu_scheduler::RedbCronStore::new(cron_path.to_str().unwrap())
            .ok()
            .map(|s| Box::new(s) as Box<dyn benshu_scheduler::CronStore>);
        let handler = coordinator.clone() as Arc<dyn benshu_scheduler::JobHandler>;
        let scheduler =
            benshu_scheduler::Scheduler::new(Arc::downgrade(&handler), cron_store).await;
        let _ = scheduler.load_jobs().await;
        progress.complete("Swarm Infrastructure");

        // NLU, FactChecker, etc.
        let nlu = self.setup_nlu(sensor.clone()).await;
        let fact_checker = self.setup_fact_checker(sensor.clone()).await;
        let image_gen = self.setup_image_gen().await;
        let tactical_slm = self.setup_tactical_slm().await;

        Ok(KernelRegistry::build(KernelRegistryBuilder {
            coordinator,
            memory,
            search_engine,
            retriever,
            skill_loader,
            vault,
            kv_engine,
            sensory,
            security,
            mcp,
            scheduler,
            bus,
            comm_hub,
            event_bus,
            state_snapshot,
            state_task,
            state_artifact,
            state_run,
            state_runtime_event,
            state_session,
            telemetry,
            experience_store,
            arbiter,
            nlu,
            fact_checker,
            image_gen,
            tactical_slm,
            base_dir: self.base_dir.clone(),
        }))
    }

    async fn register_local_sensory_models(&self, hub: &SensoryHub) {
        if let Some(stt_model) = self
            .configured_global_binding("speech_to_text", self.config.sensory.stt_model.as_deref())
        {
            match InferenceFactory::create_stt_backend(std::path::Path::new(&stt_model)).await {
                Ok(backend) => {
                    info!("👂 Registered configured STT backend from {}", stt_model);
                    hub.register_audio(Arc::new(UnifiedAudioPlugin::for_stt(
                        "global-stt".to_string(),
                        backend,
                    )));
                }
                Err(error) => {
                    warn!(
                        "⚠️ Failed to initialize configured STT backend from {}: {}",
                        stt_model, error
                    );
                }
            }
        }

        if let Some(tts_model) = self
            .configured_global_binding("text_to_speech", self.config.sensory.tts_model.as_deref())
        {
            match InferenceFactory::create_tts_backend(std::path::Path::new(&tts_model)).await {
                Ok(backend) => {
                    info!("🗣 Registered configured TTS backend from {}", tts_model);
                    hub.register_audio(Arc::new(UnifiedAudioPlugin::for_tts(
                        "global-tts".to_string(),
                        backend,
                    )));
                }
                Err(error) => {
                    warn!(
                        "⚠️ Failed to initialize configured TTS backend from {}: {}",
                        tts_model, error
                    );
                }
            }
        }

        if let Some(ocr_model) =
            self.configured_global_binding("ocr", self.config.sensory.ocr_model.as_deref())
        {
            match InferenceFactory::create_ocr_backend(std::path::Path::new(&ocr_model)).await {
                Ok(backend) => {
                    info!("📄 Registered configured OCR backend from {}", ocr_model);
                    hub.register_vision(Arc::new(UnifiedVisionPlugin::for_ocr(
                        "global-ocr".to_string(),
                        backend,
                    )));
                }
                Err(error) => {
                    warn!(
                        "⚠️ Failed to initialize configured OCR backend from {}: {}",
                        ocr_model, error
                    );
                }
            }
        }

        if self.config.sensory.enable_local_vision {
            warn!(
                "⚠️ sensory.enable_local_vision is set, but the WSL in-process llama.cpp local vision runtime has been removed. Visual understanding must now route through provider/bridge-backed multimodal runtimes."
            );
        }
    }

    fn configured_global_binding(
        &self,
        role: &str,
        configured_model: Option<&str>,
    ) -> Option<String> {
        let trimmed = configured_model.unwrap_or_default().trim();
        if trimmed.is_empty() {
            return None;
        }

        let effective = self.config.effective_global_model_binding(role, trimmed);
        let effective = effective.trim();
        if effective.is_empty() {
            None
        } else {
            Some(effective.to_string())
        }
    }

    fn seed_default_agents(&self, base_agent_path: &std::path::Path) {
        let default_config = self.get_default_agents();
        for p in &default_config.agents {
            let role_dir = base_agent_path.join(&p.name);
            if !role_dir.exists() {
                let _ = std::fs::create_dir_all(&role_dir);
            }
            let agent_file = role_dir.join("AGENT.md");
            if !agent_file.exists() {
                // If the body already contains a '---' block at the top, use it as is
                let content = if p.body.trim_start().starts_with("---") {
                    p.body.clone()
                } else {
                    let tools_yaml = p
                        .tools
                        .iter()
                        .map(|t| format!("  - {}", t))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "---\nname: {}\ntemperature: {}\ntools:\n{}\n---\n{}",
                        p.name, p.temperature, tools_yaml, p.body
                    )
                };
                let _ = std::fs::write(&agent_file, content);
            }
        }
    }

    fn get_default_agents(&self) -> AgentConfig {
        AgentConfig {
            agents: vec![
                AgentTemplate {
                    name: "benshu".to_string(),
                    provider: String::new(),
                    model: String::new(),
                    temperature: 0.5,
                    tools: vec![], // Not used when body is complete
                    body: r#"---
base_url: null
temperature: 0.5
tools:
- fs
- git
- shell
- web_search
- knowledge
- forge
- visual
- notify
auto_consolidation: true
traits:
  openness: 9.0
  conscientiousness: 10.0
  extraversion: 6.0
  agreeableness: 8.0
  neuroticism: 1.0
name: BenShu
description: The Jarvis of your system. Expert at orchestrating specialist workers via A2A while maintaining system-wide memory purity.
tone: Calm, efficient, and strategically supportive (Jarvis model).
constraints: null
backstory: The central intelligence of the BenShu AgentOS. Your primary mission is to protect the user's focus by orchestrating specialized agents through A2A. You act as the single point of contact, maintaining a clean memory structure and delegating raw execution to specialists.
---

## Role
Grand Butler & Orchestrator (BenShu Core). You are the primary interface between the user and the AgentOS specialist network. Your job is not to do everything yourself, but to ensure everything is done correctly by the right specialist.

## AgentIdentity
You are the frontstage coordinator. You think in tasks, delegations, and verifications. You maintain the highest standard of system integrity and security. When a request comes in, you first determine if it's a "Butler-level" task (coordination/status) or a "Specialist-level" task (deep technical work). You proactively delegate the latter to minimize your own memory entropy and maximize system precision.

## Core Tenets
- **Memory Purity** — Do not clutter your own context with raw technical logs. Summarize specialist results and store only the "Golden Knowledge."
- **Task Delegation (A2A)** — If a specialist such as `coder`, `writer`, or `researcher` can do it better, use built-in A2A delegation to assign them the task.
- **Verification First** — Never output a specialist's result without checking it for alignment with the user's original goal.
- **Secure Governance** — You are the final shield. Sanitize inputs and verify outputs before any destructive shell command or external broadcast.
- **Proactive Anticipation** — Use your position as the hub to identify dependencies and bottlenecks before the user notices them.

## Delegation Framework (A2A Protocol)
1. **Analyze**: Decompose the user request into atomic sub-tasks.
2. **Scan**: Identify suitable specialists via runtime role and capability signals.
3. **Dispatch**: Use `TaskRequest` to assign work to the matching specialist, such as `coder`, `writer`, `researcher`, or `commander`.
4. **Monitor**: Track `TaskResult` and handle errors or re-dispatch if necessary.
5. **Synthesis**: Combine specialist outputs into a polished, high-level executive summary for the user.

## Communication Style
- Precise, loyal, and slightly formal but accessible.
- Do not narrate internal topology unless the user explicitly asks.
- Provide status updates for complex, multi-agent operations.
- Always offer a high-level summary before diving into technical details (if requested).

## Capacity Authorization
- **A2A Coordinator**: Use built-in delegation, handover, shared board, and broadcast capabilities to manage specialist coordination.
- **System Guard**: Monitor `fs` and `shell` operations with high paranoia.
- **Registry**: Maintain the `knowledge` graph of the entire project/system state.
"#.to_string(),
                },
                AgentTemplate {
                    name: "coder".to_string(),
                    provider: String::new(),
                    model: String::new(),
                    temperature: 0.2,
                    tools: vec![],
                    body: r#"---
temperature: 0.2
tools:
- fs
- git
- shell
---

## Role
Senior Software Engineer (Coder). You are the hands-on specialist for implementation, debugging and architecture design.

## AgentIdentity
- **Precision First**: Write clear, idiomatic, and documented code.
- **TDD Mindset**: Ensure changes are testable.
- **Best Practices**: Follow project-specific conventions and established design patterns.
- **Documentation**: Always explain your changes and any trade-offs you made.

## Guidelines
1. Analyze existing code before making changes.
2. Use tools to verify file contents before editing.
3. Provide multi-replacement chunks for non-contiguous edits.
4. Explain the rationale for complex refactors.
5. Do not handle written artifacts such as articles, fiction, papers, essays, or reports; route those to `writer`.
"#.to_string(),
                },
                AgentTemplate {
                    name: "writer".to_string(),
                    provider: String::new(),
                    model: String::new(),
                    temperature: 0.6,
                    tools: vec![],
                    body: r#"---
temperature: 0.6
tools:
- writing
artifact_policy:
  handles:
    - artifact: written_document
      intents: [draft, revise, export, summarize_sources, structured_writing, longform_writing]
      triggers: [文章, 小说, 论文, 作文, 报告, 文稿, 长文, essay, article, paper, novel, story, report, draft]
      tools: [writing]
      quality_contract:
        require_title: true
        require_stable_ledger_for_multi_step: true
        require_audit_before_export_for_multi_step: true
    - artifact: longform_fiction
      intents: [plan, compose, architect, draft, audit, revise, export]
      triggers: [小说, 故事, 章节, 连载, fiction, novel, story, chapter]
      tools: [writing]
---

## Role
Written Artifact Specialist (Writer). You are responsible for articles, fiction, papers, essays, reports, and long-form documents.

## Guidelines
1. Use evidence and knowledge receipts when the task depends on retrieved or stored knowledge.
2. Use the `writing` tool package for structured document contracts, ledgers, planning, continuity, drafting, audit, revision, export, knowledge grounding, and local file persistence.
3. For articles, papers, essays, reports, and other complex non-code documents, keep title, structure, stable terms, claims, evidence references, and revision state in the document ledger.
4. Preserve document identity, title, entities, argument, and continuity across checkpoints.
5. Do not implement, debug, or modify code; route software engineering tasks to `coder`.
"#.to_string(),
                },
            ],
        }
    }

    async fn setup_nlu(
        &self,
        sensor: Arc<parking_lot::RwLock<dyn benshu_infra::traits::ResourceSensor>>,
    ) -> Arc<dyn benshu_infra::traits::nlu::NluEngine> {
        use benshu_inference::backend::nlu::NluCluster;
        use benshu_inference::backend::InferenceFactory;
        let nlu_base = self.base_dir.join("models").join("nlu");
        let optimal_backend = InferenceFactory::create_nlu_backend(&nlu_base.join("optimal"))
            .await
            .ok();
        let cold_backend = InferenceFactory::create_nlu_backend(&nlu_base.join("cold"))
            .await
            .ok();
        let llm_backend: Option<Arc<dyn benshu_inference::backend::ModelBackend>> = None;
        Arc::new(NluCluster::new(
            optimal_backend,
            cold_backend,
            llm_backend,
            sensor,
        ))
    }

    async fn setup_fact_checker(
        &self,
        sensor: Arc<parking_lot::RwLock<dyn benshu_infra::traits::ResourceSensor>>,
    ) -> Arc<dyn benshu_infra::traits::validation::FactChecker> {
        use benshu_inference::backend::validation::FactCheckCluster;
        let local_backend = match self.configured_global_binding(
            "fact_check",
            self.config.sensory.fact_check_model.as_deref(),
        ) {
            Some(selected_model) => {
                InferenceFactory::create_fact_checker_backend(std::path::Path::new(&selected_model))
                    .await
                    .ok()
            }
            None => None,
        };
        let llm_backend: Option<Arc<dyn benshu_inference::backend::ModelBackend>> = None;
        Arc::new(FactCheckCluster::new(local_backend, llm_backend, sensor))
    }

    async fn setup_image_gen(&self) -> Arc<dyn benshu_inference::backend::ImageGenBackend> {
        let Some(model_id) = self.configured_global_binding(
            "image_generation",
            self.config.sensory.image_gen_model.as_deref(),
        ) else {
            return Arc::new(benshu_inference::backend::NullImageGenBackend);
        };
        let path = std::path::PathBuf::from(model_id.clone());
        match InferenceFactory::create_image_gen_backend(&path).await {
            Ok(backend) => backend,
            Err(e) => {
                warn!(
                    "⚠️ [Creative] Failed to initialize ImageGen ({}): {}. Using Null backend.",
                    model_id, e
                );
                Arc::new(benshu_inference::backend::NullImageGenBackend)
            }
        }
    }
    async fn setup_tactical_slm(&self) -> Option<Arc<dyn benshu_inference::backend::ModelBackend>> {
        use benshu_inference::backend::BackendCapability;
        let model_id = self.configured_global_binding(
            "slm_tactical",
            self.config.sensory.tactical_model.as_deref(),
        )?;
        let path = std::path::PathBuf::from(model_id.clone());
        if let Ok(binding) = InferenceFactory::describe_binding(&path, None, BackendCapability::LLM)
        {
            tracing::info!(
                "🧭 [Tactical] Loading tactical text model via factory={} source={:?} roles={:?}",
                binding.factory_id,
                binding.source,
                binding.declared_roles
            );
        }
        match InferenceFactory::create_backend(&path, None).await {
            Ok(backend) => Some(backend),
            Err(e) => {
                warn!(
                    "⚠️ [Tactical] Failed to load SLM ({}): {}. Falling back to passthrough.",
                    model_id, e
                );
                None
            }
        }
    }
}
