use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::*;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiView};

/// Host state for Wasm execution
struct HostState {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

/// A high-performance Wasm runtime for agent skills.
/// Now uses Lazy Initialization to avoid startup cost.
#[derive(Clone)]
pub struct WasmRuntime {
    engine: Arc<OnceCell<Engine>>,
}

impl WasmRuntime {
    /// Create a new Wasm runtime handle.
    /// Does NOT initialize the engine yet (Lazy).
    pub fn new() -> Self {
        Self {
            engine: Arc::new(OnceCell::new()),
        }
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmRuntime {
    async fn get_engine(&self) -> anyhow::Result<&Engine> {
        self.engine
            .get_or_try_init(|| async {
                info!("Initializing WASM Engine (Lazy Loading)...");
                let mut config = Config::new();
                config.wasm_component_model(true);
                config.async_support(false);

                // Security: Enable Fuel for CPU limiting
                config.consume_fuel(true);

                let engine = Engine::new(&config)
                    .map_err(|e| anyhow::anyhow!("Failed to create Wasm engine: {}", e))?;

                info!("WASM Engine initialized successfully.");
                Ok(engine)
            })
            .await
    }

    /// Execute a Wasm skill
    pub async fn call(
        &self,
        wasm_path: &Path,
        arguments: &str,
        base_dir: &Path,
    ) -> anyhow::Result<std::process::Output> {
        self.call_with_contract(wasm_path, arguments, base_dir, "run", 128, 1024 * 1024)
            .await
    }

    async fn call_with_contract(
        &self,
        wasm_path: &Path,
        arguments: &str,
        base_dir: &Path,
        entrypoint: &str,
        memory_limit_mb: usize,
        max_output_bytes: usize,
    ) -> anyhow::Result<std::process::Output> {
        let engine = self.get_engine().await?;
        let wasm_path = wasm_path.to_path_buf();
        let arguments = arguments.to_string();
        let base_dir = base_dir.to_path_buf();
        let entrypoint = entrypoint.to_string();
        let engine = engine.clone();

        // Offload heavy Wasm execution to a blocking thread to avoid stalling the async runtime
        tokio::task::spawn_blocking(move || {
            Self::call_blocking(
                &engine,
                &wasm_path,
                &arguments,
                &base_dir,
                &entrypoint,
                memory_limit_mb,
                max_output_bytes,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("Wasm execution join error: {}", e))?
    }

    /// Blocking implementation of call
    fn call_blocking(
        engine: &Engine,
        wasm_path: &Path,
        arguments: &str,
        base_dir: &Path,
        entrypoint: &str,
        memory_limit_mb: usize,
        max_output_bytes: usize,
    ) -> anyhow::Result<std::process::Output> {
        use wasmtime_wasi::pipe::MemoryOutputPipe;

        let component = Component::from_file(engine, wasm_path)
            .map_err(|e| anyhow::anyhow!("Failed to load Wasm component: {}", e))?;

        let stdout = MemoryOutputPipe::new(max_output_bytes);
        let stderr = MemoryOutputPipe::new((max_output_bytes / 2).max(64 * 1024));

        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder.stdout(stdout.clone());
        wasi_builder.stderr(stderr.clone());

        // Security: WASI Directory Mapping
        wasi_builder
            .preopened_dir(base_dir, ".", DirPerms::all(), FilePerms::all())
            .map_err(|e| anyhow::anyhow!("Failed to mount base dir: {}", e))?;

        let wasi = wasi_builder.build();

        // Security: Memory Limits
        let limits = StoreLimitsBuilder::new()
            .memory_size(memory_limit_mb * 1024 * 1024)
            .instances(1)
            .tables(1)
            .memories(1)
            .build();

        let mut store = Store::new(
            engine,
            HostState {
                wasi_ctx: wasi,
                table: ResourceTable::new(),
                limits,
            },
        );

        // Security: CPU Limits (Fuel)
        store
            .set_fuel(500_000_000)
            .map_err(|e| anyhow::anyhow!("Failed to set fuel: {}", e))?;

        // Enforce memory limits
        store.limiter(|state| &mut state.limits);

        let mut linker = Linker::new(engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|e| anyhow::anyhow!("Failed to link WASI: {}", e))?;

        let instance = linker
            .instantiate(&mut store, &component)
            .map_err(|e| anyhow::anyhow!("Failed to instantiate Wasm component: {}", e))?;

        // Try to call run(input: string) -> string, or the manifest-defined entrypoint.
        let mut ok = true;
        if let Some(run_func) = instance.get_func(&mut store, entrypoint) {
            use wasmtime::component::Val;

            let mut results = [Val::Bool(false)];
            let args = [Val::String(arguments.to_string())];

            if let Err(e) = run_func.call(&mut store, &args, &mut results) {
                // Fallback to parameterless run()
                if let Err(e2) = run_func.call(&mut store, &[], &mut []) {
                    debug!("Wasm execution failed: {}, fallback failed: {}", e, e2);
                    ok = false;
                }
            }
        } else {
            return Err(anyhow::anyhow!(
                "Wasm component must export a '{}' function",
                entrypoint
            ));
        }

        // Drop store to flush pipes
        drop(store);

        let stdout_data = stdout.try_into_inner().unwrap_or_default().to_vec();
        let stderr_data = stderr.try_into_inner().unwrap_or_default().to_vec();

        Ok(std::process::Output {
            status: {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    std::process::ExitStatus::from_raw(if ok { 0 } else { 1 } << 8)
                }
                #[cfg(not(unix))]
                {
                    std::process::Command::new(if ok { "true" } else { "false" })
                        .status()
                        .unwrap_or_else(|_| {
                            std::process::Command::new("cmd")
                                .args(["/C", if ok { "exit 0" } else { "exit 1" }])
                                .status()
                                .unwrap()
                        })
                }
            },
            stdout: stdout_data,
            stderr: stderr_data,
        })
    }

    async fn verify_sha256(wasm_path: &Path, expected: Option<&str>) -> anyhow::Result<()> {
        let Some(expected) = expected else {
            return Ok(());
        };

        let bytes = tokio::fs::read(wasm_path).await?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(expected.trim()) {
            return Err(anyhow::anyhow!(
                "Wasm checksum mismatch for {:?}: expected {}, got {}",
                wasm_path,
                expected,
                actual
            ));
        }

        Ok(())
    }
}

#[async_trait]
impl crate::SkillRuntime for WasmRuntime {
    async fn execute(
        &self,
        metadata: &benshu_infra::skill::SkillMetadata,
        arguments: &str,
        base_dir: &Path,
        config: &benshu_infra::skill::SkillExecutionConfig,
        _env_manager: Option<&Arc<dyn benshu_infra::traits::env::SystemEnvironment>>,
    ) -> anyhow::Result<std::process::Output> {
        let wasm_file = metadata
            .script
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No wasm file defined for this skill"))?;
        let script_path = Path::new(wasm_file);
        if script_path.is_absolute()
            || script_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(anyhow::anyhow!(
                "Wasm script path must be relative and stay inside scripts/: {}",
                wasm_file
            ));
        }
        let wasm_path = base_dir.join("scripts").join(wasm_file);

        let contract = metadata.wasm.clone().unwrap_or_default();
        Self::verify_sha256(&wasm_path, contract.sha256.as_deref()).await?;

        if config.allow_network {
            warn!(
                tool = %metadata.name,
                "Ignoring network permission for Wasm skill; current Wasm protocol is local-only"
            );
        }

        benshu_security::sandbox::GLOBAL_POLICY_GUARD
            .pre_check(&metadata.name, arguments)
            .await?;

        let memory_limit_mb = config.max_memory_mb.unwrap_or(128).min(512);
        let max_output_bytes = config.max_output_bytes.min(10 * 1024 * 1024);
        let timeout_duration = std::time::Duration::from_secs(config.timeout_secs.max(1));

        let raw_output = tokio::time::timeout(
            timeout_duration,
            self.call_with_contract(
                &wasm_path,
                arguments,
                base_dir,
                &contract.entrypoint,
                memory_limit_mb,
                max_output_bytes,
            ),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("Wasm execution timed out after {}s", config.timeout_secs)
        })??;

        let stdout = String::from_utf8_lossy(&raw_output.stdout);
        let stderr = String::from_utf8_lossy(&raw_output.stderr);
        let final_stdout = benshu_security::sandbox::GLOBAL_POLICY_GUARD
            .post_filter(&stdout)
            .await
            .into_bytes();
        let final_stderr = benshu_security::sandbox::GLOBAL_POLICY_GUARD
            .post_filter(&stderr)
            .await
            .into_bytes();

        Ok(std::process::Output {
            status: raw_output.status,
            stdout: final_stdout,
            stderr: final_stderr,
        })
    }
}
