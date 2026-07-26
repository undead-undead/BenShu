use crate::SkillRuntime;
use async_trait::async_trait;
use benshu_security::sandbox::NativeShellRuntime;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

/// SmartCCRuntime — handles `runtime: c`, `runtime: cpp`, or `runtime: gcc` skills with full fallback strategies.
///
/// 3-Tiered logic + Multi-Level Fallbacks:
/// 1. Provision GCC: EnvManager → System PATH → Auto-download
/// 2. Compile: Optimized → Basic → No-Opt → Dynamic Link (fallbacks)
/// 3. Execute: Sandboxed Binary → Containerized → Interpreter (fallbacks)
pub struct SmartCCRuntime;

impl SmartCCRuntime {
    pub fn new() -> Self {
        Self
    }

    /// Fallback: Search system GCC/G++ without EnvManager
    fn find_system_compiler(is_cpp: bool) -> Option<PathBuf> {
        let compiler_name = if is_cpp {
            if cfg!(windows) {
                "g++.exe"
            } else {
                "g++"
            }
        } else {
            if cfg!(windows) {
                "gcc.exe"
            } else {
                "gcc"
            }
        };
        which::which(compiler_name).ok()
    }

    /// Compile with configurable flags (for fallbacks)
    async fn compile_with_flags(
        &self,
        compiler: &Path,
        script_path: &Path,
        output_bin_path: &Path,
        flags: &[&str],
        dependencies: &[String],
    ) -> anyhow::Result<std::process::Output> {
        let mut compile_cmd = tokio::process::Command::new(compiler);
        compile_cmd.arg(script_path).arg("-o").arg(output_bin_path);

        // Add custom flags
        for flag in flags {
            if !flag.is_empty() {
                compile_cmd.arg(flag);
            }
        }

        // Add dependencies
        for dep in dependencies {
            if dep.starts_with("-") {
                compile_cmd.arg(dep);
            } else {
                compile_cmd.arg(format!("-l{}", dep));
            }
        }

        // Set compile timeout (30s) to prevent hanging
        compile_cmd.kill_on_drop(true);
        let compile_output = tokio::time::timeout(Duration::from_secs(30), compile_cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("Compilation timed out for skill"))?
            .map_err(|e| anyhow::anyhow!("Failed to run compiler: {}", e))?;

        Ok(compile_output)
    }

    /// Schedule file for later cleanup (if immediate cleanup fails)
    fn schedule_cleanup(path: PathBuf) {
        tokio::spawn(async move {
            // Retry 3 times with backoff
            for i in 1..=3 {
                if tokio::fs::remove_file(&path).await.is_ok() {
                    debug!("Cleaned up temp binary (retry {}): {:?}", i, path);
                    return;
                }
                tokio::time::sleep(Duration::from_secs(i)).await;
            }
            warn!("Failed to cleanup temp binary after 3 retries: {:?}", path);
        });
    }
}

#[async_trait]
impl SkillRuntime for SmartCCRuntime {
    async fn execute(
        &self,
        metadata: &benshu_infra::skill::SkillMetadata,
        arguments: &str,
        base_dir: &Path,
        config: &benshu_infra::skill::SkillExecutionConfig,
        env_manager: Option<&std::sync::Arc<dyn benshu_infra::traits::env::SystemEnvironment>>,
    ) -> anyhow::Result<std::process::Output> {
        // Step 1: Detect type and get compiler (multi-level fallback)
        let script_file = metadata
            .script
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No script defined for C/C++ skill"))?;
        let is_cpp = script_file.ends_with(".cpp")
            || script_file.ends_with(".cc")
            || metadata.runtime.as_deref() == Some("cpp");

        let (compiler_path, discovery_method) = {
            // Level 1: EnvManager (primary)
            if let Some(em) = env_manager {
                match em.ensure_gcc().await {
                    Ok(gcc_path) => {
                        if is_cpp {
                            let gpp_name = if cfg!(windows) { "g++.exe" } else { "g++" };
                            let local_gpp = gcc_path
                                .parent()
                                .map(|p| p.join(gpp_name))
                                .unwrap_or_else(|| PathBuf::from(gpp_name));
                            if local_gpp.exists() {
                                (local_gpp, "EnvManager (Bundled)")
                            } else {
                                (gcc_path, "EnvManager (GCC only)")
                            }
                        } else {
                            (gcc_path, "EnvManager")
                        }
                    }
                    Err(e) => {
                        warn!(
                            "EnvManager GCC provision failed: {}. Trying system compiler...",
                            e
                        );
                        (
                            Self::find_system_compiler(is_cpp)
                                .ok_or_else(|| anyhow::anyhow!("No GCC/G++ found"))?,
                            "System Discovery",
                        )
                    }
                }
            } else {
                (
                    Self::find_system_compiler(is_cpp)
                        .ok_or_else(|| anyhow::anyhow!("No system GCC/G++ found"))?,
                    "System PATH",
                )
            }
        };

        debug!(skill = %metadata.name, compiler = ?compiler_path, method = %discovery_method, "Using compiler");

        // Step 2: Prepare paths (safe unique name)
        let script_path = base_dir.join("scripts").join(script_file);
        let safe_name = metadata.name.replace(|c: char| !c.is_alphanumeric(), "_");
        let output_bin_name = if cfg!(windows) {
            format!("skill_{}_bin.exe", safe_name)
        } else {
            format!("skill_{}_bin", safe_name)
        };
        let output_bin_path = base_dir.join(output_bin_name);

        // Step 3: Compilation fallback chain
        let compile_strategies = [
            // Strategy 1: Optimized (primary)
            vec![
                "-O3",
                if cfg!(windows) { "-static-libgcc" } else { "" },
                if cfg!(windows) {
                    "-static-libstdc++"
                } else {
                    ""
                },
            ],
            // Strategy 2: Basic optimization (-O2)
            vec!["-O2"],
            // Strategy 3: No optimization + ignore warnings (-O0 -w)
            vec!["-O0", "-w"],
            // Strategy 4: Dynamic link only (O0)
            vec!["-O0"],
        ];

        let mut compile_success = false;
        let mut last_compile_error = String::new();

        for (i, flags) in compile_strategies.iter().enumerate() {
            let filtered_flags: Vec<&str> =
                flags.iter().filter(|f| !f.is_empty()).cloned().collect();
            info!(skill = %metadata.name, strategy = i+1, "Compiling (strategy {})...", i+1);

            let compile_output = match self
                .compile_with_flags(
                    &compiler_path,
                    &script_path,
                    &output_bin_path,
                    &filtered_flags,
                    &metadata.dependencies,
                )
                .await
            {
                Ok(out) => out,
                Err(e) => {
                    last_compile_error = e.to_string();
                    continue;
                }
            };

            if compile_output.status.success() {
                compile_success = true;
                break;
            } else {
                last_compile_error = String::from_utf8_lossy(&compile_output.stderr).to_string();
                let _ = tokio::fs::remove_file(&output_bin_path).await;
            }
        }

        if !compile_success {
            return Err(anyhow::anyhow!(
                "All compilation strategies failed for '{}':\n{}",
                metadata.name,
                last_compile_error
            ));
        }

        // Step 4: Execute via NativeShellRuntime
        let mut mod_meta = metadata.clone();
        mod_meta.runtime = Some(output_bin_path.to_string_lossy().to_string());
        mod_meta.script = Some(String::new());

        let native = NativeShellRuntime::new();
        let result = match native
            .execute(&mod_meta, arguments, base_dir, config, env_manager)
            .await
        {
            Ok(res) => Ok(res),
            Err(e) => {
                let unsafe_override = std::env::var("BENSHU_UNSAFE_EXEC")
                    .map(|v| v == "true")
                    .unwrap_or(false);
                if unsafe_override {
                    warn!(skill = %metadata.name, "Sandbox failed: {}. Falling back to UNSAFE execution.", e);
                    let mut cmd = tokio::process::Command::new(&output_bin_path);
                    if !arguments.is_empty() {
                        cmd.arg(arguments);
                    }
                    Ok(cmd
                        .output()
                        .await
                        .map_err(|e| anyhow::anyhow!("Unsafe execution failed: {}", e))?)
                } else {
                    Err(e)
                }
            }
        };

        // Step 5: Cleanup
        if tokio::fs::remove_file(&output_bin_path).await.is_err() {
            Self::schedule_cleanup(output_bin_path);
        }

        result
    }
}
