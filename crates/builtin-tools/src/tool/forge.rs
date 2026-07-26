#[cfg(feature = "http")]
use crate::compiler::GithubCompiler;
use crate::{DynamicSkill, SkillLoader};
use async_trait::async_trait;
use benshu_brain::skills::tool::ToolSet;
use benshu_compression::preview_text;
use benshu_infra::skill::SkillMetadata;
use benshu_infra::{SafetyLevel, Tool, ToolCatalogOverride, ToolDefinition};
use benshu_security::sandbox::NativeShellRuntime;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Dynamic thresholds for skill forging, allowing for different strategies
/// based on the model's capabilities and environment (Cloud vs Local).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ForgeDynamicThresholds {
    /// Max number of failed forge attempts before giving up
    pub forge_retry_limit: u8,
    /// Minimum complexity score required to trigger forging
    pub complexity_trigger: f32,
    /// Minimum tokens for a forge prompt
    pub token_min: usize,
    /// Maximum tokens for a forge prompt
    pub token_max: Option<usize>,
    /// How many seconds a tool execution can take before suggesting a forge replacement
    pub efficiency_trigger_secs: u64,
}

impl Default for ForgeDynamicThresholds {
    fn default() -> Self {
        Self {
            forge_retry_limit: 3,
            complexity_trigger: 0.75,
            token_min: 4096,
            token_max: Some(128_000),
            efficiency_trigger_secs: 30,
        }
    }
}

/// Tool to forge new skills at runtime
pub struct ForgeSkill {
    loader: Arc<SkillLoader>,
    toolset: ToolSet,
    base_dir: PathBuf,
    /// The session ID for this agent, used for retry tracking and cleanup
    pub session_id: Arc<parking_lot::RwLock<Option<String>>>,
    /// Whether the model using this tool is a local model
    pub is_local: bool,
    /// Shared cache for UV environments (hash(deps) -> (path, timestamp))
    pub uv_env_cache: Arc<RwLock<HashMap<String, (PathBuf, std::time::Instant)>>>,
    /// Thresholds for this agent's forging behavior
    pub thresholds: ForgeDynamicThresholds,
    #[cfg(feature = "http")]
    compiler: Option<GithubCompiler>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ForgeSmokeReport {
    status: String,
    latency_ms: u64,
    execution_surface: String,
    output_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ForgeResultEnvelope {
    status: String,
    tool_name: String,
    source: String,
    scope: String,
    capability_domain: Option<String>,
    execution_surface: String,
    smoke_test: ForgeSmokeReport,
    session_cleanup_recorded: bool,
    message: String,
}

impl ForgeSkill {
    fn registered_runtime(runtime: &str) -> String {
        match runtime {
            "rust" => "wasm".to_string(),
            "c" | "cpp" => "bin".to_string(),
            "js" | "quickjs" | "qjs" => "qjs".to_string(),
            "uv" | "pixi" | "python" | "python3" => "python3".to_string(),
            other => other.to_string(),
        }
    }

    fn capability_domain_for_registered_runtime(runtime: &str) -> Option<String> {
        match runtime {
            "qjs" | "quickjs" | "js" | "node" | "bash" | "python3" | "python" | "uv" | "pixi"
            | "bin" | "wasm" => Some("runtime_surface".to_string()),
            _ => None,
        }
    }

    fn build_skill_metadata(
        &self,
        args: &ForgeSkillArgs,
        source_fallback: Option<String>,
        safety_audit: Option<String>,
    ) -> SkillMetadata {
        SkillMetadata {
            name: args.name.clone(),
            description: args.description.clone(),
            homepage: None,
            parameters: None,
            interface: args.interface.clone(),
            script: Some(args.filename.clone()),
            runtime: Some(Self::registered_runtime(&args.runtime)),
            metadata: json!({}),
            kind: "tool".to_string(),
            usage_guidelines: None,
            dependencies: args.dependencies.clone().unwrap_or_default(),
            use_browser: false,
            models: Vec::new(),
            source_fallback,
            safety_audit,
            permissions: Default::default(),
            resources: Default::default(),
            wasm: None,
        }
    }

    async fn smoke_test_forged_skill(
        &self,
        metadata: &SkillMetadata,
        instructions: &str,
        skill_dir: &Path,
    ) -> anyhow::Result<ForgeSmokeReport> {
        let runtime_type = metadata.runtime.as_deref().unwrap_or("bash");
        let runtime = if let Some(existing) = self.loader.runtime_cache.get(runtime_type) {
            existing.clone()
        } else {
            let runtime = benshu_runtimes::get_runtime(runtime_type);
            self.loader
                .runtime_cache
                .insert(runtime_type.to_string(), runtime.clone());
            runtime
        };

        let mut skill = DynamicSkill::new(
            metadata.clone(),
            instructions.to_string(),
            skill_dir.to_path_buf(),
        )
        .with_runtime(runtime);
        if let Some(env_manager) = &self.loader.env_manager {
            skill = skill.with_env_manager(env_manager.clone());
        }

        let started_at = Instant::now();
        let output = tokio::time::timeout(std::time::Duration::from_secs(10), skill.call("{}"))
            .await
            .map_err(|_| {
                anyhow::anyhow!("FORGE_VERIFICATION_FAILED: smoke-test timed out after 10s")
            })??;
        let preview = preview_text(&output, 160);

        Ok(ForgeSmokeReport {
            status: "passed".to_string(),
            latency_ms: started_at.elapsed().as_millis() as u64,
            execution_surface: "runtime".to_string(),
            output_preview: if preview.is_empty() {
                None
            } else {
                Some(preview)
            },
        })
    }

    #[cfg(feature = "http")]
    pub fn new(
        loader: Arc<SkillLoader>,
        toolset: ToolSet,
        base_dir: PathBuf,
        compiler: Option<GithubCompiler>,
        uv_env_cache: Arc<RwLock<HashMap<String, (PathBuf, std::time::Instant)>>>,
        thresholds: ForgeDynamicThresholds,
        session_id: Arc<parking_lot::RwLock<Option<String>>>,
        is_local: bool,
    ) -> Self {
        Self {
            loader,
            toolset,
            base_dir,
            compiler,
            uv_env_cache,
            thresholds,
            session_id,
            is_local,
        }
    }

    #[cfg(not(feature = "http"))]
    pub fn new(
        loader: Arc<SkillLoader>,
        toolset: ToolSet,
        base_dir: PathBuf,
        _compiler: Option<()>,
        uv_env_cache: Arc<RwLock<HashMap<String, (PathBuf, std::time::Instant)>>>,
        thresholds: ForgeDynamicThresholds,
        session_id: Arc<parking_lot::RwLock<Option<String>>>,
        is_local: bool,
    ) -> Self {
        Self {
            loader,
            toolset,
            base_dir,
            uv_env_cache,
            thresholds,
            session_id,
            is_local,
        }
    }

    /// Perform a static security audit on the generated script
    fn audit_script(&self, script: &str, runtime: &str) -> anyhow::Result<()> {
        use regex::Regex;

        let sensitive_regex = [
            r"rm\s+-rf\s+/",
            r"chmod\s+777",
            r"/dev/tcp/",
            r"/etc/shadow",
            r"powershell\s+-e",
            r"Invoke-Expression",
            r"encodedCommand",
        ];

        for pattern in sensitive_regex {
            let re = Regex::new(pattern).unwrap();
            if re.is_match(script) {
                return Err(anyhow::anyhow!(
                    "SECURITY_DENIED: Script contains sensitive pattern: {}",
                    pattern
                ));
            }
        }

        // Skip some risky checks for local models to allow more freedom
        if self.is_local {
            return Ok(());
        }

        // Python specific dangerous imports - Cloud models are strictly monitored
        if runtime.contains("python")
            && (script.contains("os.system") || script.contains("subprocess."))
        {
            return Err(anyhow::anyhow!("SECURITY_DENIED: Python subprocess/system calls are restricted for cloud models. Use local models for such tasks."));
        }

        Ok(())
    }

    async fn finalize_forge(
        &self,
        args: ForgeSkillArgs,
        skill_dir: PathBuf,
        precomputed_smoke: Option<ForgeSmokeReport>,
    ) -> anyhow::Result<String> {
        // Phase 15.4: Determine source_fallback for immutable rollback
        let source_fallback =
            if args.runtime == "c" || args.runtime == "cpp" || args.runtime == "rust" {
                Some(args.filename.clone())
            } else {
                None
            };

        // Phase 15.4: Set safety audit status
        let safety_audit = if args.runtime == "c" || args.runtime == "cpp" {
            Some("sandbox_isolated".to_string())
        } else {
            Some("script_runtime".to_string())
        };

        // 3. Write SKILL.md
        let metadata = self.build_skill_metadata(&args, source_fallback, safety_audit);

        let yaml = serde_yaml_ng::to_string(&metadata)?;
        let skill_md = format!("---\n{}---\n\n{}", yaml, args.instructions);
        tokio::fs::write(skill_dir.join("SKILL.md"), skill_md).await?;

        let smoke_report = if let Some(report) = precomputed_smoke {
            report
        } else {
            self.smoke_test_forged_skill(&metadata, &args.instructions, &skill_dir)
                .await?
        };

        // 4. Load into memory
        let registered_runtime = metadata.runtime.clone();
        let skill = DynamicSkill::new(metadata, args.instructions, skill_dir);
        let skill_arc = Arc::new(skill);

        // Add to loader registry
        self.loader
            .skills
            .insert(args.name.clone(), Arc::clone(&skill_arc));

        // Convert to trait object for ToolSet (Phase 14)
        let tool_arc: Arc<dyn Tool> = skill_arc;

        // Add to active toolset for the current agent with explicit session-scoped forge metadata.
        let capability_domain = registered_runtime
            .as_deref()
            .and_then(Self::capability_domain_for_registered_runtime);
        self.toolset.add_shared_with_catalog(
            tool_arc,
            ToolCatalogOverride {
                source: Some("forge".to_string()),
                scope: Some("session".to_string()),
                capability_domain: capability_domain.clone(),
                tags: vec!["forge".to_string(), "session".to_string()],
            },
        );

        let envelope = ForgeResultEnvelope {
            status: "success".to_string(),
            tool_name: args.name.clone(),
            source: "forge".to_string(),
            scope: "session".to_string(),
            capability_domain,
            execution_surface: smoke_report.execution_surface.clone(),
            smoke_test: smoke_report,
            session_cleanup_recorded: !args.is_permanent.unwrap_or(false),
            message: format!(
                "SUCCESS: Skill '{}' forged and loaded. You can now use it by calling '{}'.",
                args.name, args.name
            ),
        };

        Ok(serde_json::to_string(&envelope)?)
    }
}

#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ForgeSkillArgs {
    /// Technical name of the skill (snake_case)
    pub name: String,
    /// Short description of what the skill does
    pub description: String,
    /// Detailed instructions/guide for the agent on how to use it
    #[serde(default = "default_forge_instructions")]
    pub instructions: String,
    /// The source code for the skill
    #[serde(alias = "code", alias = "content")]
    pub script: String,
    /// The runtime/language (python3, node, bash)
    pub runtime: String,
    /// Filename for the script (e.g. "my_tool.py")
    pub filename: String,
    /// TypeScript interface for the parameters
    pub interface: Option<String>,
    /// Optional dependencies (e.g. ["requests", "pandas"])
    pub dependencies: Option<Vec<String>>,
    /// Task complexity estimated by the agent (0.0 to 1.0)
    pub complexity: Option<f32>,
    /// Whether this skill should be permanently hardened into the registry
    pub is_permanent: Option<bool>,
    /// Phase 15.3: Baseline latency of the original script (for benchmarking)
    pub baseline_latency_ms: Option<u64>,
}

fn default_forge_instructions() -> String {
    "Use this skill according to its description and parameter interface.".to_string()
}

#[async_trait]
impl Tool for ForgeSkill {
    fn name(&self) -> String {
        "forge_skill".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Forge a new skill by providing its code, metadata, and instructions. The skill will be immediately available for use.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "instructions": { "type": "string" },
                    "script": { "type": "string" },
                    "runtime": { 
                        "type": "string", 
                        "enum": ["python3", "python", "uv", "pixi", "node", "js", "quickjs", "bash", "rust", "c", "cpp"],
                        "description": "The runtime/language. Use 'quickjs' for fast in-process JS, 'python' for UV/Pixi optimized Python, 'c'/'cpp' for native performance."
                    },
                    "filename": { "type": "string" },
                    "interface": { "type": "string" },
                    "dependencies": { 
                        "type": "array", 
                        "items": { "type": "string" },
                        "description": "List of required Python libraries (only for python/uv runtimes)"
                    },
                    "is_permanent": {
                        "type": "boolean",
                        "description": "Set to true if this should be a hardened, permanent skill package."
                    }
                },
                "required": ["name", "description", "instructions", "script", "runtime", "filename"]
            }),
            parameters_ts: Some("interface ForgeSkillArgs {\n  name: string;\n  description: string;\n  instructions: string;\n  script: string;\n  runtime: 'python3' | 'python' | 'uv' | 'pixi' | 'node' | 'js' | 'quickjs' | 'bash' | 'rust' | 'c' | 'cpp';\n  filename: string;\n  interface?: string;\n  dependencies?: string[];\n  is_permanent?: boolean;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this to create NEW capabilities that do not yet exist in your toolkit. Analyze the requirements carefully before generating code.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: ForgeSkillArgs = serde_json::from_str(arguments)?;

        let session_id = self
            .session_id
            .read()
            .clone()
            .unwrap_or_else(|| "default_session".to_string());

        // 1. Complexity Shield
        if let Some(complexity) = args.complexity {
            if complexity < self.thresholds.complexity_trigger {
                return Err(anyhow::anyhow!(
                    "FORGE_DENIED: Task complexity ({:.2}) below threshold ({:.2}). Try using existing shell/python tools instead.",
                    complexity, self.thresholds.complexity_trigger
                ));
            }
        }

        // 2. Retry Guard
        let retry_count = self.loader.get_forge_retry_count(&session_id, &args.name);
        if retry_count >= self.thresholds.forge_retry_limit {
            return Err(anyhow::anyhow!(
                "FORGE_LIMIT: Maximum retry limit ({}) reached for skill '{}' in this session.",
                self.thresholds.forge_retry_limit,
                args.name
            ));
        }

        // 3. Security Pre-Audit
        self.audit_script(&args.script, &args.runtime)?;

        // 4. Human-Gate for High Complexity
        if let Some(complexity) = args.complexity {
            if complexity > 0.85 {
                tracing::info!(
                    "Forge: High complexity ({:.2}) detected. Requesting Human-Gate approval.",
                    complexity
                );

                // Use standard approval handler to trigger messenger-based confirmation
                let handler_opt = self.loader.approval_handler.read().clone();
                let approved = if let Some(handler) = handler_opt {
                    handler
                        .approve(
                            &format!("forge:{}", args.name),
                            &format!(
                                "Reviewing generated {} code for skill '{}'",
                                args.runtime, args.name
                            ),
                            SafetyLevel::Red,
                        )
                        .await?
                } else {
                    tracing::warn!("Forge: No approval handler available for Human-Gate. Defaulting to rejection.");
                    false
                };

                if !approved {
                    return Err(anyhow::anyhow!("FORGE_REJECTED: High-complexity code review was rejected by human auditor."));
                }
            }
        }

        // 5. Prepare directory
        let skill_dir = self.base_dir.join(&args.name);
        let scripts_dir = skill_dir.join("scripts");
        tokio::fs::create_dir_all(&scripts_dir).await?;

        // 2. Write script
        let script_path = scripts_dir.join(&args.filename);

        if args.runtime == "rust" {
            #[cfg(feature = "http")]
            {
                let compiler = self.compiler.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("GitHub compiler not configured. Cannot forge Rust skills.")
                })?;

                let wasm_binary = compiler.compile(&args.name, &args.script).await?;
                let wasm_filename = format!("{}.wasm", args.name);
                let wasm_path = scripts_dir.join(&wasm_filename);
                tokio::fs::write(&wasm_path, wasm_binary).await?;

                let mut final_args = args.clone();
                final_args.filename = wasm_filename;
                return self.finalize_forge(final_args, skill_dir, None).await;
            }
            #[cfg(not(feature = "http"))]
            {
                return Err(anyhow::anyhow!(
                    "Rust skill forging requires 'http' feature (for GitHub Compiler)."
                ));
            }
        }

        if args.runtime == "c" || args.runtime == "cpp" {
            tokio::fs::write(&script_path, &args.script).await?;

            // Phase 15.4: Preserve original source as immutable fallback
            let fallback_path = scripts_dir.join(format!("{}.source", args.filename));
            tokio::fs::write(&fallback_path, &args.script).await?;

            let output_name = if cfg!(windows) {
                format!("{}.exe", args.name)
            } else {
                args.name.clone()
            };
            let bin_path = scripts_dir.join(&output_name);

            // Tiered Compiler Detection (Phase 15.3 Windows Resilience)
            let mut compiler_path = if args.runtime == "c" { "gcc" } else { "g++" }.to_string();
            let mut found = false;

            // 1. Try bundled w64devkit if in installation dir (bin/mingw/bin/gcc.exe)
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    let bundled_bin = exe_dir.join("infra").join("bin").join("mingw").join("bin");
                    let target_name = if args.runtime == "c" {
                        "gcc.exe"
                    } else {
                        "g++.exe"
                    };
                    let bundled_compiler = bundled_bin.join(target_name);
                    if bundled_compiler.exists() {
                        compiler_path = bundled_compiler.to_string_lossy().to_string();
                        found = true;
                    }
                }
            }

            // 2. Try System PATH if bundled fails
            if !found {
                let check_name = if cfg!(windows) {
                    if args.runtime == "c" {
                        "gcc.exe"
                    } else {
                        "g++.exe"
                    }
                } else {
                    if args.runtime == "c" {
                        "cc"
                    } else {
                        "c++"
                    }
                };
                if let Ok(path) = which::which(check_name) {
                    compiler_path = path.to_string_lossy().to_string();
                    found = true;
                }
            }

            // 3. Fallback: Suggest Python/QuickJS if COMPILER_MISSING
            if !found {
                tracing::warn!(
                    "Forge: COMPILER_MISSING for '{}'. Requested {}, but no gcc/cc found.",
                    args.name,
                    args.runtime
                );
                return Err(anyhow::anyhow!(
                    "FORGE_COMPILER_MISSING: No C/C++ compiler found on the host system. \
                    Action required: Please provide the skill logic as Python (runtime: 'uv') or JavaScript (runtime: 'quickjs') \
                    to ensure cross-platform compatibility without local compilers."
                ));
            }

            // Phase 15.3: Tiered Compilation Strategies for Forging
            let compile_strategies = [
                // Strategy 1: Optimized + Static
                vec![
                    "-O3",
                    if cfg!(windows) { "-static-libgcc" } else { "" },
                    if cfg!(windows) && args.runtime == "cpp" {
                        "-static-libstdc++"
                    } else {
                        ""
                    },
                ],
                // Strategy 2: Standard Optimization
                vec!["-O2"],
                // Strategy 3: No Optimization (Compatibility mode)
                vec!["-O0", "-w"],
            ];

            let mut compile_success = false;
            let mut last_compile_error = String::new();

            for (i, flags) in compile_strategies.iter().enumerate() {
                let mut cmd = tokio::process::Command::new(&compiler_path);
                cmd.arg(&script_path).arg("-o").arg(&bin_path);
                cmd.kill_on_drop(true);
                for flag in flags {
                    if !flag.is_empty() {
                        cmd.arg(flag);
                    }
                }

                match tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output()).await {
                    Ok(Ok(out)) if out.status.success() => {
                        compile_success = true;
                        tracing::info!("Forge: Compiled '{}' with strategy {}", args.name, i + 1);
                        break;
                    }
                    Ok(Ok(out)) => {
                        last_compile_error = String::from_utf8_lossy(&out.stderr).to_string();
                        let _ = tokio::fs::remove_file(&bin_path).await;
                    }
                    Ok(Err(e)) => {
                        last_compile_error = e.to_string();
                    }
                    Err(_) => {
                        last_compile_error = "Compilation timed out (30s limit)".to_string();
                    }
                }
            }

            if !compile_success {
                return Err(anyhow::anyhow!("FORGE_COMPILE_FAILED: Current host environment cannot build this skill.\nError: {}", last_compile_error));
            }

            // Phase 15.4: SANDBOXED Verification Loop (Shadow Benchmarking)
            tracing::info!("Forge: Running SANDBOXED smoke-test for '{}'", args.name);
            let test_start = std::time::Instant::now();

            use benshu_infra::SkillRuntime;
            let test_meta = benshu_infra::skill::SkillMetadata {
                name: format!("test_{}", args.name),
                description: "Temporary test for forged binary".to_string(),
                homepage: None,
                parameters: None,
                interface: None,
                runtime: Some(bin_path.to_string_lossy().to_string()),
                script: Some(String::new()),
                metadata: serde_json::Value::Null,
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

            let test_config = benshu_infra::skill::SkillExecutionConfig {
                timeout_secs: 5,
                max_memory_mb: Some(256),
                max_cpu_percent: Some(50),
                allow_network: false,
                throttle: benshu_infra::traits::resource::ThrottleLevel::Low,
                ..Default::default()
            };

            let native = NativeShellRuntime::new();

            // Fix: Coerce Arc<EnvManager> to Arc<dyn SystemEnvironment> by cloning
            let loader_env_arc: Option<Arc<dyn benshu_infra::traits::env::SystemEnvironment>> =
                self.loader.env_manager.as_ref().map(|arc| arc.clone() as _);

            let test_output = native
                .execute(
                    &test_meta,
                    "{}",
                    &self.base_dir,
                    &test_config,
                    loader_env_arc.as_ref(),
                )
                .await;

            match test_output {
                Ok(out) if out.status.success() => {
                    let actual_latency = test_start.elapsed().as_millis() as u64;
                    tracing::info!(
                        "Forge: Verification SUCCESS ({}ms). Promoting to Native status.",
                        actual_latency
                    );
                    let mut final_args = args.clone();
                    final_args.filename = output_name;
                    let preview = preview_text(&String::from_utf8_lossy(&out.stdout), 160);
                    let smoke_report = ForgeSmokeReport {
                        status: "passed".to_string(),
                        latency_ms: actual_latency,
                        execution_surface: "runtime".to_string(),
                        output_preview: if preview.is_empty() {
                            None
                        } else {
                            Some(preview)
                        },
                    };
                    return self
                        .finalize_forge(final_args, skill_dir, Some(smoke_report))
                        .await;
                }
                Ok(out) => {
                    tracing::error!("Forge: VERIFICATION_FAILED for '{}'. Executable crashed or returned error: {}", args.name, String::from_utf8_lossy(&out.stderr));
                    let _ = tokio::fs::remove_file(&bin_path).await;
                    return Err(anyhow::anyhow!("FORGE_VERIFICATION_FAILED: The compiled binary failed its sandboxed smoke-test. Output: {}", String::from_utf8_lossy(&out.stderr)));
                }
                Err(e) => {
                    tracing::error!(
                        "Forge: VERIFICATION_FAILED for '{}'. Trace: {}",
                        args.name,
                        e
                    );
                    let _ = tokio::fs::remove_file(&bin_path).await;
                    return Err(anyhow::anyhow!(
                        "FORGE_VERIFICATION_FAILED: Sandboxed smoke-test aborted with error: {}",
                        e
                    ));
                }
            }
        }

        tokio::fs::write(&script_path, &args.script).await?;

        // Handle Python Environment Reuse if UV/Python runtime
        if args.runtime == "uv" || args.runtime == "python" || args.runtime == "python3" {
            if let Some(deps) = &args.dependencies {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                deps.hash(&mut hasher);
                let deps_hash = format!("{:x}", hasher.finish());

                let cache_check = {
                    let cache = self.uv_env_cache.read();
                    cache.get(&deps_hash).cloned()
                };

                if let Some((env_path, _ts)) = cache_check {
                    tracing::info!("Forge: Reusing UV environment at {:?}", env_path);
                } else {
                    tracing::info!(
                        "Forge: Creating new Python environment for dependencies: {:?}",
                        deps
                    );
                    let env_path = self.base_dir.join(".envs").join(&deps_hash);

                    if !env_path.exists() {
                        tokio::fs::create_dir_all(&env_path).await?;
                        let has_uv = which::which("uv").is_ok();
                        let setup_result = if has_uv {
                            let setup = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                tokio::process::Command::new("uv")
                                    .arg("venv")
                                    .arg(&env_path)
                                    .status(),
                            )
                            .await;

                            match setup {
                                Ok(Ok(status)) if status.success() => tokio::time::timeout(
                                    std::time::Duration::from_secs(60),
                                    tokio::process::Command::new("uv")
                                        .arg("pip")
                                        .arg("install")
                                        .arg("--python")
                                        .arg(if cfg!(windows) {
                                            env_path.join("Scripts").join("python.exe")
                                        } else {
                                            env_path.join("bin").join("python")
                                        })
                                        .args(deps)
                                        .status(),
                                )
                                .await
                                .map(|r| r.map(|s| s.success()).unwrap_or(false))
                                .unwrap_or(false),
                                _ => false,
                            }
                        } else {
                            let venv_status = tokio::process::Command::new(if cfg!(windows) {
                                "python.exe"
                            } else {
                                "python3"
                            })
                            .args(["-m", "venv"])
                            .arg(&env_path)
                            .status()
                            .await
                            .map(|s| s.success())
                            .unwrap_or(false);

                            if venv_status {
                                tokio::process::Command::new(if cfg!(windows) {
                                    env_path.join("Scripts").join("pip.exe")
                                } else {
                                    env_path.join("bin").join("pip")
                                })
                                .arg("install")
                                .args(deps)
                                .status()
                                .await
                                .map(|s| s.success())
                                .unwrap_or(false)
                            } else {
                                false
                            }
                        };

                        if !setup_result {
                            return Err(anyhow::anyhow!(
                                "ENV_SETUP_FAILED: Failed to prepare Python environment."
                            ));
                        }
                    }

                    // Zero-dependency manual LRU pruning
                    {
                        let mut cache = self.uv_env_cache.write();
                        if cache.len() >= 20 {
                            let oldest_key = cache
                                .iter()
                                .min_by_key(|(_, v)| v.1)
                                .map(|(k, _)| k.clone());

                            if let Some(key) = oldest_key {
                                tracing::debug!("Forge: Cache capacity reached (20). Evicting oldest environment: {}", key);
                                cache.remove(&key);
                            }
                        }
                        cache.insert(deps_hash, (env_path, std::time::Instant::now()));
                    }
                }
            }
        }

        // Record for cleanup only if not permanent
        if !args.is_permanent.unwrap_or(false) {
            self.loader
                .record_session_dir(&session_id, skill_dir.clone());
        }
        self.loader.increment_forge_retry(&session_id, &args.name);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio::fs::metadata(&script_path).await?.permissions();
            perms.set_mode(0o755);
            tokio::fs::set_permissions(&script_path, perms).await?;
        }

        self.finalize_forge(args, skill_dir, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::{ForgeResultEnvelope, ForgeSmokeReport};

    #[test]
    fn forge_result_envelope_serializes_session_scoped_smoke_test_contract() {
        let envelope = ForgeResultEnvelope {
            status: "success".to_string(),
            tool_name: "pdf_builder".to_string(),
            source: "forge".to_string(),
            scope: "session".to_string(),
            capability_domain: Some("runtime_surface".to_string()),
            execution_surface: "runtime".to_string(),
            smoke_test: ForgeSmokeReport {
                status: "passed".to_string(),
                latency_ms: 42,
                execution_surface: "runtime".to_string(),
                output_preview: Some("ok".to_string()),
            },
            session_cleanup_recorded: true,
            message: "SUCCESS".to_string(),
        };

        let value = serde_json::to_value(envelope).expect("serialize forge envelope");
        assert_eq!(value["source"], "forge");
        assert_eq!(value["scope"], "session");
        assert_eq!(value["capability_domain"], "runtime_surface");
        assert_eq!(value["execution_surface"], "runtime");
        assert_eq!(value["smoke_test"]["status"], "passed");
        assert_eq!(value["smoke_test"]["latency_ms"], 42);
        assert_eq!(value["session_cleanup_recorded"], true);
    }
}
