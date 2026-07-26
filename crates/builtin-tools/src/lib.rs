pub mod compiler;
#[cfg(any(feature = "http", feature = "browser"))]
pub(crate) mod net_safety;
// sandbox module moved to 'security' crate
pub mod tool;

use async_trait::async_trait;
use benshu_brain::agent::context::ContextInjector;
use benshu_brain::agent::message::Message;
use benshu_brain::error::Result as BrainResult;
use benshu_infra::error::{Error, Result};
use benshu_infra::skill::SkillFilesystemAccess;
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};
use benshu_runtimes::{SkillExecutionConfig, SkillMetadata, SkillRuntime};
use benshu_security::skill_verifier::{RiskLevel, SkillVerifier};
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::json;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tracing::info;

/// A skill document that may or may not be executable.
pub struct SkillDoc {
    metadata: SkillMetadata,
    instructions: String,
    base_dir: PathBuf,
}

impl SkillDoc {
    pub fn new(metadata: SkillMetadata, instructions: String, base_dir: PathBuf) -> Self {
        Self {
            metadata,
            instructions,
            base_dir,
        }
    }

    pub fn metadata(&self) -> &SkillMetadata {
        &self.metadata
    }

    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn is_executable(&self) -> bool {
        self.metadata.runtime.is_some() && self.metadata.script.is_some()
    }

    pub fn execution_surface(&self) -> &'static str {
        if !self.is_executable() {
            return "none";
        }
        if self.metadata.kind.eq_ignore_ascii_case("agent") {
            "worker"
        } else if self.metadata.runtime.is_some() {
            "runtime"
        } else {
            "tool"
        }
    }

    pub fn available_assets(&self) -> Vec<(String, String)> {
        let mut assets = Vec::new();
        for kind in ["references", "templates", "scripts"] {
            let dir = self.base_dir.join(kind);
            self.collect_assets_from_dir(kind, &dir, &mut assets);
        }
        assets.sort();
        assets.dedup();
        assets
    }

    fn collect_assets_from_dir(&self, kind: &str, dir: &Path, assets: &mut Vec<(String, String)>) {
        if !dir.exists() {
            return;
        }

        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.collect_assets_from_dir(kind, &path, assets);
                continue;
            }

            if let Ok(relative) = path.strip_prefix(&self.base_dir) {
                assets.push((
                    kind.to_string(),
                    relative.to_string_lossy().replace('\\', "/"),
                ));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillManualSummary {
    pub name: String,
    pub description: String,
    pub runtime: Option<String>,
    pub executable: bool,
    pub classification: String,
    pub execution_surface: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillManualMatch {
    pub name: String,
    pub runtime: Option<String>,
    pub executable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillRegistryScope {
    Project,
    User,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRegistryRoot {
    pub scope: SkillRegistryScope,
    pub path: PathBuf,
}

/// A skill that executes an external script
pub struct DynamicSkill {
    metadata: SkillMetadata,
    instructions: String,
    base_dir: PathBuf,
    execution_config: SkillExecutionConfig,
    env_manager: Option<Arc<dyn benshu_infra::traits::env::SystemEnvironment>>,
    runtime: Option<Arc<dyn SkillRuntime>>,
}

impl DynamicSkill {
    /// Create a new dynamic skill
    pub fn new(metadata: SkillMetadata, instructions: String, base_dir: PathBuf) -> Self {
        let resources = metadata.resources.clone();
        let permissions = metadata.permissions.clone();
        let mut execution_config = SkillExecutionConfig {
            timeout_secs: resources.timeout_secs.unwrap_or(30),
            max_output_bytes: resources.max_output_bytes.unwrap_or(1024 * 1024),
            allow_network: permissions.network,
            use_browser: metadata.use_browser || permissions.browser,
            max_memory_mb: resources.max_memory_mb,
            max_cpu_percent: resources.max_cpu_percent,
            max_net_bps: resources.max_net_bps,
            max_disk_bps: resources.max_disk_bps,
            ..Default::default()
        };
        if metadata.runtime.as_deref().is_some_and(is_wasm_runtime) {
            execution_config.allow_network = false;
            execution_config.use_browser = false;
            execution_config.max_memory_mb = Some(execution_config.max_memory_mb.unwrap_or(128));
            execution_config.max_output_bytes = execution_config.max_output_bytes.min(1024 * 1024);
        }

        Self {
            metadata,
            instructions,
            base_dir,
            execution_config,
            env_manager: None,
            runtime: None,
        }
    }

    pub fn with_runtime(mut self, runtime: Arc<dyn SkillRuntime>) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// Set custom execution configuration
    pub fn with_execution_config(mut self, config: SkillExecutionConfig) -> Self {
        self.execution_config = config;
        self
    }

    /// Set an environment manager for auto-provisioning
    pub fn with_env_manager(
        mut self,
        env_manager: Arc<dyn benshu_infra::traits::env::SystemEnvironment>,
    ) -> Self {
        self.env_manager = Some(env_manager);
        self
    }

    /// Access metadata
    pub fn metadata(&self) -> &SkillMetadata {
        &self.metadata
    }

    fn declared_safety_level(&self) -> SafetyLevel {
        let permissions = &self.metadata.permissions;
        if permissions.network
            || permissions.browser
            || matches!(
                permissions.filesystem,
                SkillFilesystemAccess::ReadWriteSkill | SkillFilesystemAccess::WorkspaceReadWrite
            )
        {
            SafetyLevel::Yellow
        } else {
            SafetyLevel::Green
        }
    }
}

#[async_trait]
impl Tool for DynamicSkill {
    fn name(&self) -> String {
        self.metadata.name.clone()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.metadata.name.clone(),
            description: self.metadata.description.clone(),
            parameters: self.metadata.parameters.clone().unwrap_or(json!({})),
            parameters_ts: self.metadata.interface.clone(),
            is_binary: self
                .metadata
                .runtime
                .as_deref()
                .is_some_and(is_wasm_runtime),
            is_verified: false, // Default to unverified
            usage_guidelines: self.metadata.usage_guidelines.clone(),
            safety_level: self.declared_safety_level(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let runtime_type = self.metadata.runtime.as_deref().unwrap_or("python3");

        info!(tool = %self.name(), runtime = %runtime_type, "Dispatching skill execution");

        let throttle = benshu_brain::skills::CURRENT_THROTTLE
            .try_with(|t| *t)
            .unwrap_or(self.execution_config.throttle);
        let pressure = benshu_brain::skills::CURRENT_PRESSURE
            .try_with(|p| *p)
            .unwrap_or(false);

        // Phase 8: Autonomous Runtime Selection
        // If host is under heavy pressure, we can override the runtime to a lighter one
        let runtime = if let Some(ref r) = self.runtime {
            Arc::clone(r)
        } else {
            benshu_runtimes::get_runtime(runtime_type)
        };

        let mut config = self.execution_config.clone();
        config.throttle = throttle;
        config.is_low_resource = pressure;

        let output = runtime
            .execute(
                &self.metadata,
                arguments,
                &self.base_dir,
                &config,
                self.env_manager.as_ref(),
            )
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(anyhow::anyhow!(Error::ToolExecution {
                tool_name: self.name(),
                message: format!(
                    "Script error (exit code {}): {}\nStderr: {}",
                    output.status.code().unwrap_or(-1),
                    stdout,
                    stderr
                ),
            }));
        }

        Ok(stdout)
    }
}

fn is_wasm_runtime(runtime: &str) -> bool {
    runtime.eq_ignore_ascii_case("wasm")
}

/// Registry and loader for dynamic skills
pub struct SkillLoader {
    pub skills: DashMap<String, Arc<DynamicSkill>>,
    pub manuals: DashMap<String, Arc<SkillDoc>>,
    pub base_path: PathBuf,
    registry_roots: Vec<SkillRegistryRoot>,
    #[cfg(feature = "wasm")]
    pub(crate) wasm_runtime: Arc<dyn benshu_runtimes::SkillRuntime>,
    /// Phase 15: Session-aware state for forging and cleanup
    /// session_id -> (retry_counts, Vec<forged_dirs>)
    pub(crate) session_states: DashMap<String, (DashMap<String, u8>, Vec<PathBuf>)>,
    /// Phase 15-Revision: Environment manager for provisioned runtimes
    pub(crate) env_manager: Option<Arc<benshu_brain::env::EnvManager>>,
    /// Phase 15-Revision: Cache for runtime instances to avoid redundant allocations
    pub(crate) runtime_cache: Arc<DashMap<String, Arc<dyn benshu_runtimes::SkillRuntime>>>,
    /// Phase 15-Revision: Approval handler for security gates (Late Binding support)
    pub(crate) approval_handler:
        Arc<parking_lot::RwLock<Option<Arc<dyn benshu_brain::agent::ApprovalHandler>>>>,
}

impl SkillLoader {
    fn verify_skill_manifest(metadata: &SkillMetadata, instructions: &str) -> Result<()> {
        let verifier = SkillVerifier::default();
        let scan_target = format!(
            "{}\n{}\n{}",
            metadata.description,
            metadata.usage_guidelines.as_deref().unwrap_or_default(),
            instructions
        );
        let result = verifier.verify_skill(&metadata.name, &scan_target);
        if matches!(result.risk_level, RiskLevel::Critical) {
            let findings = result
                .findings
                .iter()
                .map(|finding| finding.description.clone())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(Error::Internal(format!(
                "Skill '{}' blocked by security verifier: {}",
                metadata.name, findings
            )));
        }
        Ok(())
    }

    pub fn manual_summaries(&self) -> Vec<SkillManualSummary> {
        let mut summaries: Vec<_> = self
            .manuals
            .iter()
            .map(|entry| {
                let skill = entry.value();
                SkillManualSummary {
                    name: skill.metadata().name.clone(),
                    description: skill.metadata().description.clone(),
                    runtime: skill.metadata().runtime.clone(),
                    executable: skill.is_executable(),
                    classification: if skill.is_executable() {
                        "executable".to_string()
                    } else {
                        "documentation_only".to_string()
                    },
                    execution_surface: skill.execution_surface().to_string(),
                }
            })
            .collect();
        summaries.sort_by(|left, right| left.name.cmp(&right.name));
        summaries
    }

    pub fn match_manual_reference(&self, query: &str) -> Option<SkillManualMatch> {
        let normalized_query = normalize_skill_lookup_text(query);
        if normalized_query.is_empty() {
            return None;
        }

        self.manual_summaries().into_iter().find_map(|summary| {
            let normalized_name = normalize_skill_lookup_text(&summary.name);
            if normalized_name.is_empty() || !normalized_query.contains(&normalized_name) {
                return None;
            }

            Some(SkillManualMatch {
                name: summary.name,
                runtime: summary.runtime,
                executable: summary.executable,
            })
        })
    }

    /// Get forge retry count for a skill in a specific session
    pub fn get_forge_retry_count(&self, session_id: &str, skill_name: &str) -> u8 {
        let entry = self
            .session_states
            .entry(session_id.to_string())
            .or_insert_with(|| (DashMap::new(), Vec::new()));
        let count = *entry.0.entry(skill_name.to_string()).or_insert(0);
        count
    }

    /// Increment forge retry count
    pub fn increment_forge_retry(&self, session_id: &str, skill_name: &str) {
        let entry = self
            .session_states
            .entry(session_id.to_string())
            .or_insert_with(|| (DashMap::new(), Vec::new()));
        let mut count = entry.0.entry(skill_name.to_string()).or_insert(0);
        *count += 1;
    }

    /// Record a directory created during a session for auto-cleanup
    pub fn record_session_dir(&self, session_id: &str, path: PathBuf) {
        let mut entry = self
            .session_states
            .entry(session_id.to_string())
            .or_insert_with(|| (DashMap::new(), Vec::new()));
        entry.1.push(path);
    }

    /// Cleanup all unhardened forged skills for a session
    pub async fn cleanup_session(&self, session_id: &str, hardened_names: &[String]) {
        if let Some((_, dirs)) = self.session_states.remove(session_id).map(|(_, v)| v) {
            for dir in dirs {
                let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !hardened_names.contains(&name.to_string()) {
                    // Phase 15-Revision: Resilience against file locking (especially on Windows)
                    for attempt in 1..=3 {
                        match tokio::fs::remove_dir_all(&dir).await {
                            Ok(_) => break,
                            Err(e) => {
                                tracing::warn!(
                                    "Attempt {} failed to delete session dir {:?}: {}. Retrying...",
                                    attempt,
                                    dir,
                                    e
                                );
                                if attempt < 3 {
                                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    /// Create a new registry
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        let base_path = base_path.into();
        Self {
            skills: DashMap::new(),
            manuals: DashMap::new(),
            base_path: base_path.clone(),
            registry_roots: vec![SkillRegistryRoot {
                scope: SkillRegistryScope::Project,
                path: base_path,
            }],
            session_states: DashMap::new(),
            env_manager: None,
            runtime_cache: Arc::new(DashMap::new()),
            approval_handler: Arc::new(parking_lot::RwLock::new(None)),
            #[cfg(feature = "wasm")]
            wasm_runtime: Arc::new(benshu_runtimes::WasmRuntime::new()),
        }
    }

    pub fn with_user_path(mut self, user_path: impl Into<PathBuf>) -> Self {
        self.registry_roots.push(SkillRegistryRoot {
            scope: SkillRegistryScope::User,
            path: user_path.into(),
        });
        self
    }

    pub fn with_additional_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.registry_roots.push(SkillRegistryRoot {
            scope: SkillRegistryScope::Custom,
            path: path.into(),
        });
        self
    }

    pub fn registry_roots(&self) -> &[SkillRegistryRoot] {
        &self.registry_roots
    }

    pub fn default_user_skill_path() -> Option<PathBuf> {
        if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .map(|path| path.join("BenShu").join("skills"))
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
                })
                .map(|path| path.join("benshu").join("skills"))
        }
    }

    /// Set an approval handler for all loaded skills
    pub fn with_approval_handler(
        self,
        approval_handler: Arc<dyn benshu_brain::agent::ApprovalHandler>,
    ) -> Self {
        *self.approval_handler.write() = Some(approval_handler);
        self
    }

    /// Set an approval handler on an existing loader (Late Binding)
    pub fn set_approval_handler(
        &self,
        approval_handler: Arc<dyn benshu_brain::agent::ApprovalHandler>,
    ) {
        *self.approval_handler.write() = Some(approval_handler);
    }

    /// Set an environment manager for all loaded skills
    pub fn with_env_manager(mut self, env_manager: Arc<benshu_brain::env::EnvManager>) -> Self {
        self.env_manager = Some(env_manager);
        self
    }

    /// Load all skills from the configured project/user/custom registry roots.
    pub async fn load_all(&self) -> Result<()> {
        self.skills.clear();
        self.manuals.clear();

        let mut join_set = tokio::task::JoinSet::new();

        for (root_order, root) in self.registry_roots.iter().cloned().enumerate() {
            if !root.path.exists() {
                continue;
            }

            let mut entries = tokio::fs::read_dir(&root.path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_dir() {
                    let env_manager = self.env_manager.clone();
                    let path_clone = path.clone();
                    let cache = self.runtime_cache.clone();
                    let root_path = root.path.clone();

                    join_set.spawn(async move {
                        let (metadata, instructions) =
                            crate::compiler::SkillParser::parse_file(&path_clone).await?;
                        Self::verify_skill_manifest(&metadata, &instructions)?;
                        let doc = Arc::new(SkillDoc::new(
                            metadata.clone(),
                            instructions,
                            path_clone.clone(),
                        ));

                        if !doc.is_executable() {
                            return Result::Ok((root_order, root_path, doc, None));
                        }

                        let mut skill =
                            DynamicSkill::new(metadata, doc.instructions().to_string(), path_clone);

                        if let Some(em) = env_manager {
                            skill = skill.with_env_manager(em);
                        }

                        // Phase 15-Revision: Use runtime cache to avoid expensive re-allocations
                        let runtime_type = skill.metadata().runtime.as_deref().unwrap_or("bash");
                        let runtime = if let Some(r) = cache.get(runtime_type) {
                            r.clone()
                        } else {
                            let r = benshu_runtimes::get_runtime(runtime_type);
                            cache.insert(runtime_type.to_string(), r.clone());
                            r
                        };

                        skill = skill.with_runtime(runtime);
                        Result::Ok((root_order, root_path, doc, Some(skill)))
                    });
                }
            }
        }

        let mut loaded = Vec::new();
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(Ok(item)) => loaded.push(item),
                Ok(Err(e)) => {
                    // Phase 15-Revision: Continue loading others even if one fails
                    tracing::error!(
                        "Failed to load skill at base path {:?}: {}",
                        self.base_path,
                        e
                    );
                }
                Err(e) => {
                    tracing::error!("Join error during skill loading: {}", e);
                }
            }
        }

        loaded.sort_by_key(|(root_order, _, doc, _)| (*root_order, doc.metadata().name.clone()));
        for (_, root_path, doc, maybe_skill) in loaded {
            info!(
                "Loaded skill manual: {} from {}",
                doc.metadata().name,
                root_path.display()
            );
            self.manuals
                .insert(doc.metadata().name.clone(), Arc::clone(&doc));

            if let Some(skill) = maybe_skill {
                info!("Loaded executable skill: {}", skill.name());
                self.skills.insert(skill.name(), Arc::new(skill));
            }
        }
        Ok(())
    }

    pub async fn load_skill(&self, path: &Path) -> Result<DynamicSkill> {
        let (metadata, instructions) = crate::compiler::SkillParser::parse_file(path).await?;
        Self::verify_skill_manifest(&metadata, &instructions)?;

        Ok(DynamicSkill::new(
            metadata,
            instructions,
            path.to_path_buf(),
        ))
    }
}

#[async_trait::async_trait]
impl ContextInjector for SkillLoader {
    async fn inject(&self, _history: &[Message]) -> BrainResult<Vec<Message>> {
        if self.manuals.is_empty() {
            return Ok(Vec::new());
        }

        let mut content = String::from("## Available Skills\n\n");
        content.push_str("You have the following skills available via `read_skill_manual`:\n\n");

        for skill in self.manual_summaries() {
            if skill.executable {
                let runtime = skill.runtime.as_deref().unwrap_or("none");
                content.push_str(&format!(
                    "- **{}** [{} | execution_surface={} | runtime={}]: {}\n",
                    skill.name,
                    skill.classification,
                    skill.execution_surface,
                    runtime,
                    skill.description
                ));
            } else {
                content.push_str(&format!(
                    "- **{}** [{} | execution_surface={}]: {}\n",
                    skill.name, skill.classification, skill.execution_surface, skill.description
                ));
            }
        }

        content.push_str(
            "\nUse `read_skill_manual(skill_name)` to see full instructions for any skill.\n",
        );

        Ok(vec![Message::system(content)])
    }
}

fn normalize_skill_lookup_text(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            if ch.is_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if matches!(ch, '_' | '-' | '/' | '\\') {
                Some(' ')
            } else if ch.is_whitespace() {
                Some(' ')
            } else {
                None
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Tool to read the full SKILL.md guide for a specific skill
pub struct ReadSkillDoc {
    loader: Arc<SkillLoader>,
}

impl ReadSkillDoc {
    pub fn new(loader: Arc<SkillLoader>) -> Self {
        Self { loader }
    }
}

#[async_trait]
impl Tool for ReadSkillDoc {
    fn name(&self) -> String {
        "read_skill_manual".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Read the full SKILL.md manual for a specific skill to understand its parameters and usage examples.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "The name of the skill to read documentation for"
                    }
                },
                "required": ["skill_name"]
            }),
            parameters_ts: Some("interface ReadSkillArgs {\n  skill_name: string; // The name of the skill to read manual for\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            skill_name: String,
        }
        let args: Args = serde_json::from_str(arguments)?;

        if let Some(skill) = self.loader.manuals.get(&args.skill_name) {
            let mut content = format!(
                "# Skill: {}\n\n{}",
                skill.metadata().name,
                skill.instructions()
            );
            let classification = if skill.is_executable() {
                "executable"
            } else {
                "documentation_only"
            };
            let runtime = skill
                .metadata()
                .runtime
                .clone()
                .unwrap_or_else(|| "none".to_string());
            content.push_str("\n\n## Skill Surface Contract\n\n");
            content.push_str(&format!("- classification: {classification}\n"));
            content.push_str("- tool_surface: skill_loading\n");
            content.push_str(&format!(
                "- execution_surface: {}\n",
                skill.execution_surface()
            ));
            content.push_str(&format!("- runtime: {runtime}\n"));
            content.push_str(&format!("- kind: {}\n", skill.metadata().kind));
            let permissions = &skill.metadata().permissions;
            content.push_str(&format!(
                "- permissions: filesystem={:?}, network={}, browser={}\n",
                permissions.filesystem, permissions.network, permissions.browser
            ));
            let resources = &skill.metadata().resources;
            if resources.timeout_secs.is_some()
                || resources.max_output_bytes.is_some()
                || resources.max_memory_mb.is_some()
                || resources.max_cpu_percent.is_some()
            {
                content.push_str(&format!(
                    "- resources: timeout_secs={:?}, max_output_bytes={:?}, max_memory_mb={:?}, max_cpu_percent={:?}\n",
                    resources.timeout_secs,
                    resources.max_output_bytes,
                    resources.max_memory_mb,
                    resources.max_cpu_percent
                ));
            }
            if runtime.eq_ignore_ascii_case("wasm") {
                let contract = skill.metadata().wasm.clone().unwrap_or_default();
                content.push_str(&format!(
                    "- wasm_contract: abi={}, entrypoint={}, sha256={}\n",
                    contract.abi,
                    contract.entrypoint,
                    contract.sha256.as_deref().unwrap_or("none")
                ));
            }
            let assets = skill.available_assets();
            if !assets.is_empty() {
                content.push_str("\n\n## Available Skill Assets\n\n");
                for (kind, path) in assets {
                    content.push_str(&format!("- `{path}` ({kind})\n"));
                }
                content.push_str(
                    "\nUse `read_skill_asset` with the relative asset path when you need one of these supporting files.\n",
                );
            }
            Ok(content)
        } else {
            Err(anyhow::anyhow!(
                "Skill '{}' not found in registry",
                args.skill_name
            ))
        }
    }
}

/// Tool to read a specific reference/template/script file that belongs to a skill.
const MAX_SKILL_ASSET_BYTES: u64 = 5 * 1024 * 1024;

pub struct ReadSkillAsset {
    loader: Arc<SkillLoader>,
}

impl ReadSkillAsset {
    pub fn new(loader: Arc<SkillLoader>) -> Self {
        Self { loader }
    }

    fn validate_relative_asset_path(relative_path: &Path) -> anyhow::Result<()> {
        if relative_path.is_absolute() {
            return Err(anyhow::anyhow!(
                "Asset path must be relative to the skill directory"
            ));
        }

        let mut components = relative_path.components();
        let Some(Component::Normal(first)) = components.next() else {
            return Err(anyhow::anyhow!(
                "Asset path must start with references/, templates/, or scripts/"
            ));
        };

        let first = first.to_string_lossy();
        if !matches!(first.as_ref(), "references" | "templates" | "scripts") {
            return Err(anyhow::anyhow!(
                "Asset path must stay inside references/, templates/, or scripts/"
            ));
        }

        if relative_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(anyhow::anyhow!(
                "Asset path cannot contain parent directory traversal"
            ));
        }

        Ok(())
    }
}

#[async_trait]
impl Tool for ReadSkillAsset {
    fn name(&self) -> String {
        "read_skill_asset".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Read a supporting asset from a skill, such as a reference, template, or script file, after loading the main skill manual.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "The name of the skill that owns the asset"
                    },
                    "asset_path": {
                        "type": "string",
                        "description": "Relative path inside references/, templates/, or scripts/"
                    }
                },
                "required": ["skill_name", "asset_path"]
            }),
            parameters_ts: Some("interface ReadSkillAssetArgs {\n  skill_name: string;\n  asset_path: string; // Relative path inside references/, templates/, or scripts/\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Call this after `read_skill_manual` when a skill manual points you to a specific reference, template, or script asset.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            skill_name: String,
            asset_path: String,
        }

        let args: Args = serde_json::from_str(arguments)?;
        let Some(skill) = self.loader.manuals.get(&args.skill_name) else {
            return Err(anyhow::anyhow!(
                "Skill '{}' not found in registry",
                args.skill_name
            ));
        };

        let relative_path = PathBuf::from(&args.asset_path);
        Self::validate_relative_asset_path(&relative_path)?;

        let asset_path = skill.base_dir().join(&relative_path);
        if !asset_path.exists() || !asset_path.is_file() {
            return Err(anyhow::anyhow!(
                "Skill asset '{}' was not found for skill '{}'",
                args.asset_path,
                args.skill_name
            ));
        }

        let canonical_base = std::fs::canonicalize(skill.base_dir())?;
        let canonical_asset = std::fs::canonicalize(&asset_path)?;
        if !canonical_asset.starts_with(&canonical_base) {
            return Err(anyhow::anyhow!(
                "Skill asset '{}' escaped the skill directory",
                args.asset_path
            ));
        }

        let metadata = tokio::fs::metadata(&canonical_asset).await?;
        if metadata.len() > MAX_SKILL_ASSET_BYTES {
            anyhow::bail!(
                "Skill asset '{}' is larger than the 5MB safety limit",
                args.asset_path
            );
        }
        let content = tokio::fs::read_to_string(&canonical_asset).await?;
        Ok(format!(
            "# Skill Asset: {}\n\n{}",
            relative_path.to_string_lossy().replace('\\', "/"),
            content
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use benshu_infra::traits::runtime::SkillRuntime as InfraSkillRuntime;
    use parking_lot::Mutex;
    use std::process::{ExitStatus, Output};
    use tempfile::tempdir;

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;

    fn write_skill(dir: &Path, name: &str, content: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).expect("create skill dir");
        std::fs::write(skill_dir.join("SKILL.md"), content).expect("write SKILL.md");
    }

    #[tokio::test]
    async fn load_all_keeps_doc_only_skill_in_manuals() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "doc_only",
            r#"---
name: doc_only
description: Documentation-only workflow
kind: knowledge
---
# Doc Skill

This skill explains a workflow and should not execute anything directly.
"#,
        );

        let loader = SkillLoader::new(temp.path());
        loader.load_all().await.expect("load skills");

        assert!(loader.manuals.contains_key("doc_only"));
        assert!(!loader.skills.contains_key("doc_only"));
    }

    #[tokio::test]
    async fn load_all_registers_executable_skill_in_manuals_and_tools() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "exec_skill",
            r#"---
name: exec_skill
description: Executable runtime skill
runtime: bash
script: run.sh
---
# Exec Skill

Execute through the configured runtime.
"#,
        );

        let loader = SkillLoader::new(temp.path());
        loader.load_all().await.expect("load skills");

        assert!(loader.manuals.contains_key("exec_skill"));
        assert!(loader.skills.contains_key("exec_skill"));
    }

    #[tokio::test]
    async fn load_all_reads_project_and_user_skill_roots() {
        let project = tempdir().expect("project tempdir");
        let user = tempdir().expect("user tempdir");
        write_skill(
            project.path(),
            "project_skill",
            r#"---
name: project_skill
description: Project scoped skill
kind: knowledge
---
# Project Skill
"#,
        );
        write_skill(
            user.path(),
            "user_skill",
            r#"---
name: user_skill
description: User scoped skill
kind: knowledge
---
# User Skill
"#,
        );

        let loader = SkillLoader::new(project.path()).with_user_path(user.path());
        loader.load_all().await.expect("load skills");

        assert!(loader.manuals.contains_key("project_skill"));
        assert!(loader.manuals.contains_key("user_skill"));
        assert_eq!(loader.registry_roots().len(), 2);
    }

    #[tokio::test]
    async fn load_all_uses_deterministic_later_root_override() {
        let project = tempdir().expect("project tempdir");
        let user = tempdir().expect("user tempdir");
        write_skill(
            project.path(),
            "shared_skill",
            r#"---
name: shared_skill
description: Project scoped skill
kind: knowledge
---
# Project Skill
"#,
        );
        write_skill(
            user.path(),
            "shared_skill",
            r#"---
name: shared_skill
description: User scoped skill
kind: knowledge
---
# User Skill
"#,
        );

        let loader = SkillLoader::new(project.path()).with_user_path(user.path());
        loader.load_all().await.expect("load skills");

        let manual = loader
            .manuals
            .get("shared_skill")
            .expect("shared skill manual");
        assert_eq!(manual.metadata().description, "User scoped skill");
    }

    #[tokio::test]
    async fn context_injector_lists_manual_skills() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "doc_only",
            r#"---
name: doc_only
description: Documentation-only workflow
kind: knowledge
---
# Doc Skill

Read me first.
"#,
        );

        let loader = SkillLoader::new(temp.path());
        loader.load_all().await.expect("load skills");

        let injected = loader.inject(&[]).await.expect("inject context");
        let content = injected
            .first()
            .map(|m| m.content.as_text())
            .unwrap_or_default()
            .to_string();

        assert!(content.contains("read_skill_manual"));
        assert!(content.contains("doc_only"));
    }

    #[tokio::test]
    async fn match_manual_reference_detects_named_skill_for_progressive_loading() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "python_tooling",
            r#"---
name: python_tooling
description: Python workflow with uv runtime
runtime: uv
script: run.py
---
# Python Tooling

Read the manual first.
"#,
        );

        let loader = SkillLoader::new(temp.path());
        loader.load_all().await.expect("load skills");

        let matched = loader
            .match_manual_reference("请按 python tooling 这个 skill 来做")
            .expect("should match skill manual");
        assert_eq!(matched.name, "python_tooling");
        assert_eq!(matched.runtime.as_deref(), Some("uv"));
        assert!(matched.executable);
    }

    #[tokio::test]
    async fn context_injector_exposes_only_skill_summaries_not_full_manual_content() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "python_tooling",
            r#"---
name: python_tooling
description: Python workflow with uv runtime
runtime: uv
script: run.py
---
# Python Tooling

SECRET_MANUAL_SENTENCE_DO_NOT_INLINE

Read the references before running.
"#,
        );

        let loader = SkillLoader::new(temp.path());
        loader.load_all().await.expect("load skills");

        let injected = loader.inject(&[]).await.expect("inject context");
        let content = injected
            .first()
            .map(|m| m.content.as_text())
            .unwrap_or_default()
            .to_string();

        assert!(content.contains("## Available Skills"));
        assert!(content.contains("read_skill_manual"));
        assert!(content.contains("python_tooling"));
        assert!(content.contains("Python workflow with uv runtime"));
        assert!(content.contains("[executable | execution_surface=runtime | runtime=uv]"));
        assert!(!content.contains("SECRET_MANUAL_SENTENCE_DO_NOT_INLINE"));
        assert!(!content.contains("Read the references before running."));
    }

    #[tokio::test]
    async fn context_injector_distinguishes_doc_and_executable_skill_surfaces() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "doc_only",
            r#"---
name: doc_only
description: Documentation-only workflow
kind: knowledge
---
# Doc Skill
"#,
        );
        write_skill(
            temp.path(),
            "python_tooling",
            r#"---
name: python_tooling
description: Python workflow with uv runtime
runtime: uv
script: run.py
---
# Python Tooling
"#,
        );

        let loader = SkillLoader::new(temp.path());
        loader.load_all().await.expect("load skills");

        let injected = loader.inject(&[]).await.expect("inject context");
        let content = injected
            .first()
            .map(|m| m.content.as_text())
            .unwrap_or_default()
            .to_string();

        assert!(content.contains("doc_only"));
        assert!(content.contains("[documentation_only | execution_surface=none]"));
        assert!(content.contains("python_tooling"));
        assert!(content.contains("[executable | execution_surface=runtime | runtime=uv]"));
    }

    #[tokio::test]
    async fn read_skill_manual_lists_available_assets() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "python_tooling",
            r#"---
name: python_tooling
description: Python workflow with uv runtime
runtime: uv
script: run.py
---
# Python Tooling

See the references before running.
"#,
        );
        let skill_dir = temp.path().join("python_tooling");
        std::fs::create_dir_all(skill_dir.join("references")).expect("create references");
        std::fs::create_dir_all(skill_dir.join("templates")).expect("create templates");
        std::fs::write(
            skill_dir.join("references").join("setup.md"),
            "Reference content",
        )
        .expect("write reference");
        std::fs::write(
            skill_dir.join("templates").join("example.txt"),
            "Template content",
        )
        .expect("write template");

        let loader = Arc::new(SkillLoader::new(temp.path()));
        loader.load_all().await.expect("load skills");

        let tool = ReadSkillDoc::new(loader);
        let content = tool
            .call(r#"{"skill_name":"python_tooling"}"#)
            .await
            .expect("read manual");

        assert!(content.contains("Available Skill Assets"));
        assert!(content.contains("references/setup.md"));
        assert!(content.contains("templates/example.txt"));
        assert!(content.contains("read_skill_asset"));
        assert!(content.contains("## Skill Surface Contract"));
        assert!(content.contains("- classification: executable"));
        assert!(content.contains("- execution_surface: runtime"));
        assert!(content.contains("- runtime: uv"));
    }

    #[tokio::test]
    async fn read_skill_manual_exposes_doc_only_surface_contract() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "doc_only",
            r#"---
name: doc_only
description: Documentation-only workflow
kind: knowledge
---
# Doc Skill

This skill explains a workflow and should not execute anything directly.
"#,
        );

        let loader = Arc::new(SkillLoader::new(temp.path()));
        loader.load_all().await.expect("load skills");
        let tool = ReadSkillDoc::new(loader);
        let content = tool
            .call(r#"{"skill_name":"doc_only"}"#)
            .await
            .expect("read doc_only skill");

        assert!(content.contains("## Skill Surface Contract"));
        assert!(content.contains("- classification: documentation_only"));
        assert!(content.contains("- execution_surface: none"));
        assert!(content.contains("- runtime: none"));
    }

    #[tokio::test]
    async fn read_skill_asset_reads_allowed_files_and_rejects_traversal() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "python_tooling",
            r#"---
name: python_tooling
description: Python workflow with uv runtime
---
# Python Tooling
"#,
        );
        let skill_dir = temp.path().join("python_tooling");
        std::fs::create_dir_all(skill_dir.join("references")).expect("create references");
        std::fs::write(
            skill_dir.join("references").join("setup.md"),
            "Reference content",
        )
        .expect("write reference");

        let loader = Arc::new(SkillLoader::new(temp.path()));
        loader.load_all().await.expect("load skills");

        let tool = ReadSkillAsset::new(loader.clone());
        let content = tool
            .call(r#"{"skill_name":"python_tooling","asset_path":"references/setup.md"}"#)
            .await
            .expect("read asset");
        assert!(content.contains("Reference content"));

        let err = tool
            .call(r#"{"skill_name":"python_tooling","asset_path":"../secrets.txt"}"#)
            .await
            .expect_err("traversal should be rejected");
        assert!(err.to_string().contains("Asset path"));
    }

    fn success_output(stdout: &str) -> Output {
        Output {
            status: success_status(),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[cfg(unix)]
    fn success_status() -> ExitStatus {
        ExitStatus::from_raw(0)
    }

    #[cfg(windows)]
    fn success_status() -> ExitStatus {
        ExitStatus::from_raw(0)
    }

    #[derive(Default)]
    struct RecordingRuntime {
        calls: Mutex<Vec<(Option<String>, Option<String>, PathBuf, String)>>,
    }

    #[async_trait]
    impl InfraSkillRuntime for RecordingRuntime {
        async fn execute(
            &self,
            metadata: &SkillMetadata,
            arguments: &str,
            base_dir: &Path,
            _config: &SkillExecutionConfig,
            _env_manager: Option<&Arc<dyn benshu_infra::traits::env::SystemEnvironment>>,
        ) -> anyhow::Result<Output> {
            self.calls.lock().push((
                metadata.runtime.clone(),
                metadata.script.clone(),
                base_dir.to_path_buf(),
                arguments.to_string(),
            ));
            Ok(success_output("executed via declared runtime"))
        }
    }

    #[tokio::test]
    async fn executable_skill_executes_with_declared_runtime_contract() {
        let temp = tempdir().expect("tempdir");
        let skill_dir = temp.path().join("exec_skill");
        std::fs::create_dir_all(&skill_dir).expect("create skill dir");

        let metadata = SkillMetadata {
            name: "exec_skill".to_string(),
            description: "Executable runtime skill".to_string(),
            homepage: None,
            parameters: None,
            interface: None,
            script: Some("run.sh".to_string()),
            runtime: Some("bash".to_string()),
            metadata: json!({}),
            kind: "tool".to_string(),
            usage_guidelines: None,
            dependencies: Vec::new(),
            use_browser: false,
            models: Vec::new(),
            source_fallback: None,
            safety_audit: None,
            permissions: Default::default(),
            resources: Default::default(),
            wasm: None,
        };

        let runtime = Arc::new(RecordingRuntime::default());
        let skill = DynamicSkill::new(
            metadata,
            "Execute through bash.".to_string(),
            skill_dir.clone(),
        )
        .with_runtime(runtime.clone());

        let result = skill
            .call(r#"{"task":"demo"}"#)
            .await
            .expect("execute skill");
        assert_eq!(result, "executed via declared runtime");

        let calls = runtime.calls.lock();
        assert_eq!(calls.len(), 1);
        let (runtime_name, script_name, base_dir, arguments) = &calls[0];
        assert_eq!(runtime_name.as_deref(), Some("bash"));
        assert_eq!(script_name.as_deref(), Some("run.sh"));
        assert_eq!(base_dir, &skill_dir);
        assert_eq!(arguments, r#"{"task":"demo"}"#);
    }
}
/// Tool to search and install skills from Smithery using CLI (npm/pnpm/bun)
#[cfg(feature = "http")]
pub struct SmitheryTool {
    loader: Arc<SkillLoader>,
}

#[cfg(feature = "http")]
impl SmitheryTool {
    pub fn new(loader: Arc<SkillLoader>) -> Self {
        Self { loader }
    }
}

#[cfg(feature = "http")]
#[async_trait]
impl Tool for SmitheryTool {
    fn name(&self) -> String {
        "smithery_manager".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Search and install new skills from the Smithery.ai registry. Supports 'search' to find skills and 'install' to add them to your environment.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["search", "install"],
                        "description": "The action to perform"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query or skill slug to install"
                    },
                    "manager": {
                        "type": "string",
                        "enum": ["npm", "pnpm", "bun"],
                        "description": "The package manager to use (default: npm)"
                    }
                },
                "required": ["action", "query"]
            }),
            parameters_ts: Some("interface ClawHubArgs {\n  action: 'search' | 'install';\n  query: string; // Search query or skill slug\n  manager?: 'npm' | 'pnpm' | 'bun'; // Package manager (default: npm)\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            query: String,
            manager: Option<String>,
        }
        let args: Args = serde_json::from_str(arguments)?;

        let manager = args.manager.as_deref().unwrap_or({
            if cfg!(windows) {
                "bun"
            } else {
                "npm"
            }
        });
        let (cmd, base_args) = match manager {
            "pnpm" => ("pnpm", vec!["dlx", "smithery@latest"]),
            "bun" => ("bunx", vec!["smithery@latest"]),
            "pixi" => ("pixi", vec!["run", "bunx", "smithery@latest"]),
            _ => ("npx", vec!["smithery@latest"]),
        };

        match args.action.as_str() {
            "search" => {
                info!(
                    "Searching Smithery registry for: {} (via {})",
                    args.query, manager
                );
                let output = tokio::process::Command::new(cmd)
                    .args(&base_args)
                    .arg("search")
                    .arg(&args.query)
                    .output()
                    .await?;

                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            }
            "install" => {
                info!(
                    "Installing skill from Smithery: {} (via {})",
                    args.query, manager
                );
                let output = tokio::process::Command::new(cmd)
                    .args(&base_args)
                    .arg("install")
                    .arg(&args.query)
                    .output()
                    .await?;

                if output.status.success() {
                    // Refresh the loader to pick up the new skill
                    info!(
                        "Skill {} installed successfully, refreshing registry...",
                        args.query
                    );
                    self.loader.load_all().await?;
                    Ok(format!(
                        "Successfully installed '{}'. It is now available for use.",
                        args.query
                    ))
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    Err(anyhow::anyhow!("Failed to install skill: {}", err))
                }
            }
            _ => Err(anyhow::anyhow!("Unknown action: {}", args.action)),
        }
    }
}
