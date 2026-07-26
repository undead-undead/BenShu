#[cfg(target_os = "windows")]
use anyhow::Context;
use async_trait::async_trait;
use benshu_brain::env::EnvManager;
use benshu_infra::traits::env::SystemEnvironment;
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};
use benshu_routing::{
    build_observed_verification_result_envelope, build_verified_verification_result_envelope,
    VerificationDomain, VerificationMode, VerificationSource,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;

#[derive(Clone)]
pub struct RuntimeSurfaceTool {
    env_manager: Arc<EnvManager>,
}

impl RuntimeSurfaceTool {
    pub fn new(env_manager: Arc<EnvManager>) -> Self {
        Self { env_manager }
    }

    fn supported_runtimes() -> &'static [&'static str] {
        &[
            "quickjs",
            "powershell",
            "cmd",
            "bash",
            "uv",
            "pixi",
            "bun",
            "gcc",
        ]
    }

    async fn inspect_runtime(
        &self,
        runtime: &str,
        ensure_if_missing: bool,
    ) -> anyhow::Result<RuntimeSurfaceStatus> {
        let normalized = runtime.trim().to_lowercase();
        match normalized.as_str() {
            "quickjs" | "qjs" | "js" => Ok(RuntimeSurfaceStatus {
                runtime: "quickjs".to_string(),
                available: true,
                managed: true,
                source: "embedded".to_string(),
                path: None,
                version: None,
                notes: Some("In-process QuickJS runtime managed by BenShu.".to_string()),
            }),
            "powershell" => Ok(self.inspect_simple_system_runtime(
                "powershell",
                if cfg!(windows) {
                    Some("powershell.exe")
                } else {
                    None
                },
                "Windows-native shell surface.",
            )),
            "cmd" => Ok(self.inspect_simple_system_runtime(
                "cmd",
                if cfg!(windows) { Some("cmd.exe") } else { None },
                "Windows command shell surface.",
            )),
            "bash" | "sh" | "shell" => self.inspect_bash_surface(ensure_if_missing).await,
            "uv" => {
                self.inspect_managed_binary(
                    "uv",
                    ensure_if_missing,
                    || self.env_manager.ensure_uv_inherent(),
                    Self::version_args_for("uv"),
                )
                .await
            }
            "pixi" => {
                self.inspect_managed_binary(
                    "pixi",
                    ensure_if_missing,
                    || self.env_manager.ensure_pixi_inherent(),
                    Self::version_args_for("pixi"),
                )
                .await
            }
            "bun" => {
                self.inspect_managed_binary(
                    "bun",
                    ensure_if_missing,
                    || self.env_manager.ensure_bun_inherent(),
                    Self::version_args_for("bun"),
                )
                .await
            }
            "gcc" | "cc" | "c" | "cpp" | "g++" => {
                self.inspect_managed_binary(
                    "gcc",
                    ensure_if_missing,
                    || self.env_manager.ensure_gcc_inherent(),
                    Self::version_args_for("gcc"),
                )
                .await
            }
            other => Err(anyhow::anyhow!(
                "Unsupported runtime_surface '{}'. Supported: {}",
                other,
                Self::supported_runtimes().join(", ")
            )),
        }
    }

    fn inspect_simple_system_runtime(
        &self,
        runtime: &str,
        command: Option<&str>,
        note: &str,
    ) -> RuntimeSurfaceStatus {
        let path = command
            .and_then(|cmd| which::which(cmd).ok())
            .map(|p| p.display().to_string());
        RuntimeSurfaceStatus {
            runtime: runtime.to_string(),
            available: path.is_some(),
            managed: false,
            source: if path.is_some() {
                "system".to_string()
            } else {
                "unavailable".to_string()
            },
            path,
            version: None,
            notes: Some(note.to_string()),
        }
    }

    async fn inspect_bash_surface(
        &self,
        ensure_if_missing: bool,
    ) -> anyhow::Result<RuntimeSurfaceStatus> {
        #[cfg(not(target_os = "windows"))]
        let _ = ensure_if_missing;

        if let Some(path) = self.discover_bash_path() {
            let source = if path.to_string_lossy().contains("git-bash") {
                "bundled"
            } else if path.starts_with(self.env_manager.get_infra_bin_dir()) {
                "infra_bin"
            } else {
                "system"
            };
            return Ok(RuntimeSurfaceStatus {
                runtime: "bash".to_string(),
                available: true,
                managed: true,
                source: source.to_string(),
                path: Some(path.display().to_string()),
                version: self.command_version(&path, &["--version"]).await,
                notes: Some("Portable bash surface for managed shell execution.".to_string()),
            });
        }

        #[cfg(target_os = "windows")]
        if ensure_if_missing {
            let _ = self
                .env_manager
                .ensure_git_inherent()
                .await
                .context("Failed to provision Git Bash for runtime_surface")?;
            if let Some(path) = self.discover_bash_path() {
                return Ok(RuntimeSurfaceStatus {
                    runtime: "bash".to_string(),
                    available: true,
                    managed: true,
                    source: "infra_bin".to_string(),
                    path: Some(path.display().to_string()),
                    version: self.command_version(&path, &["--version"]).await,
                    notes: Some("Portable Git Bash provisioned through BenShu.".to_string()),
                });
            }
        }

        Ok(RuntimeSurfaceStatus {
            runtime: "bash".to_string(),
            available: false,
            managed: true,
            source: "unavailable".to_string(),
            path: None,
            version: None,
            notes: Some(
                "Bash surface is unavailable on this host until Git Bash is present.".to_string(),
            ),
        })
    }

    fn discover_bash_path(&self) -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            if let Some(bundled_dir) = self.env_manager.get_bundled_bin_dir() {
                let bundled = bundled_dir.join("git-bash").join("bash.exe");
                if bundled.exists() {
                    return Some(bundled);
                }
            }

            let infra = self
                .env_manager
                .get_infra_bin_dir()
                .join("git-bash")
                .join("bash.exe");
            if infra.exists() {
                return Some(infra);
            }

            if let Ok(path) = which::which("bash") {
                return Some(path);
            }

            None
        }

        #[cfg(not(target_os = "windows"))]
        {
            which::which("bash").or_else(|_| which::which("sh")).ok()
        }
    }

    async fn inspect_managed_binary<F, Fut>(
        &self,
        runtime: &str,
        ensure_if_missing: bool,
        ensure_fn: F,
        version_args: &'static [&'static str],
    ) -> anyhow::Result<RuntimeSurfaceStatus>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<PathBuf>>,
    {
        let path = if ensure_if_missing {
            Some(ensure_fn().await?)
        } else {
            self.discover_managed_binary(runtime)
        };

        let source = path
            .as_ref()
            .map(|path| self.binary_source_label(path))
            .unwrap_or("unavailable".to_string());

        let version = if let Some(path) = &path {
            self.command_version(path, version_args).await
        } else {
            None
        };

        Ok(RuntimeSurfaceStatus {
            runtime: runtime.to_string(),
            available: path.is_some(),
            managed: true,
            source,
            path: path.map(|p| p.display().to_string()),
            version,
            notes: Some(format!(
                "Managed runtime surface for `{}` through BenShu provisioning.",
                runtime
            )),
        })
    }

    fn discover_managed_binary(&self, runtime: &str) -> Option<PathBuf> {
        let runtime = runtime.to_lowercase();
        let binary_name = match runtime.as_str() {
            "uv" => Self::platform_bin("uv"),
            "pixi" => Self::platform_bin("pixi"),
            "bun" => Self::platform_bin("bun"),
            "gcc" => {
                #[cfg(target_os = "windows")]
                {
                    "gcc.exe".to_string()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    "gcc".to_string()
                }
            }
            _ => return None,
        };

        if runtime == "gcc" {
            #[cfg(target_os = "windows")]
            {
                let infra = self
                    .env_manager
                    .get_infra_bin_dir()
                    .join("mingw")
                    .join("bin")
                    .join(&binary_name);
                if infra.exists() {
                    return Some(infra);
                }
            }

            if let Ok(path) = which::which(&binary_name) {
                return Some(path);
            }
            return None;
        }

        if let Some(bundled_dir) = self.env_manager.get_bundled_bin_dir() {
            let bundled = bundled_dir.join(&binary_name);
            if bundled.exists() {
                return Some(bundled);
            }
        }

        let infra = self.env_manager.get_infra_bin_dir().join(&binary_name);
        if infra.exists() {
            return Some(infra);
        }

        which::which(&binary_name).ok()
    }

    fn binary_source_label(&self, path: &Path) -> String {
        if let Some(bundled_dir) = self.env_manager.get_bundled_bin_dir() {
            if path.starts_with(&bundled_dir) {
                return "bundled".to_string();
            }
        }
        if path.starts_with(self.env_manager.get_infra_bin_dir()) {
            return "infra_bin".to_string();
        }
        "system".to_string()
    }

    fn platform_bin(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    fn version_args_for(runtime: &str) -> &'static [&'static str] {
        match runtime {
            "gcc" => &["--version"],
            "pixi" => &["--version"],
            "bun" => &["--version"],
            _ => &["--version"],
        }
    }

    async fn command_version(&self, path: &Path, args: &[&str]) -> Option<String> {
        let output = Command::new(path).args(args).output().await.ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let merged = if stdout.trim().is_empty() {
            stderr
        } else {
            stdout
        };
        merged
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeSurfaceArgs {
    action: String,
    #[serde(default)]
    runtime: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    use_browser: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct RuntimeSurfaceStatus {
    runtime: String,
    available: bool,
    managed: bool,
    source: String,
    path: Option<String>,
    version: Option<String>,
    notes: Option<String>,
}

fn runtime_surface_source(
    kind: &str,
    title: impl Into<String>,
    uri: impl Into<String>,
) -> VerificationSource {
    VerificationSource {
        kind: kind.to_string(),
        title: title.into(),
        uri: uri.into(),
        observed_at: Some(chrono::Utc::now().to_rfc3339()),
    }
}

#[async_trait]
impl Tool for RuntimeSurfaceTool {
    fn name(&self) -> String {
        "runtime_surface".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Inspect, ensure, and provision BenShu-managed runtime surfaces such as quickjs, bash, uv, pixi, bun, and gcc.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["catalog", "inspect", "ensure", "provision_env"],
                        "description": "catalog lists managed runtime surfaces, inspect checks one runtime without installing it, ensure provisions a managed runtime if missing, provision_env creates a managed environment through uv/pixi."
                    },
                    "runtime": {
                        "type": "string",
                        "enum": Self::supported_runtimes(),
                        "description": "Runtime surface to inspect or ensure."
                    },
                    "id": {
                        "type": "string",
                        "description": "Stable environment id for provision_env."
                    },
                    "dependencies": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Dependencies for provision_env."
                    },
                    "use_browser": {
                        "type": "boolean",
                        "description": "Whether the provisioned environment should include browser support."
                    }
                },
                "required": ["action"]
            }),
            parameters_ts: Some("interface RuntimeSurfaceArgs {\n  action: 'catalog' | 'inspect' | 'ensure' | 'provision_env';\n  runtime?: 'quickjs' | 'powershell' | 'cmd' | 'bash' | 'uv' | 'pixi' | 'bun' | 'gcc';\n  id?: string;\n  dependencies?: string[];\n  use_browser?: boolean;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this tool when a task needs BenShu-managed runtimes. Prefer it over guessing runtime availability. Use `catalog` or `inspect` first when uncertain; use `ensure` only when the runtime is actually needed.".to_string()),
            safety_level: SafetyLevel::Yellow,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: RuntimeSurfaceArgs = serde_json::from_str(arguments)?;
        let response = match args.action.as_str() {
            "catalog" => {
                let mut items = Vec::new();
                for runtime in Self::supported_runtimes() {
                    items.push(self.inspect_runtime(runtime, false).await?);
                }
                json!({
                    "action": "catalog",
                    "verification_preview": build_verified_verification_result_envelope(
                        VerificationDomain::StateFact,
                        VerificationMode::RuntimeStateCheck,
                        Self::supported_runtimes()
                            .iter()
                            .map(|runtime| runtime_surface_source(
                                "runtime_surface",
                                format!("Runtime surface {}", runtime),
                                format!("runtime_surface://{}", runtime)
                            ))
                            .collect(),
                        "runtime surface catalog inspected"
                    ),
                    "runtimes": items,
                })
            }
            "inspect" => {
                let runtime = args
                    .runtime
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("`runtime` is required for action `inspect`"))?;
                let status = self.inspect_runtime(runtime, false).await?;
                json!({
                    "action": "inspect",
                    "verification_preview": build_observed_verification_result_envelope(
                        VerificationDomain::StateFact,
                        VerificationMode::RuntimeStateCheck,
                        vec![runtime_surface_source(
                            "runtime_surface",
                            format!("Runtime surface {}", status.runtime),
                            status
                                .path
                                .clone()
                                .unwrap_or_else(|| format!("runtime_surface://{}", status.runtime))
                        )],
                        Vec::new(),
                        vec![format!(
                            "runtime={} available={} managed={} source={} path={}",
                            status.runtime,
                            status.available,
                            status.managed,
                            status.source,
                            status.path.as_deref().unwrap_or("unavailable")
                        )],
                        "runtime surface inspect completed"
                    ),
                    "status": status,
                })
            }
            "ensure" => {
                let runtime = args
                    .runtime
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("`runtime` is required for action `ensure`"))?;
                let status = self.inspect_runtime(runtime, true).await?;
                json!({
                    "action": "ensure",
                    "verification_preview": build_observed_verification_result_envelope(
                        VerificationDomain::StateFact,
                        VerificationMode::RuntimeStateCheck,
                        vec![runtime_surface_source(
                            "runtime_surface",
                            format!("Runtime surface {}", status.runtime),
                            status
                                .path
                                .clone()
                                .unwrap_or_else(|| format!("runtime_surface://{}", status.runtime))
                        )],
                        Vec::new(),
                        vec![format!(
                            "runtime={} available={} managed={} source={} path={}",
                            status.runtime,
                            status.available,
                            status.managed,
                            status.source,
                            status.path.as_deref().unwrap_or("unavailable")
                        )],
                        "runtime surface ensure completed"
                    ),
                    "status": status,
                })
            }
            "provision_env" => {
                let id = args.id.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("`id` is required for action `provision_env`")
                })?;
                let prefix = self
                    .env_manager
                    .provision(id, &args.dependencies, args.use_browser)
                    .await?;
                json!({
                    "action": "provision_env",
                    "verification_preview": build_observed_verification_result_envelope(
                        VerificationDomain::ExecutionFact,
                        VerificationMode::ExecutionResultCheck,
                        vec![runtime_surface_source(
                            "managed_env",
                            format!("Managed runtime env {}", id),
                            prefix.display().to_string()
                        )],
                        vec![format!(
                            "provisioned managed env id={} prefix={} dependencies={} browser={}",
                            id,
                            prefix.display(),
                            args.dependencies.join(","),
                            args.use_browser
                        )],
                        Vec::new(),
                        "managed runtime environment provision completed"
                    ),
                    "id": id,
                    "dependencies": args.dependencies,
                    "use_browser": args.use_browser,
                    "prefix": prefix.display().to_string(),
                })
            }
            other => {
                return Err(anyhow::anyhow!(
                    "Unsupported runtime_surface action `{}`",
                    other
                ));
            }
        };

        Ok(serde_json::to_string_pretty(&response)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn runtime_surface_catalog_lists_managed_surfaces() {
        let temp = tempdir().unwrap();
        let tool = RuntimeSurfaceTool::new(Arc::new(EnvManager::new(temp.path().join("runtimes"))));

        let out = tool.call(r#"{"action":"catalog"}"#).await.unwrap();
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        let runtimes = value["runtimes"].as_array().unwrap();
        assert!(runtimes.iter().any(|item| item["runtime"] == "quickjs"));
        assert!(runtimes.iter().any(|item| item["runtime"] == "uv"));
        assert!(runtimes.iter().any(|item| item["runtime"] == "bash"));
    }

    #[tokio::test]
    async fn runtime_surface_inspect_reports_quickjs_as_managed() {
        let temp = tempdir().unwrap();
        let tool = RuntimeSurfaceTool::new(Arc::new(EnvManager::new(temp.path().join("runtimes"))));

        let out = tool
            .call(r#"{"action":"inspect","runtime":"quickjs"}"#)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["verification_preview"]["mode"], "RuntimeStateCheck");
        assert_eq!(
            value["verification_preview"]["state_evidence"][0],
            "runtime=quickjs available=true managed=true source=embedded path=unavailable"
        );
        assert_eq!(value["status"]["runtime"], "quickjs");
        assert_eq!(value["status"]["managed"], true);
        assert_eq!(value["status"]["source"], "embedded");
    }

    #[tokio::test]
    async fn runtime_surface_provision_env_emits_execution_evidence() {
        let temp = tempdir().unwrap();
        let tool = RuntimeSurfaceTool::new(Arc::new(EnvManager::new(temp.path().join("runtimes"))));

        let out = tool
            .call(
                r#"{"action":"provision_env","id":"tv-test","dependencies":["rich"],"use_browser":false}"#,
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            value["verification_preview"]["mode"],
            "ExecutionResultCheck"
        );
        assert!(value["verification_preview"]["execution_evidence"][0]
            .as_str()
            .unwrap_or_default()
            .contains("provisioned managed env id=tv-test"));
    }
}
