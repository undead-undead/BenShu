use async_trait::async_trait;
use benshu_compression::{
    compress_command_output, interpret_command_outcome, CommandCompressionResult,
};
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};
use benshu_security::{sandbox::GLOBAL_DETECTOR, LeakAction, ShellFirewall};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const MAX_SCRIPT_CHARS: usize = 16_000;
const MAX_OUTPUT_CHARS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsControlProviderKind {
    WindowsNative,
    WslTestBridge,
}

impl WindowsControlProviderKind {
    fn label(self) -> &'static str {
        match self {
            Self::WindowsNative => "windows_native",
            Self::WslTestBridge => "wsl_test_bridge",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::WindowsNative => "Windows native product runtime",
            Self::WslTestBridge => "WSL test bridge runtime",
        }
    }
}

#[derive(Debug, Clone)]
struct WindowsPowerShellProvider {
    executable: PathBuf,
    kind: WindowsControlProviderKind,
}

impl WindowsPowerShellProvider {
    fn descriptor(&self) -> serde_json::Value {
        json!({
            "kind": self.kind.label(),
            "description": self.kind.description(),
            "executable": self.executable.display().to_string(),
            "semantic_layer": "windows_control",
            "transport": "powershell"
        })
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WindowsControlAction {
    Probe,
    SystemInfo,
    ListProcesses,
    ListServices,
    ListDrives,
    ListEnvironment,
    RunPowerShell,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WindowsControlAccess {
    ReadOnly,
    Mutating,
}

fn default_action() -> WindowsControlAction {
    WindowsControlAction::Probe
}

fn default_access() -> WindowsControlAccess {
    WindowsControlAccess::ReadOnly
}

fn default_timeout_secs() -> u64 {
    30
}

#[derive(Debug, Deserialize)]
struct WindowsControlArgs {
    #[serde(default = "default_action")]
    action: WindowsControlAction,
    #[serde(default)]
    script: Option<String>,
    #[serde(default = "default_access")]
    access: WindowsControlAccess,
    #[serde(default)]
    confirm_mutation: bool,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    include_sensitive: bool,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

pub struct WindowsControlTool {
    workspace: PathBuf,
}

impl WindowsControlTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    fn is_wsl() -> bool {
        std::fs::read_to_string("/proc/version")
            .map(|content| content.to_ascii_lowercase().contains("microsoft"))
            .unwrap_or(false)
    }

    fn resolve_provider() -> Option<WindowsPowerShellProvider> {
        if let Ok(path) = std::env::var("BENSHU_WINDOWS_POWERSHELL") {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                let executable = PathBuf::from(trimmed);
                if executable.is_file() {
                    return Some(WindowsPowerShellProvider {
                        executable,
                        kind: if cfg!(target_os = "windows") {
                            WindowsControlProviderKind::WindowsNative
                        } else {
                            WindowsControlProviderKind::WslTestBridge
                        },
                    });
                }
            }
        }

        if cfg!(target_os = "windows") {
            if let Ok(path) = which::which("powershell.exe").or_else(|_| which::which("powershell"))
            {
                return Some(WindowsPowerShellProvider {
                    executable: path,
                    kind: WindowsControlProviderKind::WindowsNative,
                });
            }
            return None;
        }

        if Self::is_wsl() {
            for candidate in [
                "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
                "/mnt/c/Windows/SysWOW64/WindowsPowerShell/v1.0/powershell.exe",
            ] {
                let path = PathBuf::from(candidate);
                if path.is_file() {
                    return Some(WindowsPowerShellProvider {
                        executable: path,
                        kind: WindowsControlProviderKind::WslTestBridge,
                    });
                }
            }
            if let Ok(path) = which::which("powershell.exe") {
                return Some(WindowsPowerShellProvider {
                    executable: path,
                    kind: WindowsControlProviderKind::WslTestBridge,
                });
            }
        }

        None
    }

    fn wsl_path_to_windows_path(path: &Path) -> Option<String> {
        let path_str = path.to_str()?;
        let remainder = path_str.strip_prefix("/mnt/")?;
        let (drive, rest) = remainder.split_once('/')?;
        if drive.len() != 1 || !drive.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return None;
        }
        Some(format!(
            r"{}:\{}",
            drive.to_ascii_uppercase(),
            rest.replace('/', "\\")
        ))
    }

    fn looks_like_windows_absolute_path(value: &str) -> bool {
        let bytes = value.as_bytes();
        (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/'))
            || value.starts_with(r"\\")
    }

    fn normalize_working_dir(
        &self,
        provider: &WindowsPowerShellProvider,
        candidate: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        let Some(candidate) = candidate.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };

        if provider.kind == WindowsControlProviderKind::WslTestBridge
            && Self::looks_like_windows_absolute_path(candidate)
        {
            return Ok(Some(candidate.replace('/', "\\")));
        }

        let path =
            if candidate.starts_with('/') || Self::looks_like_windows_absolute_path(candidate) {
                PathBuf::from(candidate)
            } else {
                self.workspace.join(candidate)
            };

        if provider.kind == WindowsControlProviderKind::WslTestBridge {
            return Self::wsl_path_to_windows_path(&path).map(Some).ok_or_else(|| {
                anyhow::anyhow!(
                    "working_dir '{}' is not addressable from the Windows WSL bridge; use /mnt/<drive>/... or omit working_dir",
                    candidate
                )
            });
        }

        Ok(Some(path.display().to_string()))
    }

    fn probe_script() -> String {
        r#"
$result = [ordered]@{
  computer_name = $env:COMPUTERNAME
  username = $env:USERNAME
  user_domain = $env:USERDOMAIN
  powershell_version = $PSVersionTable.PSVersion.ToString()
  edition = $PSVersionTable.PSEdition
  os = [System.Environment]::OSVersion.VersionString
  process_architecture = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
  current_directory = (Get-Location).Path
}
$result | ConvertTo-Json -Depth 4
"#
        .trim()
        .to_string()
    }

    fn single_quoted_ps_literal(value: &str) -> String {
        format!("'{}'", Self::escape_single_quoted(value))
    }

    fn bounded_limit(limit: Option<u32>) -> u32 {
        limit.unwrap_or(50).clamp(1, 200)
    }

    fn optional_filter_predicate(filter: Option<&str>, fields: &[&str]) -> String {
        let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
            return String::new();
        };
        let literal = Self::single_quoted_ps_literal(&format!("*{filter}*"));
        let checks = fields
            .iter()
            .map(|field| format!("$_.{field} -like {literal}"))
            .collect::<Vec<_>>()
            .join(" -or ");
        format!(" | Where-Object {{ {checks} }}")
    }

    fn system_info_script() -> String {
        r#"
$os = $null
$computer = $null
try { $os = Get-CimInstance Win32_OperatingSystem } catch {}
try { $computer = Get-CimInstance Win32_ComputerSystem } catch {}
$result = [ordered]@{
  computer_name = $env:COMPUTERNAME
  username = $env:USERNAME
  powershell_version = $PSVersionTable.PSVersion.ToString()
  edition = $PSVersionTable.PSEdition
  os_caption = if ($os) { $os.Caption } else { [System.Environment]::OSVersion.VersionString }
  os_version = if ($os) { $os.Version } else { [System.Environment]::OSVersion.Version.ToString() }
  architecture = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
  total_physical_memory = if ($computer) { [int64]$computer.TotalPhysicalMemory } else { $null }
  current_directory = (Get-Location).Path
}
$result | ConvertTo-Json -Depth 4
"#
        .trim()
        .to_string()
    }

    fn list_processes_script(limit: Option<u32>, filter: Option<&str>) -> String {
        let filter = Self::optional_filter_predicate(filter, &["ProcessName", "Path"]);
        format!(
            "Get-Process{filter} | Sort-Object ProcessName | Select-Object -First {} Id,ProcessName,CPU,WorkingSet64,Path | ConvertTo-Json -Depth 4",
            Self::bounded_limit(limit)
        )
    }

    fn list_services_script(limit: Option<u32>, filter: Option<&str>) -> String {
        let filter = Self::optional_filter_predicate(filter, &["Name", "DisplayName", "Status"]);
        format!(
            "Get-Service{filter} | Sort-Object Name | Select-Object -First {} Name,DisplayName,Status,StartType | ConvertTo-Json -Depth 4",
            Self::bounded_limit(limit)
        )
    }

    fn list_drives_script() -> String {
        "Get-PSDrive -PSProvider FileSystem | Select-Object Name,Root,Description,Used,Free | ConvertTo-Json -Depth 4".to_string()
    }

    fn list_environment_script(
        limit: Option<u32>,
        filter: Option<&str>,
        include_sensitive: bool,
    ) -> String {
        let filter = Self::optional_filter_predicate(filter, &["Name", "Value"]);
        let sensitive_filter = if include_sensitive {
            String::new()
        } else {
            " | Where-Object { $_.Name -notmatch '(?i)(key|token|secret|password|passwd|credential|auth)' }".to_string()
        };
        format!(
            "Get-ChildItem Env:{filter}{sensitive_filter} | Sort-Object Name | Select-Object -First {} Name,Value | ConvertTo-Json -Depth 4",
            Self::bounded_limit(limit)
        )
    }

    fn escape_single_quoted(text: &str) -> String {
        text.replace('\'', "''")
    }

    fn wrap_script(script: &str, working_dir: Option<&str>) -> String {
        let mut wrapped = String::from("$ErrorActionPreference = 'Stop'\n");
        if let Some(dir) = working_dir {
            wrapped.push_str("Set-Location -LiteralPath '");
            wrapped.push_str(&Self::escape_single_quoted(dir));
            wrapped.push_str("'\n");
        }
        wrapped.push_str(script);
        wrapped
    }

    fn validate_script(script: &str, access: WindowsControlAccess) -> anyhow::Result<()> {
        if script.trim().is_empty() {
            anyhow::bail!("PowerShell script is required");
        }
        if script.len() > MAX_SCRIPT_CHARS {
            anyhow::bail!(
                "PowerShell script is larger than the {} character safety limit",
                MAX_SCRIPT_CHARS
            );
        }

        ShellFirewall::enforce(script).map_err(|reason| anyhow::anyhow!(reason))?;

        if access == WindowsControlAccess::ReadOnly {
            let lowered = script.to_ascii_lowercase();
            let mutating_markers = [
                " set-",
                "\nset-",
                "new-",
                "remove-",
                "rename-",
                "move-",
                "copy-",
                "clear-",
                "start-",
                "stop-",
                "restart-",
                "invoke-webrequest",
                "invoke-restmethod",
                "out-file",
                "add-content",
                "set-content",
                "export-",
                "install-",
                "uninstall-",
                "register-",
                "unregister-",
                "set-itemproperty",
                "new-itemproperty",
                "remove-itemproperty",
            ];
            if let Some(marker) = mutating_markers.iter().find(|marker| {
                let marker = **marker;
                let trimmed = marker.trim();
                lowered.contains(marker) || lowered.starts_with(trimmed)
            }) {
                anyhow::bail!(
                    "read_only Windows control blocked mutating PowerShell marker '{}'; set access='mutating' with confirm_mutation=true when the user explicitly asked for this side effect",
                    marker.trim()
                );
            }
        }

        Ok(())
    }

    fn action_label(action: WindowsControlAction) -> &'static str {
        match action {
            WindowsControlAction::Probe => "probe",
            WindowsControlAction::SystemInfo => "system_info",
            WindowsControlAction::ListProcesses => "list_processes",
            WindowsControlAction::ListServices => "list_services",
            WindowsControlAction::ListDrives => "list_drives",
            WindowsControlAction::ListEnvironment => "list_environment",
            WindowsControlAction::RunPowerShell => "run_powershell",
        }
    }

    fn script_for_action(args: &WindowsControlArgs) -> anyhow::Result<String> {
        match args.action {
            WindowsControlAction::Probe => Ok(Self::probe_script()),
            WindowsControlAction::SystemInfo => Ok(Self::system_info_script()),
            WindowsControlAction::ListProcesses => Ok(Self::list_processes_script(
                args.limit,
                args.filter.as_deref(),
            )),
            WindowsControlAction::ListServices => Ok(Self::list_services_script(
                args.limit,
                args.filter.as_deref(),
            )),
            WindowsControlAction::ListDrives => Ok(Self::list_drives_script()),
            WindowsControlAction::ListEnvironment => Ok(Self::list_environment_script(
                args.limit,
                args.filter.as_deref(),
                args.include_sensitive,
            )),
            WindowsControlAction::RunPowerShell => Ok(args.script.clone().unwrap_or_default()),
        }
    }

    fn shape_output(command: &str, input: &str) -> CommandCompressionResult {
        compress_command_output(command, input, MAX_OUTPUT_CHARS)
    }

    fn sanitize_output(output: &str) -> (String, Vec<serde_json::Value>) {
        let (mut redacted, detections) = GLOBAL_DETECTOR.redact(output);
        for detection in &detections {
            if detection.action != LeakAction::Redact && !detection.redacted_value.is_empty() {
                redacted = redacted.replace(
                    &detection.redacted_value,
                    &format!("[REDACTED:{}]", detection.pattern_name),
                );
            }
        }

        let diagnostics = detections
            .into_iter()
            .map(|detection| {
                json!({
                    "code": "windows_control.output_secret_redacted",
                    "pattern": detection.pattern_name,
                    "action": match detection.action {
                        LeakAction::Block => "block",
                        LeakAction::Redact => "redact",
                        LeakAction::Warn => "warn",
                    }
                })
            })
            .collect();
        (redacted, diagnostics)
    }

    fn parse_structured_stdout(stdout: &str) -> Option<serde_json::Value> {
        serde_json::from_str(stdout.trim()).ok()
    }

    async fn execute_powershell(
        provider: &WindowsPowerShellProvider,
        script: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<std::process::Output> {
        let mut command = Command::new(&provider.executable);
        command
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("RemoteSigned")
            .arg("-Command")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(script.as_bytes()).await?;
            stdin.shutdown().await?;
        }

        tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs.clamp(1, 300)),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("windows_control timed out after {timeout_secs}s"))?
        .map_err(Into::into)
    }
}

#[async_trait]
impl Tool for WindowsControlTool {
    fn name(&self) -> String {
        "windows_control".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Control or inspect the Windows native environment through a guarded PowerShell provider. Supports Windows-native execution and the WSL test bridge, with provider profile evidence in every result.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["probe", "system_info", "list_processes", "list_services", "list_drives", "list_environment", "run_powershell"],
                        "description": "Prefer structured read-only actions. Use run_powershell only when no narrower action fits."
                    },
                    "script": {
                        "type": "string",
                        "description": "PowerShell script for run_powershell. Defaults to a safe probe script for probe."
                    },
                    "access": {
                        "type": "string",
                        "enum": ["read_only", "mutating"],
                        "description": "read_only blocks obvious side-effect cmdlets. mutating requires confirm_mutation=true."
                    },
                    "confirm_mutation": {
                        "type": "boolean",
                        "description": "Must be true when access='mutating'. The user request must explicitly require the side effect."
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Optional working directory. Under WSL bridge this must be addressable as /mnt/<drive>/..."
                    },
                    "filter": {
                        "type": "string",
                        "description": "Optional text filter for list_processes, list_services, or list_environment."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "description": "Optional max records for list actions. Defaults to 50."
                    },
                    "include_sensitive": {
                        "type": "boolean",
                        "description": "Only for list_environment. Defaults false and hides variables with sensitive-looking names."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 300,
                        "description": "Execution timeout in seconds. Defaults to 30."
                    }
                },
                "required": ["action"]
            }),
            parameters_ts: Some(
                "interface WindowsControlArgs {\n  action: 'probe' | 'system_info' | 'list_processes' | 'list_services' | 'list_drives' | 'list_environment' | 'run_powershell';\n  script?: string;\n  access?: 'read_only' | 'mutating';\n  confirm_mutation?: boolean;\n  working_dir?: string;\n  filter?: string;\n  limit?: number;\n  include_sensitive?: boolean;\n  timeout_secs?: number;\n}"
                    .to_string(),
            ),
            is_binary: false,
            is_verified: true,
            safety_level: SafetyLevel::Red,
            usage_guidelines: Some(
                "Use only from a Windows/native-control worker. Prefer action='probe' or read_only scripts for inspection. Do not use for arbitrary code execution when a narrower tool exists. Mutating actions require an explicit user request and confirm_mutation=true."
                    .to_string(),
            ),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: WindowsControlArgs = serde_json::from_str(arguments)?;
        let provider = Self::resolve_provider()
            .ok_or_else(|| anyhow::anyhow!("No Windows PowerShell provider found"))?;

        if args.access == WindowsControlAccess::Mutating && !args.confirm_mutation {
            anyhow::bail!(
                "mutating Windows control requires confirm_mutation=true and an explicit user request"
            );
        }
        if !matches!(args.action, WindowsControlAction::RunPowerShell)
            && args.access == WindowsControlAccess::Mutating
        {
            anyhow::bail!("structured windows_control actions are read-only; use run_powershell for explicit mutating work");
        }

        let base_script = Self::script_for_action(&args)?;
        Self::validate_script(&base_script, args.access)?;
        let working_dir = self.normalize_working_dir(&provider, args.working_dir.as_deref())?;
        let script = Self::wrap_script(&base_script, working_dir.as_deref());

        let output = Self::execute_powershell(&provider, &script, args.timeout_secs).await?;
        let raw_stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let raw_stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let (stdout, stdout_diagnostics) = Self::sanitize_output(&raw_stdout);
        let (stderr, stderr_diagnostics) = Self::sanitize_output(&raw_stderr);
        let stdout_shaped = Self::shape_output("windows_control", &stdout);
        let stderr_shaped = Self::shape_output("windows_control", &stderr);
        let outcome = interpret_command_outcome(
            "windows_control",
            output.status.code(),
            output.status.success(),
            &stdout,
            &stderr,
        );
        let mut diagnostics = vec![json!({
            "code": "windows_control.provider_profile",
            "profile": provider.kind.label(),
            "description": provider.kind.description(),
        })];
        diagnostics.extend(stdout_diagnostics);
        diagnostics.extend(stderr_diagnostics);
        if provider.kind == WindowsControlProviderKind::WslTestBridge {
            diagnostics.push(json!({
                "code": "windows_control.dev_wsl_bridge",
                "severity": "info",
                "message": "WSL bridge is a development/test provider profile, not the Windows native product path."
            }));
        }

        Ok(serde_json::to_string_pretty(&json!({
            "provider": provider.descriptor(),
            "working_dir": working_dir,
            "action": Self::action_label(args.action),
            "access": match args.access {
                WindowsControlAccess::ReadOnly => "read_only",
                WindowsControlAccess::Mutating => "mutating",
            },
            "status": output.status.code().unwrap_or_default(),
            "raw_status_success": output.status.success(),
            "success": outcome.success,
            "outcome_kind": outcome.kind,
            "outcome_summary": outcome.summary,
            "stdout": stdout_shaped.content,
            "stderr": stderr_shaped.content,
            "stdout_truncated": stdout_shaped.truncated,
            "stderr_truncated": stderr_shaped.truncated,
            "structured_stdout": Self::parse_structured_stdout(&stdout),
            "diagnostics": diagnostics,
            "receipt": {
                "schema_version": "benshu.windows_control.receipt.v1",
                "kind": "windows_control",
                "provider_profile": provider.kind.label(),
                "transport": "powershell",
                "action": Self::action_label(args.action),
                "access": match args.access {
                    WindowsControlAccess::ReadOnly => "read_only",
                    WindowsControlAccess::Mutating => "mutating",
                },
                "side_effect_class": match args.access {
                    WindowsControlAccess::ReadOnly => "read_only_inspection",
                    WindowsControlAccess::Mutating => "host_mutation",
                },
                "timeout_secs": args.timeout_secs.clamp(1, 300),
                "status": if outcome.success { "completed" } else { "failed" }
            }
        }))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn windows_control_definition_exposes_guarded_actions() {
        let tool = WindowsControlTool::new(PathBuf::from("."));
        let def = tool.definition().await;
        let actions = def.parameters["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            vec![
                "probe",
                "system_info",
                "list_processes",
                "list_services",
                "list_drives",
                "list_environment",
                "run_powershell"
            ]
        );
        assert_eq!(def.safety_level, SafetyLevel::Red);
    }

    #[test]
    fn read_only_script_blocks_obvious_mutation() {
        let err = WindowsControlTool::validate_script(
            "Remove-Item -LiteralPath C:\\temp\\x.txt",
            WindowsControlAccess::ReadOnly,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("read_only Windows control blocked"));
    }

    #[test]
    fn wsl_path_conversion_handles_drive_mounts() {
        let converted =
            WindowsControlTool::wsl_path_to_windows_path(Path::new("/mnt/c/Users/example/AppData"));
        assert_eq!(converted.as_deref(), Some(r"C:\Users\example\AppData"));
    }

    #[test]
    fn list_environment_excludes_sensitive_names_by_default() {
        let script = WindowsControlTool::list_environment_script(None, None, false);
        assert!(script.contains("key|token|secret|password"));
    }

    #[test]
    fn output_sanitizer_redacts_detected_tokens() {
        let (redacted, diagnostics) =
            WindowsControlTool::sanitize_output("token=sk-abcdefghijklmnopqrstuvwxyz");
        assert!(!redacted.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(!diagnostics.is_empty());
    }

    #[tokio::test]
    async fn probe_call_returns_receipt_when_provider_available() {
        if WindowsControlTool::resolve_provider().is_none() {
            return;
        }

        let tool = WindowsControlTool::new(PathBuf::from("."));
        let result = tool
            .call(r#"{"action":"probe","timeout_secs":30}"#)
            .await
            .expect("probe call succeeds");
        let payload: serde_json::Value = serde_json::from_str(&result).expect("json payload");

        assert_eq!(payload["action"], "probe");
        assert_eq!(payload["access"], "read_only");
        assert_eq!(
            payload["receipt"]["schema_version"],
            "benshu.windows_control.receipt.v1"
        );
        assert!(payload["provider"]["kind"].is_string());
    }
}
