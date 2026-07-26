use async_trait::async_trait;
use benshu_compression::{
    compress_command_output, interpret_command_outcome, CommandCompressionResult,
};
use benshu_infra::skill::{SkillExecutionConfig, SkillMetadata};
use benshu_infra::traits::runtime::SkillRuntime;
use benshu_infra::{Tool, ToolDefinition};
use benshu_security::sandbox::NativeShellRuntime;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

const MAX_OUTPUT_CHARS: usize = 2048;
const MAX_COMMAND_CHARS: usize = 16_000;

pub struct CommandExecTool {
    workspace: PathBuf,
}

impl CommandExecTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    fn validate_path(&self, candidate: &str) -> anyhow::Result<PathBuf> {
        let full_path = if candidate.starts_with('/') || (cfg!(windows) && candidate.contains(':'))
        {
            PathBuf::from(candidate)
        } else {
            self.workspace.join(candidate)
        };

        let normalized = if full_path.exists() {
            full_path.canonicalize()?
        } else if let Some(parent) = full_path.parent() {
            if parent.exists() {
                let canon_parent = parent.canonicalize()?;
                let name = full_path
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("Invalid working directory path"))?;
                canon_parent.join(name)
            } else {
                full_path
            }
        } else {
            full_path
        };

        let workspace_canon = if self.workspace.exists() {
            self.workspace
                .canonicalize()
                .unwrap_or_else(|_| self.workspace.clone())
        } else {
            self.workspace.clone()
        };

        if normalized.starts_with(&workspace_canon) {
            return Ok(normalized);
        }

        if let Ok(trusted) = benshu_brain::skills::CURRENT_WORKSPACES.try_with(|w| w.clone()) {
            for root in trusted {
                let root_canon = if root.exists() {
                    root.canonicalize().unwrap_or_else(|_| root.clone())
                } else {
                    root.clone()
                };
                if normalized.starts_with(&root_canon) {
                    return Ok(normalized);
                }
            }
        }

        anyhow::bail!(
            "Access Denied: working_dir '{}' is outside authorized workspaces.",
            candidate
        )
    }

    fn write_wrapper_script(
        &self,
        runtime: CommandRuntime,
        dir: &Path,
    ) -> anyhow::Result<(String, String)> {
        let scripts_dir = dir.join("scripts");
        std::fs::create_dir_all(&scripts_dir)?;

        let (file_name, runtime_name, content) = match runtime {
            CommandRuntime::PowerShell => (
                "command_exec.ps1",
                "powershell",
                r#"
param([string]$CommandLine)
$ErrorActionPreference = 'Stop'
Invoke-Expression $CommandLine
"#,
            ),
            CommandRuntime::Cmd => (
                "command_exec.cmd",
                "cmd",
                "@echo off\r\nsetlocal\r\ncmd.exe /d /s /c \"%~1\"\r\n",
            ),
            CommandRuntime::Bash => (
                "command_exec.sh",
                "bash",
                "#!/usr/bin/env bash\nset -euo pipefail\nbash -lc \"$1\"\n",
            ),
        };

        let script_path = scripts_dir.join(file_name);
        std::fs::write(&script_path, content)?;

        #[cfg(not(target_os = "windows"))]
        if matches!(runtime, CommandRuntime::Bash) {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        Ok((file_name.to_string(), runtime_name.to_string()))
    }

    fn shape_output(command: &str, input: &str) -> CommandCompressionResult {
        compress_command_output(command, input, MAX_OUTPUT_CHARS)
    }

    fn persist_raw_output_artifact(
        &self,
        working_dir: &Path,
        stream: &str,
        raw: &str,
        shaped: &CommandCompressionResult,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        if !shaped.truncated || raw.is_empty() {
            return Ok(None);
        }

        let artifact_dir = working_dir
            .join(".benshu")
            .join("tool-output")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&artifact_dir)?;

        let file_path = artifact_dir.join(format!("{stream}.txt"));
        std::fs::write(&file_path, raw)?;

        Ok(Some(json!({
            "kind": "command_output",
            "stream": stream,
            "uri": file_path.to_string_lossy().to_string(),
            "media_type": "text/plain",
            "original_chars": shaped.original_chars,
            "preview_chars": shaped.output_chars,
            "compression_mode": format!("{:?}", shaped.mode).to_lowercase(),
        })))
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CommandRuntime {
    PowerShell,
    Cmd,
    Bash,
}

#[derive(Debug, Deserialize)]
struct CommandExecArgs {
    command: String,
    #[serde(default = "default_runtime")]
    runtime: CommandRuntime,
    #[serde(default = "default_working_dir")]
    working_dir: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
    #[serde(default)]
    allow_network: bool,
}

fn default_runtime() -> CommandRuntime {
    if cfg!(windows) {
        CommandRuntime::PowerShell
    } else {
        CommandRuntime::Bash
    }
}

fn default_working_dir() -> String {
    ".".to_string()
}

fn default_timeout_secs() -> u64 {
    60
}

#[async_trait]
impl Tool for CommandExecTool {
    fn name(&self) -> String {
        "command_exec".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Execute a controlled local command via an explicit shell runtime. On Windows, prefer powershell or cmd; use bash only when the bundled/system bash surface is required.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The exact command line to execute."
                    },
                    "runtime": {
                        "type": "string",
                        "enum": ["powershell", "cmd", "bash"],
                        "description": "Which command runtime to use. Windows-first deployments should prefer powershell or cmd."
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Workspace-relative working directory. Defaults to '.'."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 300,
                        "description": "Execution timeout in seconds. Defaults to 60."
                    },
                    "allow_network": {
                        "type": "boolean",
                        "description": "Whether the command may use network access. Defaults to false."
                    }
                },
                "required": ["command", "runtime"]
            }),
            parameters_ts: Some(
                "interface CommandExec {\n  command: string;\n  runtime: 'powershell' | 'cmd' | 'bash';\n  working_dir?: string;\n  timeout_secs?: number;\n  allow_network?: boolean;\n}"
                    .to_string(),
            ),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some(
                "Use for explicit local command execution when a specialist needs PowerShell, cmd, or bash semantics. Prefer powershell on Windows unless cmd or bash is specifically required."
                    .to_string(),
            ),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: CommandExecArgs = serde_json::from_str(arguments)?;
        if args.command.len() > MAX_COMMAND_CHARS {
            anyhow::bail!(
                "command is larger than the {} character safety limit",
                MAX_COMMAND_CHARS
            );
        }
        let working_dir = self.validate_path(&args.working_dir)?;
        let (script_name, runtime_name) = self.write_wrapper_script(args.runtime, &working_dir)?;

        let metadata = SkillMetadata {
            name: "command_exec".to_string(),
            description: "Builtin command execution wrapper".to_string(),
            homepage: None,
            parameters: None,
            interface: None,
            script: Some(script_name),
            runtime: Some(runtime_name.clone()),
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

        let config = SkillExecutionConfig {
            timeout_secs: args.timeout_secs.clamp(1, 300),
            allow_network: args.allow_network,
            ..Default::default()
        };

        let runtime = NativeShellRuntime::new();
        let output = runtime
            .execute(&metadata, &args.command, &working_dir, &config, None)
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout_shaped = Self::shape_output(&args.command, &stdout);
        let stderr_shaped = Self::shape_output(&args.command, &stderr);
        let outcome = interpret_command_outcome(
            &args.command,
            output.status.code(),
            output.status.success(),
            &stdout,
            &stderr,
        );

        let mut evidence_artifacts = Vec::new();
        if let Some(artifact) =
            self.persist_raw_output_artifact(&working_dir, "stdout", &stdout, &stdout_shaped)?
        {
            evidence_artifacts.push(artifact);
        }
        if let Some(artifact) =
            self.persist_raw_output_artifact(&working_dir, "stderr", &stderr, &stderr_shaped)?
        {
            evidence_artifacts.push(artifact);
        }

        Ok(serde_json::to_string_pretty(&json!({
            "runtime": runtime_name,
            "working_dir": working_dir.display().to_string(),
            "status": output.status.code().unwrap_or_default(),
            "raw_status_success": output.status.success(),
            "success": outcome.success,
            "outcome_kind": outcome.kind,
            "outcome_summary": outcome.summary,
            "stdout": stdout_shaped.content,
            "stderr": stderr_shaped.content,
            "evidence_artifacts": evidence_artifacts,
        }))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn command_exec_definition_exposes_explicit_runtimes() {
        let temp = tempfile::tempdir().unwrap();
        let tool = CommandExecTool::new(temp.path().to_path_buf());
        let def = tool.definition().await;
        let runtimes = def.parameters["properties"]["runtime"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>();
        assert_eq!(runtimes, vec!["powershell", "cmd", "bash"]);
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn command_exec_runs_bash_command() {
        let temp = tempfile::tempdir().unwrap();
        let tool = CommandExecTool::new(temp.path().to_path_buf());
        let result = tool
            .call(
                r#"{
                    "command":"printf hello",
                    "runtime":"bash",
                    "working_dir":".",
                    "timeout_secs":5
                }"#,
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["success"], true);
        assert_eq!(value["stdout"], "hello");
        assert_eq!(value["outcome_kind"], "success");
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn command_exec_persists_long_output_artifact_when_compressed() {
        let temp = tempfile::tempdir().unwrap();
        let tool = CommandExecTool::new(temp.path().to_path_buf());
        let result = tool
            .call(
                r#"{
                    "command":"seq 1 5000",
                    "runtime":"bash",
                    "working_dir":".",
                    "timeout_secs":5
                }"#,
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        let artifacts = value["evidence_artifacts"].as_array().unwrap();
        assert!(!artifacts.is_empty());
        let uri = artifacts[0]["uri"].as_str().unwrap();
        assert!(std::path::Path::new(uri).exists());
    }
}
