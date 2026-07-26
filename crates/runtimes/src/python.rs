use crate::SkillRuntime;
use async_trait::async_trait;
use benshu_security::sandbox::NativeShellRuntime;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// SmartPythonRuntime — handles `runtime: python` or `runtime: python3` skills.
///
/// Universal Tiered Strategy:
/// 1. Level 1: Pixi + UV (Integrated BenShu Environment with Conda support)
/// 2. Level 2: Universal UV (Standalone/System UV for extreme speed)
/// 3. Level 3: System Python (Fallback)
pub struct SmartPythonRuntime;

impl SmartPythonRuntime {
    pub fn new() -> Self {
        Self
    }

    async fn find_system_interpreter(name: &str) -> Option<PathBuf> {
        which::which(name).ok()
    }
}

#[async_trait]
impl SkillRuntime for SmartPythonRuntime {
    async fn execute(
        &self,
        metadata: &benshu_infra::skill::SkillMetadata,
        arguments: &str,
        base_dir: &Path,
        config: &benshu_infra::skill::SkillExecutionConfig,
        env_manager: Option<&std::sync::Arc<dyn benshu_infra::traits::env::SystemEnvironment>>,
    ) -> anyhow::Result<std::process::Output> {
        let start_time = std::time::Instant::now(); // Phase 15-Revision: Start performance timer

        // --- LEVEL 0: Forged Skill / UV Cache Sensing ---
        if !metadata.dependencies.is_empty() {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            metadata.dependencies.hash(&mut hasher);
            let deps_hash = format!("{:x}", hasher.finish());

            if let Some(parent) = base_dir.parent() {
                let env_path = parent.join(".envs").join(&deps_hash);
                let python_bin = if cfg!(target_os = "windows") {
                    env_path.join("Scripts").join("python.exe")
                } else {
                    env_path.join("bin").join("python")
                };

                if python_bin.exists() {
                    let check_cmd = std::process::Command::new(&python_bin)
                        .arg("--version")
                        .output();

                    if let Ok(output) = check_cmd {
                        if output.status.success() {
                            debug!(skill = %metadata.name, hash = %deps_hash, "Using valid cached UV environment");
                            let mut mod_meta = metadata.clone();
                            mod_meta.runtime = Some(python_bin.to_string_lossy().to_string());
                            let res = NativeShellRuntime::new()
                                .execute(&mod_meta, arguments, base_dir, config, env_manager)
                                .await;
                            if res.is_ok() {
                                info!(skill = %metadata.name, "Successfully used cached UV environment in {}ms", start_time.elapsed().as_millis());
                            }
                            return res;
                        }
                    }

                    warn!(skill = %metadata.name, "UV cache exists but Python is invalid or broken. Cleaning up...");
                    let _ = tokio::fs::remove_dir_all(&env_path).await;
                }
            }
        }

        // --- LEVEL 1: Pixi + UV (Integrated BenShu Environment) ---
        if let Some(em) = env_manager {
            debug!(skill = %metadata.name, "Attempting Pixi + UV provisioning...");
            match em
                .provision(&metadata.name, &metadata.dependencies, metadata.use_browser)
                .await
            {
                Ok(env_prefix) => {
                    let python_bin = if cfg!(target_os = "windows") {
                        env_prefix.join("python.exe")
                    } else {
                        env_prefix.join("bin").join("python")
                    };
                    if python_bin.exists() {
                        let mut mod_meta = metadata.clone();
                        mod_meta.runtime = Some(python_bin.to_string_lossy().to_string());
                        let res = NativeShellRuntime::new()
                            .execute(&mod_meta, arguments, base_dir, config, env_manager)
                            .await;
                        if res.is_ok() {
                            info!(skill = %metadata.name, "Successfully used Pixi + UV environment in {}ms", start_time.elapsed().as_millis());
                        }
                        return res;
                    }
                }
                Err(e) => {
                    warn!(skill = %metadata.name, "Pixi Python provision failed: {}. Falling back...", e);
                }
            }
        }

        // --- LEVEL 2: Universal UV (Standalone/System UV) ---
        let uv_path = if let Some(em) = env_manager {
            em.ensure_uv().await.ok()
        } else {
            Self::find_system_interpreter("uv").await
        };

        if let Some(path) = uv_path {
            debug!(skill = %metadata.name, "Using UV as standalone universal adapter");

            let mut mod_meta = metadata.clone();
            let deps_arg = if metadata.dependencies.is_empty() {
                String::new()
            } else {
                format!("--with {}", metadata.dependencies.join(","))
            };

            let uv_cmd = format!("{} run {} python", path.to_string_lossy(), deps_arg);
            mod_meta.runtime = Some(uv_cmd);

            let res = NativeShellRuntime::new()
                .execute(&mod_meta, arguments, base_dir, config, env_manager)
                .await;
            if res.is_ok() {
                info!(skill = %metadata.name, "Successfully used standalone UV runtime in {}ms", start_time.elapsed().as_millis());
            }
            return res;
        }

        // --- LEVEL 3: Fallback System Python ---
        if let Some(py_path) =
            Self::find_system_interpreter(if cfg!(windows) { "python" } else { "python3" }).await
        {
            warn!(skill = %metadata.name, "UV not found. Falling back to system Python (Lower performance/isolation)");
            let mut mod_meta = metadata.clone();
            mod_meta.runtime = Some(py_path.to_string_lossy().to_string());
            let res = NativeShellRuntime::new()
                .execute(&mod_meta, arguments, base_dir, config, env_manager)
                .await;
            if res.is_ok() {
                info!(skill = %metadata.name, "Successfully used system Python in {}ms (isolation may be low)", start_time.elapsed().as_millis());
            }
            return res;
        }

        Err(anyhow::anyhow!(
            "No suitable Python runtime found. UV (recommended) or Python required."
        ))
    }
}
