use crate::{LeakDetector, ShellFirewall};
use async_trait::async_trait;
use benshu_infra::error::{Error, Result};
use benshu_infra::skill::{SkillExecutionConfig, CURRENT_SECURITY, CURRENT_WORKSPACES};
use benshu_infra::traits::env::SystemEnvironment;
use benshu_infra::traits::resource::ThrottleLevel;
use benshu_infra::traits::runtime::SkillRuntime;
use benshu_infra::traits::security::DynamicPolicy;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::time::SystemTime;
use tokio::process::Command;
use tracing::{debug, info, warn};

pub static GLOBAL_DETECTOR: Lazy<crate::LeakDetector> = Lazy::new(|| crate::LeakDetector::new());
pub static GLOBAL_POLICY_GUARD: Lazy<crate::PolicyGuard> = Lazy::new(|| {
    let base_dir = std::env::var("BENSHU_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
    crate::PolicyGuard::new(&base_dir)
});

#[derive(Debug, Clone, Serialize)]
pub struct ActiveSandboxContext {
    pub pid: u32,
    pub tool_name: String,
    pub interpreter: String,
    pub started_at: SystemTime,
    pub sandbox_engine: String,
    pub isolation_state: String,
}

pub static ACTIVE_SANDBOXES: Lazy<DashMap<u32, ActiveSandboxContext>> = Lazy::new(DashMap::new);

#[cfg(target_os = "windows")]
mod windows_sandbox {
    use std::mem::size_of;
    use std::ptr::null;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::JobObjects::*;

    pub struct JobObject(HANDLE);

    impl JobObject {
        pub fn create(
            config: &benshu_infra::skill::SkillExecutionConfig,
            policy: &benshu_infra::traits::security::DynamicPolicy,
        ) -> Option<Self> {
            unsafe {
                let handle = CreateJobObjectW(null(), null());
                if handle == 0 {
                    return None;
                }

                // 1. Set standard limits: Absolute kill on close, memory limits, and process limits
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
                    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                    | JOB_OBJECT_LIMIT_PRIORITY_CLASS;

                // Prevent fork bombs: limit varies by throttle level.
                // If binary execution is blocked, limit to exactly 1 process (the script itself).
                info.BasicLimitInformation.ActiveProcessLimit = if !policy.allow_binary_exec {
                    1
                } else {
                    match config.throttle {
                        benshu_infra::traits::resource::ThrottleLevel::High => 50,
                        benshu_infra::traits::resource::ThrottleLevel::Medium => 20,
                        benshu_infra::traits::resource::ThrottleLevel::Low => 5,
                    }
                };

                // Phase 7.2: Resource Throttling - Map levels to Windows Priority Classes
                info.BasicLimitInformation.PriorityClass = match config.throttle {
                    benshu_infra::traits::resource::ThrottleLevel::High => 0x00000020, // NORMAL_PRIORITY_CLASS
                    benshu_infra::traits::resource::ThrottleLevel::Medium => 0x00004000, // BELOW_NORMAL_PRIORITY_CLASS
                    benshu_infra::traits::resource::ThrottleLevel::Low => 0x00000040, // IDLE_PRIORITY_CLASS
                };

                if let Some(mem_mb) = config.max_memory_mb {
                    info.BasicLimitInformation.LimitFlags |=
                        JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
                    let bytes = mem_mb as usize * 1024 * 1024;
                    info.JobMemoryLimit = bytes;
                    info.ProcessMemoryLimit = bytes;
                }

                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );

                // 2. Set UI restrictions: No clipboard access, no desktop switching
                let mut ui_info: JOBOBJECT_BASIC_UI_RESTRICTIONS = std::mem::zeroed();
                ui_info.UIRestrictionsClass = JOB_OBJECT_UILIMIT_READCLIPBOARD
                    | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
                    | JOB_OBJECT_UILIMIT_HANDLES
                    | JOB_OBJECT_UILIMIT_GLOBALATOMS
                    | JOB_OBJECT_UILIMIT_EXITWINDOWS
                    | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS;

                SetInformationJobObject(
                    handle,
                    JobObjectBasicUIRestrictions,
                    &ui_info as *const _ as *const _,
                    size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
                );

                // 3. Set CPU rate control if requested
                if let Some(cpu_percent) = config.max_cpu_percent {
                    let mut cpu_info: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = std::mem::zeroed();
                    cpu_info.ControlFlags =
                        JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                    cpu_info.Anonymous.CpuRate = cpu_percent as u32 * 100; // 100% = 10,000

                    SetInformationJobObject(
                        handle,
                        JobObjectCpuRateControlInformation,
                        &cpu_info as *const _ as *const _,
                        size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                    );
                }

                // 4. Set Network rate control
                // If policy blocks network, we set a near-zero bandwidth (1kbps) as a hard block
                if policy.block_network || !config.allow_network {
                    let mut net_info: JOBOBJECT_NET_RATE_CONTROL_INFORMATION = std::mem::zeroed();
                    net_info.ControlFlags = JOB_OBJECT_NET_RATE_CONTROL_ENABLE
                        | JOB_OBJECT_NET_RATE_CONTROL_MAX_BANDWIDTH;
                    net_info.MaxBandwidth = 1000; // 1 kbps - effectively unusable for network exfiltration

                    SetInformationJobObject(
                        handle,
                        JobObjectNetRateControlInformation,
                        &net_info as *const _ as *const _,
                        size_of::<JOBOBJECT_NET_RATE_CONTROL_INFORMATION>() as u32,
                    );
                } else if let Some(net_bps) = config.max_net_bps {
                    let mut net_info: JOBOBJECT_NET_RATE_CONTROL_INFORMATION = std::mem::zeroed();
                    net_info.ControlFlags = JOB_OBJECT_NET_RATE_CONTROL_ENABLE
                        | JOB_OBJECT_NET_RATE_CONTROL_MAX_BANDWIDTH;
                    net_info.MaxBandwidth = net_bps;

                    SetInformationJobObject(
                        handle,
                        JobObjectNetRateControlInformation,
                        &net_info as *const _ as *const _,
                        size_of::<JOBOBJECT_NET_RATE_CONTROL_INFORMATION>() as u32,
                    );
                }

                Some(Self(handle))
            }
        }

        pub fn assign(&self, process_handle: HANDLE) -> bool {
            unsafe { AssignProcessToJobObject(self.0, process_handle) != 0 }
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

pub struct NativeShellRuntime;

impl NativeShellRuntime {
    pub fn new() -> Self {
        Self
    }

    /// Layer 1 (Application): BenShu shell firewall.
    /// Runs BEFORE the OS sandbox — rejects obviously dangerous commands.
    fn pre_flight_firewall(arguments: &str, interpreter: &str) -> Result<()> {
        // Build a combined string for checking (interpreter + args together)
        let combined = format!("{} {}", interpreter, arguments);
        ShellFirewall::enforce(&combined).map_err(|reason| {
            warn!(
                interpreter = %interpreter,
                reason = %reason,
                "Pre-flight firewall blocked execution"
            );
            Error::ToolExecution {
                tool_name: "NativeShellRuntime::Firewall".to_string(),
                message: reason,
            }
        })
    }

    /// Layer 1b: Secret-in-args guard.
    ///
    /// CLI arguments are visible to all users via `ps aux`. Secrets (API keys,
    /// tokens, private keys) must NEVER be passed as command-line arguments.
    /// They must always be injected via environment variables through the Vault.
    fn check_args_for_secrets(arguments: &str) -> Result<()> {
        let (_redacted, detections) = GLOBAL_DETECTOR.redact(arguments);

        use crate::leaks::LeakAction;
        let hard_violations: Vec<_> = detections
            .iter()
            .filter(|d| matches!(d.action, LeakAction::Redact | LeakAction::Block))
            .collect();

        if !hard_violations.is_empty() {
            let names: Vec<&str> = hard_violations
                .iter()
                .map(|d| d.pattern_name.as_str())
                .collect();
            warn!(
                patterns = ?names,
                "SECURITY: Secret detected in skill CLI arguments — execution blocked. \
                 Use Vault env injection instead."
            );
            return Err(Error::ToolExecution {
                tool_name: "NativeShellRuntime::SecretGuard".to_string(),
                message: format!(
                    "Secrets must not be passed as CLI arguments (detected: {}). \
                     Inject secrets via environment variables through the Vault.",
                    names.join(", ")
                ),
            });
        }

        Ok(())
    }

    /// Layer 3 (Output): Secret leak scanner.
    /// Strips API keys / tokens from stdout/stderr before returning to caller.
    fn sanitize_output(stdout: Vec<u8>, stderr: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
        let stdout_str = String::from_utf8_lossy(&stdout);
        let (clean_stdout, stdout_detections) = GLOBAL_DETECTOR.redact(&stdout_str);

        let stderr_str = String::from_utf8_lossy(&stderr);
        let (clean_stderr, stderr_detections) = GLOBAL_DETECTOR.redact(&stderr_str);

        let total = stdout_detections.len() + stderr_detections.len();
        if total > 0 {
            warn!(
                count = total,
                "Secret leak scanner redacted {} potential secret(s) from skill output", total
            );
        }

        (clean_stdout.into_bytes(), clean_stderr.into_bytes())
    }

    /// Layer 1d: Path Boundary Check (Crucial for Windows)
    ///
    /// Since Windows Job Objects don't support filesystem virtualization, we manually
    /// verify that no arguments or paths attempt to escape the workspace or allowed paths.
    fn verify_path_bounds(arguments: &str, base_dir: &Path, policy: &DynamicPolicy) -> Result<()> {
        let abs_base = std::fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());

        // Scan for potential absolute paths (e.g. C:\ or / on Windows/GitBash)
        // We use a simple but effective heuristic: if it looks like a path and it's not relative to base.
        let parts: Vec<&str> = arguments.split_whitespace().collect();
        for part in parts {
            let p = Path::new(part);
            if p.is_absolute() {
                let is_in_base = p.starts_with(&abs_base);
                let is_in_allowed = policy.allowed_paths.iter().any(|ap| p.starts_with(ap));

                // Also check task-local trusted workspaces (flexible cfg from UI)
                let is_in_trusted = CURRENT_WORKSPACES
                    .try_with(|cw| cw.iter().any(|tw| p.starts_with(tw)))
                    .unwrap_or(false);

                if !is_in_base && !is_in_allowed && !is_in_trusted {
                    // Special case: allow common system binaries if they match what we expect
                    if p.starts_with("C:\\Windows\\System32") || p.starts_with("C:\\Program Files")
                    {
                        continue;
                    }

                    warn!(path = ?p, "SECURITY: Skill attempted to access path outside workspace on Windows.");
                    return Err(Error::ToolExecution {
                        tool_name: "WindowsPathGuard".into(),
                        message: format!(
                            "Forbidden path access: {}. Agent is restricted to the workspace.",
                            part
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    /// Layer 1c (macOS): Pre-flight TCC (Transparency, Consent, and Control) checks.
    ///
    /// Checks if the current process has enough permissions to avoid silent failures
    /// in the sandbox. This doesn't block execution but warns the user.
    #[cfg(target_os = "macos")]
    fn check_macos_tcc_permissions() {
        // Attempt to list a directory that requires Full Disk Access
        let tcc_db = Path::new("/Library/Application Support/com.apple.TCC");
        if !tcc_db.exists() {
            warn!(
                "SECURITY: BenShu may lack 'Full Disk Access' on macOS. \
                 Some sandboxed tools might fail to access required system resources."
            );
        }
    }

    /// Layer 2 (Kernel): Build the OS-native sandboxed subprocess command.
    fn build_os_sandboxed_command(
        &self,
        interpreter: &str,
        script_path: &Path,
        base_dir: &Path,
        arguments: &str,
        config: &SkillExecutionConfig,
        policy: &DynamicPolicy,
    ) -> Command {
        // Split interpreter into binary and extra args (e.g. "uv run" -> ["uv", "run"])
        let parts: Vec<&str> = interpreter.split_whitespace().collect();
        let binary = parts.first().cloned().unwrap_or("bash");
        let extra_args = if parts.len() > 1 { &parts[1..] } else { &[] };

        #[cfg(target_os = "linux")]
        {
            let unsafe_override = std::env::var("BENSHU_UNSAFE_SKILL_EXEC")
                .map(|v| v == "true")
                .unwrap_or(false);

            if unsafe_override {
                warn!("UNSAFE EXECUTION: bwrap bypassed via BENSHU_UNSAFE_SKILL_EXEC.");
                let mut c = Command::new(binary);
                c.args(extra_args).arg(script_path).arg(arguments);
                return c;
            }

            let mut c = Command::new("bwrap");
            c.arg("--ro-bind").arg("/").arg("/");
            c.arg("--dev").arg("/dev");
            c.arg("--proc").arg("/proc");
            c.arg("--tmpfs").arg("/tmp");

            let abs_base =
                std::fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());
            c.arg("--bind").arg(&abs_base).arg(&abs_base);

            // Dynamic Path Expansion (Phase 18.5)
            for path in &policy.allowed_paths {
                if path.exists() {
                    c.arg("--bind").arg(path).arg(path);
                }
            }

            if !config.allow_network && policy.block_network {
                c.arg("--unshare-net");
            }
            c.arg("--die-with-parent");

            use std::os::unix::process::CommandExt;
            let nice_val = match config.throttle {
                ThrottleLevel::High => 0,
                ThrottleLevel::Medium => 10,
                ThrottleLevel::Low => 19,
            };
            unsafe {
                c.pre_exec(move || {
                    libc::setpriority(libc::PRIO_PROCESS, 0, nice_val);
                    Ok(())
                });
            }

            c.arg("--")
                .arg(binary)
                .args(extra_args)
                .arg(script_path)
                .arg(arguments);
            c
        }

        #[cfg(target_os = "macos")]
        {
            let unsafe_override = std::env::var("BENSHU_UNSAFE_SKILL_EXEC")
                .map(|v| v == "true")
                .unwrap_or(false);

            if unsafe_override {
                let mut c = Command::new(binary);
                c.args(extra_args).arg(script_path).arg(arguments);
                return c;
            }

            let abs_base =
                std::fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());
            let base_str = abs_base.to_string_lossy();

            let mut profile = format!(
                r#"(version 1)
                   (allow default)
                   {network_policy}
                   (deny file-read* (subpath "/Library/Keychains"))
                   (deny file-read* (subpath "/Users/.*/Library/Keychains"))
                   (deny file-read* (subpath "/Users/.*/Library/Safari"))
                   (deny file-write*)
                   (allow file-write* (subpath "{base_str}"))
                   (allow file-write* (subpath "/tmp"))"#,
                network_policy = if config.allow_network && !policy.block_network {
                    "(allow network*)"
                } else {
                    "(deny network*)"
                },
                base_str = base_str
            );

            // Dynamic Path Expansion (Phase 18.5)
            for extra_path in &policy.allowed_paths {
                if extra_path.exists() {
                    let p_str = extra_path.to_string_lossy();
                    profile.push_str(&format!("\n(allow file* (subpath \"{}\"))", p_str));
                }
            }

            let mut c = Command::new("sandbox-exec");
            c.arg("-p").arg(profile);
            c.arg(binary)
                .args(extra_args)
                .arg(script_path)
                .arg(arguments);
            c
        }

        #[cfg(target_os = "windows")]
        {
            let mut c = Command::new(binary);
            c.args(extra_args).arg(script_path).arg(arguments);
            c
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            warn!("Unsupported OS. Running without OS sandboxing (firewall only).");
            let mut c = Command::new(binary);
            c.args(extra_args).arg(script_path).arg(arguments);
            c
        }
    }

    fn inject_env_vars(
        config: &SkillExecutionConfig,
        cmd: &mut Command,
        env_prefix: Option<&Path>,
        models_path: Option<&Path>,
    ) {
        use std::collections::HashMap;
        let mut final_env = HashMap::new();

        // 1. Inject infra/bin (Portable Toolchain) FIRST in PATH
        let mut path_entries = Vec::new();

        // Find the absolute infra/bin directory
        // In the security crate, we don't have direct access to EnvManager instance easily here,
        // but we can compute it from the process executable entry or env
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let infra_bin = exe_dir.join("infra").join("bin");
                if infra_bin.exists() {
                    path_entries.push(infra_bin.to_string_lossy().to_string());
                } else {
                    // Check if we are running in the 'data' structure
                    let local_infra_bin = exe_dir.join("data").join("infra").join("bin");
                    if local_infra_bin.exists() {
                        path_entries.push(local_infra_bin.to_string_lossy().to_string());
                    }
                }
            }
        }

        if let Some(prefix) = env_prefix {
            let bin = prefix.join("bin").to_string_lossy().to_string();
            path_entries.push(bin);
        }

        let old_path = std::env::var("PATH").unwrap_or_default();
        if !path_entries.is_empty() {
            let separator = if cfg!(windows) { ";" } else { ":" };
            path_entries.push(old_path);
            final_env.insert("PATH".to_string(), path_entries.join(separator));
        }

        if let Some(prefix) = env_prefix {
            final_env.insert(
                "CONDA_PREFIX".to_string(),
                prefix.to_string_lossy().to_string(),
            );
        }

        if let Some(mp) = models_path {
            final_env.insert(
                "BENSHU_MODELS_PATH".to_string(),
                mp.to_string_lossy().to_string(),
            );
        }

        for (key, value) in &final_env {
            cmd.env(key, value);
        }
    }
}

#[async_trait]
impl SkillRuntime for NativeShellRuntime {
    async fn execute(
        &self,
        metadata: &benshu_infra::skill::SkillMetadata,
        arguments: &str,
        base_dir: &Path,
        config: &SkillExecutionConfig,
        env_manager: Option<&std::sync::Arc<dyn SystemEnvironment>>,
    ) -> anyhow::Result<std::process::Output> {
        let mut interpreter = metadata.runtime.as_deref().unwrap_or("bash").to_string();

        // ─── Layer -0.5: Dynamic Security State ─────────────────────────────
        // Fetch real-time policy adjustments from the active security handler
        let dynamic_policy = CURRENT_SECURITY
            .try_with(|security| security.get_dynamic_policy())
            .unwrap_or_default();

        // Resolve script path
        let script_file = metadata
            .script
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No script defined for this skill"))?;
        let script_path = base_dir.join("scripts").join(script_file);

        // ─── Layer 0: Environment Provisioning (Pixi) ─────────────────────
        let mut env_prefix = None;
        let mut models_path = None;
        if let Some(em) = env_manager {
            if !metadata.dependencies.is_empty()
                || metadata.use_browser
                || metadata.runtime.as_deref() == Some("bash")
            {
                // If it's a bash skill on Windows, ensure we trigger provision to get m2-bash
                let mut deps = metadata.dependencies.clone();
                if metadata.runtime.as_deref() == Some("bash")
                    && !deps.contains(&"bash".to_string())
                {
                    deps.push("bash".to_string());
                }
                env_prefix = Some(
                    em.provision(&metadata.name, &deps, metadata.use_browser)
                        .await?,
                );
            }
            if !metadata.models.is_empty() {
                models_path = Some(
                    em.provision_models(&metadata.name, &metadata.models)
                        .await?,
                );
            }
        }

        // ─── Layer 0.5: Windows Mini-Bash & PowerShell First ──────────────
        #[cfg(target_os = "windows")]
        if interpreter == "bash" || interpreter == "sh" {
            // Priority 1: Check Portable Mini Git Bash (15MB version)
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    let portable_bash = exe_dir
                        .join("infra")
                        .join("bin")
                        .join("git-bash")
                        .join("bash.exe");
                    if portable_bash.exists() {
                        interpreter = portable_bash.to_string_lossy().to_string();
                    } else if let Ok(git_bash) = which::which("bash") {
                        // Priority 2: Check system PATH
                        interpreter = git_bash.to_string_lossy().to_string();
                    } else if let Some(prefix) = &env_prefix {
                        // Priority 3: Fallback to Pixi's MSYS bash (if available)
                        let msys_bash = prefix.join("Library").join("bin").join("bash.exe");
                        if msys_bash.exists() {
                            interpreter = msys_bash.to_string_lossy().to_string();
                        }
                    }
                }
            }
        } else if interpreter == "powershell" || (cfg!(windows) && interpreter == "shell") {
            // Priority 4: PowerShell bypass for native shell commands
            interpreter = "powershell.exe".to_string();
        }

        // ─── Layer 0.5: macOS TCC Pre-flight ──────────────────────────────
        #[cfg(target_os = "macos")]
        Self::check_macos_tcc_permissions();

        // ─── Layer 1: Application firewall (BenShu) ───────────────────────
        Self::pre_flight_firewall(arguments, &interpreter)?;

        // ─── Layer 1.05: Windows Path Guard ─────────────────────────────────
        #[cfg(target_os = "windows")]
        Self::verify_path_bounds(arguments, base_dir, &dynamic_policy)?;

        // ─── Layer 1.1: Wasm Policy Guard (Pre-Check) ───────────────────────
        // We use the global policy guard to run the Wasm-based security logic.
        // It provides a sandbox-aware decision on whether this call is safe.
        if let Err(e) = GLOBAL_POLICY_GUARD
            .pre_check(&metadata.name, arguments)
            .await
        {
            warn!(tool = %metadata.name, reason = %e, "Wasm Policy Guard blocked execution");
            return Err(anyhow::anyhow!(
                "PolicyGuard violation for {}: {}",
                metadata.name,
                e
            ));
        }

        // ─── Layer 1b: Secret-in-args guard ──────────────────────────────────
        Self::check_args_for_secrets(arguments)?;

        // ─── Layer 2: Kernel sandbox ─────────────────────────────────────────
        let mut cmd = self.build_os_sandboxed_command(
            &interpreter,
            &script_path,
            base_dir,
            arguments,
            config,
            &dynamic_policy,
        );
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.kill_on_drop(true);
        Self::inject_env_vars(
            config,
            &mut cmd,
            env_prefix.as_deref(),
            models_path.as_deref(),
        );

        debug!(
            interpreter = %interpreter,
            script = ?script_path,
            "NativeShellRuntime: spawning sandboxed process"
        );

        let child = cmd.spawn().map_err(|e| Error::ToolExecution {
            tool_name: "NativeShellRuntime".to_string(),
            message: format!("Failed to spawn process: {}", e),
        })?;

        #[cfg(target_os = "windows")]
        let (isolation_state, _job_handle) = {
            if let Some(job) = windows_sandbox::JobObject::create(config, &dynamic_policy) {
                match child.raw_handle() {
                    Some(process_handle) => {
                        let handle = process_handle as windows_sys::Win32::Foundation::HANDLE;
                        if !job.assign(handle) {
                            tracing::warn!(
                                "Failed to assign process to Job Object sandbox. Running with partial isolation."
                            );
                            ("partial".to_string(), None)
                        } else {
                            tracing::debug!(
                                "Process successfully locked in Windows Job Object with UI and resource limits."
                            );
                            ("hardened".to_string(), Some(job)) // Persist the job handle until wait completes
                        }
                    }
                    None => {
                        tracing::warn!(
                            "Spawned child did not expose a Windows process handle. Running without Job Object isolation."
                        );
                        ("degraded".to_string(), None)
                    }
                }
            } else {
                ("degraded".to_string(), None)
            }
        };
        #[cfg(not(target_os = "windows"))]
        let (isolation_state, _job_handle) = {
            if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
                ("hardened".to_string(), None::<()>)
            } else {
                ("degraded".to_string(), None::<()>)
            }
        };

        let child_id = child.id();
        let pid = child_id.unwrap_or(0);

        if pid > 0 {
            ACTIVE_SANDBOXES.insert(
                pid,
                ActiveSandboxContext {
                    pid,
                    tool_name: metadata.name.clone(),
                    interpreter: interpreter.to_string(),
                    started_at: SystemTime::now(),
                    sandbox_engine: if cfg!(target_os = "linux") {
                        "bwrap".to_string()
                    } else if cfg!(target_os = "macos") {
                        "sandbox-exec".to_string()
                    } else if cfg!(target_os = "windows") {
                        "job-objects".to_string()
                    } else {
                        "firewall-only".to_string()
                    },
                    isolation_state,
                },
            );
        }

        let timeout_duration = std::time::Duration::from_secs(config.timeout_secs);
        let wait_res = tokio::time::timeout(timeout_duration, child.wait_with_output()).await;

        // Ensure cleanup and killing on failure/timeout
        if pid > 0 {
            ACTIVE_SANDBOXES.remove(&pid);
        }

        let raw_output = match wait_res {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(anyhow::anyhow!("Process IO error: {}", e));
            }
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "Execution timed out after {}s",
                    config.timeout_secs
                ));
            }
        };

        // ─── Layer 3: Secret leak sanitizer ──────────────────────────────────
        let (clean_stdout, clean_stderr) =
            Self::sanitize_output(raw_output.stdout, raw_output.stderr);

        // ─── Layer 4: Wasm Policy Guard (Post-Filter) ───────────────────────
        let final_stdout = GLOBAL_POLICY_GUARD
            .post_filter(&String::from_utf8_lossy(&clean_stdout))
            .await
            .into_bytes();
        let final_stderr = GLOBAL_POLICY_GUARD
            .post_filter(&String::from_utf8_lossy(&clean_stderr))
            .await
            .into_bytes();

        Ok(std::process::Output {
            status: raw_output.status,
            stdout: final_stdout,
            stderr: final_stderr,
        })
    }
}
