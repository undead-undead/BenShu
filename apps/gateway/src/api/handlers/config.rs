use crate::api::llama_cpp_runtime::{
    current_binary_dir, discover_supported_windows_llama_server, first_existing_path,
    running_under_wsl, runtime_discovery_roots, MIN_SUPPORTED_LLAMA_CPP_BUILD,
};
use crate::api::state::{AppError, AppState};
use axum::{extract::State, http::StatusCode, Json};
use benshu_brain::config::{
    ManagedRuntimeHostConfig, RuntimeHostControlConfig, WindowsMlBridgeBinding,
    WindowsMlBridgeConfig,
};
use benshu_inference::{
    build_effective_diagnostics_with_runtime, describe_local_model_contract,
    recommend_llama_cpp_runtime, resolve_llama_cpp_reasoning_compatibility,
    should_apply_llama_cpp_auto_recommendation, summarize_hardware, HardwareStatus,
    LlamaCppRuntimeInput,
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct ConfigUpdateResult {
    pub saved: bool,
    pub main_brain_restart_needed: bool,
    pub windows_ml_restart_needed: bool,
    pub main_brain_restart_requested: bool,
    pub windows_ml_restart_requested: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContinuationRuntimeStatus {
    pub disk_cache_enabled: bool,
    pub cache_dir: String,
    pub cache_budget_mb: u64,
    pub cache_max_entries: u32,
    pub disable_disk_cache_for_sensitive_tasks: bool,
    pub cleanup_allowed: bool,
    pub index_present: bool,
    pub entries_dir_present: bool,
    pub entry_file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContinuationCacheCleanupRequest {
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContinuationCacheCleanupReport {
    pub dry_run: bool,
    pub cache_dir: String,
    pub scanned: usize,
    pub deleted: usize,
    pub bytes_matched: u64,
    pub bytes_deleted: u64,
    pub cleanup_allowed: bool,
    pub skipped_reason: Option<String>,
}

fn sanitize_bridge_alias(model: &str) -> String {
    let fallback = "image-model";
    let stem = Path::new(model)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback);
    let alias = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let compact = alias
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if compact.is_empty() {
        fallback.to_string()
    } else {
        compact
    }
}

fn continuation_cache_dir(state: &AppState) -> PathBuf {
    let config = state.app_config.read();
    if let Some(cache_dir) = config.continuation_runtime.cache_dir.clone() {
        if cache_dir.is_absolute() {
            cache_dir
        } else {
            state.kernel.base_dir().join(cache_dir)
        }
    } else {
        state
            .kernel
            .base_dir()
            .join("runtime")
            .join("continuation-cache")
    }
}

fn path_has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn continuation_cache_cleanup_allowed(base_dir: &Path, cache_dir: &Path) -> bool {
    if path_has_parent_component(cache_dir) {
        return false;
    }
    let Ok(base) = std::fs::canonicalize(base_dir) else {
        return false;
    };
    let root = Path::new(std::path::MAIN_SEPARATOR_STR);
    if cache_dir == root || cache_dir == base {
        return false;
    }
    if let Ok(cache) = std::fs::canonicalize(cache_dir) {
        cache.starts_with(&base) && cache != base
    } else {
        cache_dir.starts_with(&base) && cache_dir != base
    }
}

fn scan_cache_dir(path: &Path) -> (usize, u64) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return (0, 0);
    };
    if metadata.is_file() {
        return (1, metadata.len());
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return (0, 0);
    };
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let (entry_count, entry_bytes) = scan_cache_dir(&entry_path);
        count = count.saturating_add(entry_count);
        bytes = bytes.saturating_add(entry_bytes);
    }
    (count, bytes)
}

fn delete_cache_contents(path: &Path) -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return (0, 0);
    };
    let mut deleted = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let (entry_count, entry_bytes) = scan_cache_dir(&entry_path);
        let result = if entry_path.is_dir() {
            std::fs::remove_dir_all(&entry_path)
        } else {
            std::fs::remove_file(&entry_path)
        };
        if result.is_ok() {
            deleted = deleted.saturating_add(entry_count);
            bytes = bytes.saturating_add(entry_bytes);
        }
    }
    (deleted, bytes)
}

fn detect_wsl_windows_host() -> Option<String> {
    let output = std::process::Command::new("sh")
        .arg("-lc")
        .arg("ip route | awk '/default/ {print $3; exit}'")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let host = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn default_image_bridge_base_url(existing: Option<&str>) -> Option<String> {
    if let Ok(url) = std::env::var("BENSHU_WINDOWS_IMAGE_BRIDGE_BASE_URL") {
        if !url.trim().is_empty() {
            return Some(url);
        }
    }
    if let Some(existing) = existing {
        if !existing.trim().is_empty() {
            return Some(existing.to_string());
        }
    }
    detect_wsl_windows_host().map(|host| format!("http://{host}:8022/v1"))
}

fn build_windows_ml_binding(
    role: &str,
    source_model: &str,
    runtime_target: &str,
    execution_provider: &str,
    image_bridge_base_url: Option<&str>,
) -> Option<WindowsMlBridgeBinding> {
    let trimmed = source_model.trim();
    if trimmed.is_empty() {
        return None;
    }

    let contract = describe_local_model_contract(Path::new(trimmed));
    let artifact_kind = contract.kind.as_str().to_string();
    let mut effective_model = trimmed.to_string();
    let mut bridge_mode = "direct_binding".to_string();
    let mut note = contract.reason;

    if role == "image_generation" {
        match artifact_kind.as_str() {
            "diffusers_directory" | "image_onnx_directory" => {
                if let Some(base_url) = image_bridge_base_url {
                    let alias = sanitize_bridge_alias(trimmed);
                    effective_model = format!("bridge-image:{base_url}|{alias}");
                    bridge_mode = "image_bridge".to_string();
                    note = format!(
                        "Selected image model directory is automatically linked to the Windows image bridge. Source package stays at the chosen folder, while runtime traffic resolves through {effective_model}."
                    );
                } else {
                    bridge_mode = "image_bridge_pending_base_url".to_string();
                    note = "Selected image model directory is bridge-ready, but no Windows image bridge base URL is available yet. Start/configure the Windows image bridge to activate the effective runtime binding."
                        .to_string();
                }
            }
            "image_bridge" => {
                bridge_mode = "image_bridge".to_string();
                note = "Image generation already points at a dedicated bridge runtime.".to_string();
            }
            "api_reference" => {
                bridge_mode = "cloud_api".to_string();
                note = "Image generation is configured against a cloud/API backend.".to_string();
            }
            _ => {
                bridge_mode = "unclassified_image_binding".to_string();
            }
        }
    } else if matches!(artifact_kind.as_str(), "onnx_directory" | "onnx_file") {
        bridge_mode = "windows_native_target".to_string();
        note = format!(
            "Selected model matches the Windows-native ML contract for {role}; no extra bridge URI is required because the effective runtime binds directly from the chosen package."
        );
    } else if trimmed.eq_ignore_ascii_case("tesseract") || trimmed.eq_ignore_ascii_case("piper") {
        bridge_mode = "specialized_runtime".to_string();
    } else {
        bridge_mode = "migration_pending".to_string();
    }

    Some(WindowsMlBridgeBinding {
        role: role.to_string(),
        source_model: trimmed.to_string(),
        effective_model,
        artifact_kind,
        runtime_target: runtime_target.to_string(),
        execution_provider: execution_provider.to_string(),
        bridge_mode,
        note,
    })
}

fn sync_windows_ml_bridge_config(config: &mut benshu_brain::config::AppConfig) {
    let image_bridge_base_url =
        default_image_bridge_base_url(config.windows_ml_bridge.image_bridge_base_url.as_deref());
    let mut bindings = BTreeMap::new();

    let pairs = [
        (
            "embedding",
            config.knowledge.embed_model.as_str(),
            "onnx_runtime_directml_winml",
            "directml_winml",
        ),
        (
            "rerank",
            config.knowledge.rerank_model.as_str(),
            "onnx_runtime_directml_winml",
            "directml_winml",
        ),
        (
            "slm_tactical",
            config.sensory.tactical_model.as_deref().unwrap_or_default(),
            "onnx_runtime_directml_winml",
            "directml_winml",
        ),
        (
            "fact_check",
            config
                .sensory
                .fact_check_model
                .as_deref()
                .unwrap_or_default(),
            "onnx_runtime_directml_winml",
            "directml_winml",
        ),
        (
            "speech_to_text",
            config.sensory.stt_model.as_deref().unwrap_or_default(),
            "specialized_voice_runtime",
            "specialized_runtime",
        ),
        (
            "text_to_speech",
            config.sensory.tts_model.as_deref().unwrap_or_default(),
            "specialized_voice_runtime",
            "specialized_runtime",
        ),
        (
            "ocr",
            config.sensory.ocr_model.as_deref().unwrap_or_default(),
            "specialized_ocr_runtime",
            "specialized_runtime",
        ),
        (
            "vision",
            config.sensory.vision_model.as_deref().unwrap_or_default(),
            "multimodal_runtime",
            "provider_or_bridge_multimodal",
        ),
        (
            "image_generation",
            config
                .sensory
                .image_gen_model
                .as_deref()
                .unwrap_or_default(),
            "specialized_image_runtime",
            "directml_or_bridge",
        ),
        (
            "image_edit",
            config
                .sensory
                .image_edit_model
                .as_deref()
                .unwrap_or_default(),
            "windows_native_image_edit_runtime",
            "directml_winml",
        ),
        (
            "audio_understanding",
            config
                .sensory
                .audio_understanding_model
                .as_deref()
                .unwrap_or_default(),
            "onnx_runtime_directml_winml",
            "directml_winml",
        ),
        (
            "realtime_vad",
            config
                .sensory
                .realtime_vad_model
                .as_deref()
                .unwrap_or_default(),
            "onnx_runtime_directml_winml",
            "directml_winml",
        ),
        (
            "duplex_voice",
            config
                .sensory
                .duplex_voice_model
                .as_deref()
                .unwrap_or_default(),
            "windows_native_realtime_voice_runtime",
            "directml_winml",
        ),
        (
            "local_classifier",
            config
                .sensory
                .local_classifier_model
                .as_deref()
                .unwrap_or_default(),
            "onnx_runtime_directml_winml",
            "directml_winml",
        ),
        (
            "local_router",
            config
                .sensory
                .local_router_model
                .as_deref()
                .unwrap_or_default(),
            "onnx_runtime_directml_winml",
            "directml_winml",
        ),
        (
            "local_safety",
            config
                .sensory
                .local_safety_model
                .as_deref()
                .unwrap_or_default(),
            "onnx_runtime_directml_winml",
            "directml_winml",
        ),
    ];

    for (role, source_model, runtime_target, execution_provider) in pairs {
        if let Some(binding) = build_windows_ml_binding(
            role,
            source_model,
            runtime_target,
            execution_provider,
            image_bridge_base_url.as_deref(),
        ) {
            bindings.insert(role.to_string(), binding);
        }
    }

    config.windows_ml_bridge = WindowsMlBridgeConfig {
        image_bridge_base_url,
        bindings,
    };
}

fn discover_windows_script(script_name: &str) -> Option<PathBuf> {
    first_existing_path(
        runtime_discovery_roots()
            .into_iter()
            .map(|root| root.join("scripts").join("windows").join(script_name)),
    )
}

fn discover_wsl_script(script_name: &str) -> Option<PathBuf> {
    first_existing_path(
        runtime_discovery_roots()
            .into_iter()
            .map(|root| root.join("scripts").join("wsl").join(script_name)),
    )
}

fn discover_windows_llama_server_exe() -> Option<PathBuf> {
    if let Some(status) = discover_supported_windows_llama_server() {
        return Some(status.path);
    }
    warn!(
        target: "benshu::runtime_host_control",
        minimum_build = MIN_SUPPORTED_LLAMA_CPP_BUILD,
        "No supported llama.cpp runtime was discovered for automatic local GGUF hosting."
    );
    None
}

fn is_managed_llama_runtime_command(control: &ManagedRuntimeHostConfig) -> bool {
    control.restart_command.iter().any(|arg| {
        let lowered = arg.to_ascii_lowercase();
        lowered.contains("restart_llama_server_vulkan.ps1")
            || lowered.contains("enable_windows_llama_bridge.sh")
    })
}

fn maybe_auto_configure_runtime_host_control(config: &mut benshu_brain::config::AppConfig) {
    if cfg!(windows) {
        maybe_auto_configure_main_brain_restart(config);
        maybe_auto_configure_windows_ml_restart(config);
        return;
    }
    if running_under_wsl() {
        maybe_auto_configure_main_brain_wsl_bridge_restart(config);
    }
}

fn maybe_auto_configure_main_brain_wsl_bridge_restart(
    config: &mut benshu_brain::config::AppConfig,
) {
    let control = &mut config.runtime_host_control.main_brain;
    if !control.control_mode.trim().is_empty()
        && !control.control_mode.eq("disabled")
        && !is_managed_llama_runtime_command(control)
    {
        if control.timeout_secs == 0 {
            control.timeout_secs = 240;
        }
        return;
    }

    let Some(script_path) = discover_wsl_script("enable_windows_llama_bridge.sh") else {
        return;
    };
    let Some(agent_cfg) = config.agents.get("benshu") else {
        return;
    };
    if agent_cfg
        .local_model_artifact
        .as_deref()
        .map_or(true, |value| value.trim().is_empty())
    {
        return;
    }

    let Some(server_exe) = discover_windows_llama_server_exe() else {
        return;
    };
    let command = vec![
        "env".to_string(),
        format!(
            "BENSHU_WINDOWS_LLAMA_SERVER_EXE={}",
            server_exe.to_string_lossy()
        ),
        "bash".to_string(),
        script_path.to_string_lossy().to_string(),
    ];

    control.control_mode = "command".to_string();
    control.restart_command = command;
    control.timeout_secs = control.timeout_secs.max(240);
}

fn main_brain_runtime_changed(
    current: &benshu_brain::config::AppConfig,
    updated: &benshu_brain::config::AppConfig,
) -> bool {
    if current.llama_cpp_runtime != updated.llama_cpp_runtime {
        return true;
    }
    let current_agent = current.agents.get("benshu");
    let updated_agent = updated.agents.get("benshu");
    let agent_runtime_changed = match (current_agent, updated_agent) {
        (Some(current), Some(updated)) => {
            current.provider != updated.provider
                || current.base_url != updated.base_url
                || current.model != updated.model
                || current.local_model_artifact != updated.local_model_artifact
                || current.local_mmproj_artifact != updated.local_mmproj_artifact
                || current.local_runtime_family != updated.local_runtime_family
        }
        (None, None) => false,
        _ => true,
    };
    agent_runtime_changed
        || current.runtime_host_control.main_brain != updated.runtime_host_control.main_brain
}

fn discover_windows_image_service_exe() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("BENSHU_WINDOWS_IMAGE_SERVICE_EXE") {
        let path = PathBuf::from(value);
        if path.exists() {
            return Some(path);
        }
    }

    let Some(exe_dir) = current_binary_dir() else {
        return None;
    };

    first_existing_path([
        exe_dir.join("benshu-windows-image-service.exe"),
        exe_dir
            .parent()
            .map(|parent| parent.join("benshu-windows-image-service.exe"))
            .unwrap_or_else(|| exe_dir.join("benshu-windows-image-service.exe")),
    ])
}

fn discover_windows_python_exe() -> Option<PathBuf> {
    std::env::var("BENSHU_WINDOWS_PYTHON_EXE")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn parse_host_port(base_url: Option<&str>, default_host: &str, default_port: u16) -> (String, u16) {
    let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) else {
        return (default_host.to_string(), default_port);
    };
    let Ok(url) = Url::parse(base_url) else {
        return (default_host.to_string(), default_port);
    };
    let host = url.host_str().unwrap_or(default_host).to_string();
    let port = url.port().unwrap_or(default_port);
    (host, port)
}

fn bool_flag(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn apply_llama_cpp_runtime_planning(config: &mut benshu_brain::config::AppConfig) {
    let Some(agent_cfg) = config.agents.get("benshu") else {
        return;
    };
    let runtime_snapshot = config.llama_cpp_runtime.clone();
    let input = LlamaCppRuntimeInput {
        model_path: agent_cfg.local_model_artifact.clone(),
        mmproj_path: agent_cfg.local_mmproj_artifact.clone(),
        ctx_size: runtime_snapshot.ctx_size,
        requested_gpu_layers: runtime_snapshot.gpu_layers,
        tuning_mode: runtime_snapshot.tuning_mode.clone(),
        performance_profile: runtime_snapshot.performance_profile.clone(),
        vram_limit_gb: config.knowledge.model_vram_limit_gb,
        ram_limit_gb: config.knowledge.model_ram_limit_gb,
        hardware: summarize_hardware(&HardwareStatus::detect()),
    };
    let recommendation = recommend_llama_cpp_runtime(&input);
    let auto_tuning = should_apply_llama_cpp_auto_recommendation(&runtime_snapshot.tuning_mode);

    let runtime = &mut config.llama_cpp_runtime;
    if auto_tuning {
        runtime.ctx_size = recommendation.recommended_ctx_size;
        runtime.gpu_layers = recommendation.recommended_gpu_layers;
        runtime.batch_size = recommendation.recommended_batch_size;
        runtime.ubatch_size = recommendation.recommended_ubatch_size;
        runtime.threads = recommendation.recommended_threads;
        runtime.mmap = recommendation.recommended_mmap;
        runtime.mlock = recommendation.recommended_mlock;
        runtime.kv_offload = recommendation.recommended_kv_offload;
        runtime.flash_attn_mode = recommendation.recommended_flash_attn_mode.clone();
        runtime.cache_prompt = recommendation.recommended_cache_prompt;
        runtime.cont_batching = recommendation.recommended_cont_batching;
        runtime.parallel_slots = recommendation.recommended_parallel_slots;
    }
    let reasoning = resolve_llama_cpp_reasoning_compatibility(
        agent_cfg.local_model_artifact.as_deref(),
        &runtime.reasoning_mode,
        &runtime.reasoning_format,
    );
    if reasoning.adjusted {
        runtime.reasoning_mode = reasoning.reasoning_mode.clone();
        runtime.reasoning_format = reasoning.reasoning_format.clone();
        if let Some(note) = reasoning.note {
            warn!("{note}");
        }
    }

    let diagnostics = build_effective_diagnostics_with_runtime(
        &input,
        &recommendation,
        runtime.gpu_layers,
        runtime.ctx_size,
        runtime.kv_offload,
    );
    runtime.last_recommendation = Some(recommendation);
    runtime.effective_diagnostics = Some(diagnostics);
}

fn maybe_auto_configure_main_brain_restart(config: &mut benshu_brain::config::AppConfig) {
    let control = &mut config.runtime_host_control.main_brain;
    if !control.control_mode.trim().is_empty()
        && !control.control_mode.eq("disabled")
        && !is_managed_llama_runtime_command(control)
    {
        if control.timeout_secs == 0 {
            control.timeout_secs = 60;
        }
        return;
    }

    let Some(script_path) = discover_windows_script("restart_llama_server_vulkan.ps1") else {
        return;
    };
    let Some(server_exe) = discover_windows_llama_server_exe() else {
        return;
    };
    let Some(agent_cfg) = config.agents.get("benshu") else {
        return;
    };
    let Some(model_path) = agent_cfg
        .local_model_artifact
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return;
    };

    let (bind_host, port) = parse_host_port(agent_cfg.base_url.as_deref(), "127.0.0.1", 8012);
    let alias = agent_cfg
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("benshu-main-brain");
    let api_key = config
        .providers
        .openai_api_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("sk-local-llama-key");
    let runtime = &config.llama_cpp_runtime;
    let reasoning = resolve_llama_cpp_reasoning_compatibility(
        Some(model_path),
        &runtime.reasoning_mode,
        &runtime.reasoning_format,
    );
    if let Some(note) = reasoning.note.as_deref() {
        warn!("{note}");
    }

    let mut command = vec![
        "powershell".to_string(),
        "-NoProfile".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        script_path.to_string_lossy().to_string(),
        "-ServerExe".to_string(),
        server_exe.to_string_lossy().to_string(),
        "-MinBuild".to_string(),
        MIN_SUPPORTED_LLAMA_CPP_BUILD.to_string(),
        "-ModelPath".to_string(),
        model_path.to_string(),
        "-Port".to_string(),
        port.to_string(),
        "-CtxSize".to_string(),
        runtime.ctx_size.to_string(),
        "-GpuLayers".to_string(),
        runtime.gpu_layers.to_string(),
        "-Threads".to_string(),
        runtime.threads.to_string(),
        "-BatchSize".to_string(),
        runtime.batch_size.to_string(),
        "-UbatchSize".to_string(),
        runtime.ubatch_size.to_string(),
        "-ParallelSlots".to_string(),
        runtime.parallel_slots.to_string(),
        "-CacheRam".to_string(),
        runtime.cache_ram.unwrap_or(256).to_string(),
        "-CtxCheckpoints".to_string(),
        runtime.ctx_checkpoints.unwrap_or(0).to_string(),
        "-FlashAttnMode".to_string(),
        runtime.flash_attn_mode.clone(),
        "-KvOffload".to_string(),
        bool_flag(runtime.kv_offload).to_string(),
        "-Mmap".to_string(),
        bool_flag(runtime.mmap).to_string(),
        "-Mlock".to_string(),
        bool_flag(runtime.mlock).to_string(),
        "-CachePrompt".to_string(),
        bool_flag(runtime.cache_prompt).to_string(),
        "-ContBatching".to_string(),
        bool_flag(runtime.cont_batching).to_string(),
        "-Warmup".to_string(),
        bool_flag(runtime.warmup).to_string(),
        "-ContextShift".to_string(),
        bool_flag(runtime.context_shift).to_string(),
        "-Jinja".to_string(),
        bool_flag(runtime.jinja).to_string(),
        "-CpuMoe".to_string(),
        bool_flag(runtime.cpu_moe).to_string(),
        "-FitMode".to_string(),
        runtime.fit_mode.clone(),
        "-MmprojOffload".to_string(),
        bool_flag(runtime.mmproj_offload).to_string(),
        "-ReasoningMode".to_string(),
        reasoning.reasoning_mode,
        "-ReasoningFormat".to_string(),
        reasoning.reasoning_format,
        "-BindHost".to_string(),
        bind_host,
        "-Alias".to_string(),
        alias.to_string(),
        "-ApiKey".to_string(),
        api_key.to_string(),
    ];

    if let Some(mmproj_path) = agent_cfg
        .local_mmproj_artifact
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.push("-MmprojPath".to_string());
        command.push(mmproj_path.to_string());
    }
    if let Some(value) = runtime.threads_batch.filter(|value| *value > 0) {
        command.push("-ThreadsBatch".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime
        .rope_scaling
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.push("-RopeScaling".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.rope_scale.filter(|value| value.is_finite()) {
        command.push("-RopeScale".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.rope_freq_base.filter(|value| value.is_finite()) {
        command.push("-RopeFreqBase".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.rope_freq_scale.filter(|value| value.is_finite()) {
        command.push("-RopeFreqScale".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.yarn_orig_ctx.filter(|value| *value > 0) {
        command.push("-YarnOrigCtx".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.yarn_ext_factor.filter(|value| value.is_finite()) {
        command.push("-YarnExtFactor".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.yarn_attn_factor.filter(|value| value.is_finite()) {
        command.push("-YarnAttnFactor".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.yarn_beta_slow.filter(|value| value.is_finite()) {
        command.push("-YarnBetaSlow".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.yarn_beta_fast.filter(|value| value.is_finite()) {
        command.push("-YarnBetaFast".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime
        .cache_type_k
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.push("-CacheTypeK".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime
        .cache_type_v
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.push("-CacheTypeV".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime
        .device
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.push("-Device".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime
        .split_mode
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.push("-SplitMode".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime
        .tensor_split
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.push("-TensorSplit".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.main_gpu {
        command.push("-MainGpu".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime
        .fit_target
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.push("-FitTarget".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.fit_ctx.filter(|value| *value > 0) {
        command.push("-FitCtx".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.n_cpu_moe.filter(|value| *value > 0) {
        command.push("-NCpuMoe".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.image_min_tokens.filter(|value| *value > 0) {
        command.push("-ImageMinTokens".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.image_max_tokens.filter(|value| *value > 0) {
        command.push("-ImageMaxTokens".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime.reasoning_budget {
        command.push("-ReasoningBudget".to_string());
        command.push(value.to_string());
    }
    if let Some(value) = runtime
        .reasoning_budget_message
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.push("-ReasoningBudgetMessage".to_string());
        command.push(value.to_string());
    }
    command.push("-SamplingTemperature".to_string());
    command.push(runtime.sampling_temperature.to_string());
    command.push("-SamplingTopK".to_string());
    command.push(runtime.sampling_top_k.to_string());
    command.push("-SamplingTopP".to_string());
    command.push(runtime.sampling_top_p.to_string());
    command.push("-SamplingMinP".to_string());
    command.push(runtime.sampling_min_p.to_string());
    command.push("-SamplingTypicalP".to_string());
    command.push(runtime.sampling_typical_p.to_string());
    command.push("-SamplingRepeatPenalty".to_string());
    command.push(runtime.sampling_repeat_penalty.to_string());
    command.push("-SamplingPresencePenalty".to_string());
    command.push(runtime.sampling_presence_penalty.to_string());
    command.push("-SamplingFrequencyPenalty".to_string());
    command.push(runtime.sampling_frequency_penalty.to_string());
    command.push("-SamplingMirostat".to_string());
    command.push(runtime.sampling_mirostat.to_string());
    command.push("-SamplingMirostatEta".to_string());
    command.push(runtime.sampling_mirostat_eta.to_string());
    command.push("-SamplingMirostatTau".to_string());
    command.push(runtime.sampling_mirostat_tau.to_string());
    if let Some(value) = runtime.seed {
        command.push("-Seed".to_string());
        command.push(value.to_string());
    }

    control.control_mode = "command".to_string();
    control.restart_command = command;
    control.timeout_secs = control.timeout_secs.max(60);
}

fn maybe_auto_configure_windows_ml_restart(config: &mut benshu_brain::config::AppConfig) {
    let control = &mut config.runtime_host_control.windows_ml;
    if !control.control_mode.trim().is_empty() && !control.control_mode.eq("disabled") {
        if control.timeout_secs == 0 {
            control.timeout_secs = 60;
        }
        return;
    }

    let image_model_dir = config
        .sensory
        .image_gen_model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            config
                .sensory
                .image_edit_model
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        });

    if let Some(model_dir) = image_model_dir {
        let Some(script_path) = discover_windows_script("restart_onnx_directml_image_bridge.ps1")
        else {
            return;
        };
        let service_exe = discover_windows_image_service_exe();
        let python_exe = discover_windows_python_exe();

        if service_exe.is_none() && python_exe.is_none() {
            return;
        }

        let (_, port) = parse_host_port(
            config.windows_ml_bridge.image_bridge_base_url.as_deref(),
            "127.0.0.1",
            8022,
        );
        let model_alias = Path::new(model_dir)
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("local-image-model");

        let mut command = vec![
            "powershell".to_string(),
            "-NoProfile".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-File".to_string(),
            script_path.to_string_lossy().to_string(),
            "-SourceModelDir".to_string(),
            model_dir.to_string(),
            "-OnnxModelDir".to_string(),
            model_dir.to_string(),
            "-ModelAlias".to_string(),
            model_alias.to_string(),
            "-ListenHost".to_string(),
            "127.0.0.1".to_string(),
            "-Port".to_string(),
            port.to_string(),
            "-NumSteps".to_string(),
            config.windows_ml_runtime.image_profile.steps.to_string(),
            "-GuidanceScale".to_string(),
            config.windows_ml_runtime.image_profile.guidance.to_string(),
            "-DeviceId".to_string(),
            "0".to_string(),
        ];

        if let Some(service_exe) = service_exe {
            command.push("-ServiceExe".to_string());
            command.push(service_exe.to_string_lossy().to_string());
        }

        if let Some(python_exe) = python_exe {
            let python = python_exe.to_string_lossy().to_string();
            command.push("-PythonExe".to_string());
            command.push(python.clone());
            command.push("-ExportPythonExe".to_string());
            command.push(python);
        }

        control.control_mode = "command".to_string();
        control.restart_command = command;
        control.timeout_secs = control.timeout_secs.max(120);
        return;
    }

    let Some(script_path) = discover_windows_script("restart_image_bridge_service.ps1") else {
        return;
    };
    let Ok(command_path) = std::env::var("BENSHU_WINDOWS_IMAGE_BRIDGE_COMMAND") else {
        return;
    };
    if command_path.trim().is_empty() {
        return;
    }

    let arguments = std::env::var("BENSHU_WINDOWS_IMAGE_BRIDGE_ARGS").unwrap_or_default();
    let working_directory =
        std::env::var("BENSHU_WINDOWS_IMAGE_BRIDGE_WORKDIR").unwrap_or_default();
    let health_url = std::env::var("BENSHU_WINDOWS_IMAGE_BRIDGE_HEALTH_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            config
                .windows_ml_bridge
                .image_bridge_base_url
                .as_deref()
                .map(|base| format!("{}/health", base.trim_end_matches("/v1")))
        })
        .unwrap_or_default();

    control.control_mode = "command".to_string();
    control.restart_command = vec![
        "powershell".to_string(),
        "-NoProfile".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        script_path.to_string_lossy().to_string(),
        "-CommandPath".to_string(),
        command_path,
        "-Arguments".to_string(),
        arguments,
        "-WorkingDirectory".to_string(),
        working_directory,
        "-HealthUrl".to_string(),
        health_url,
        "-HealthTimeoutSeconds".to_string(),
        "60".to_string(),
    ];
    control.timeout_secs = control.timeout_secs.max(60);
}

async fn apply_runtime_host_restarts(
    runtime_host_control: RuntimeHostControlConfig,
    restart_main_brain: bool,
    restart_windows_ml: bool,
) -> (RuntimeRestartOutcome, RuntimeRestartOutcome) {
    let mut main_brain_restarted = RuntimeRestartOutcome::default();
    let mut windows_ml_restarted = RuntimeRestartOutcome::default();
    if restart_main_brain {
        main_brain_restarted =
            request_runtime_host_restart("main_brain", &runtime_host_control.main_brain).await;
    }
    if restart_windows_ml {
        windows_ml_restarted =
            request_runtime_host_restart("windows_ml", &runtime_host_control.windows_ml).await;
    }
    (main_brain_restarted, windows_ml_restarted)
}

#[derive(Default)]
struct RuntimeRestartOutcome {
    started: bool,
    actual_base_url: Option<String>,
}

async fn reload_live_agents_after_runtime_budget_change(state: &AppState) {
    let roles = state.kernel.coordinator().get_active_roles();
    if roles.is_empty() {
        return;
    }

    state.factory.shared_provider_pool.write().clear();
    let mut seen = BTreeSet::new();
    for role in roles {
        let role_name = role.name().to_string();
        if !seen.insert(role_name.clone()) {
            continue;
        }
        if let Err(error) = state.factory.reload_agent(&role_name).await {
            warn!(
                target: "benshu::runtime_context_budget",
                role = %role_name,
                error = %error,
                "Runtime config changed, but live agent budget reload failed."
            );
        }
    }
}

async fn request_runtime_host_restart(
    role: &str,
    control: &ManagedRuntimeHostConfig,
) -> RuntimeRestartOutcome {
    let mode = control.control_mode.trim().to_string();
    if mode.is_empty() || mode.eq_ignore_ascii_case("disabled") {
        info!(
            target: "benshu::runtime_host_control",
            role,
            "Runtime host restart skipped because control mode is disabled."
        );
        return RuntimeRestartOutcome::default();
    }

    let timeout_secs = control.timeout_secs.max(1);
    let control_for_task = control.clone();
    let control_for_log = control.clone();
    let role_for_task = role.to_string();
    let role_for_log = role.to_string();
    let result = tokio::task::spawn_blocking(move || match mode.as_str() {
        "windows_service" => {
            restart_windows_service(&role_for_task, &control_for_task, timeout_secs)
        }
        "command" => run_restart_command(&role_for_task, &control_for_task, timeout_secs),
        other => Err(anyhow::anyhow!(
            "unsupported runtime host control mode for {}: {other}",
            role_for_task
        )),
    })
    .await;

    match result {
        Ok(Ok(stdout)) => {
            info!(
                target: "benshu::runtime_host_control",
                role = role_for_log.as_str(),
                control_mode = control_for_log.control_mode.as_str(),
                "Runtime host restart completed."
            );
            RuntimeRestartOutcome {
                started: true,
                actual_base_url:
                    crate::api::handlers::system::extract_runtime_base_url_from_restart_stdout(
                        &stdout,
                    ),
            }
        }
        Ok(Err(error)) => {
            warn!(
                target: "benshu::runtime_host_control",
                role = role_for_log.as_str(),
                control_mode = control_for_log.control_mode.as_str(),
                error = %error,
                "Runtime host restart request failed."
            );
            RuntimeRestartOutcome::default()
        }
        Err(join_error) => {
            warn!(
                target: "benshu::runtime_host_control",
                role = role_for_log.as_str(),
                control_mode = control_for_log.control_mode.as_str(),
                error = %join_error,
                "Runtime host restart task join failed."
            );
            RuntimeRestartOutcome::default()
        }
    }
}

fn restart_windows_service(
    role: &str,
    control: &ManagedRuntimeHostConfig,
    timeout_secs: u64,
) -> anyhow::Result<String> {
    if !cfg!(windows) {
        return Err(anyhow::anyhow!(
            "windows_service restart for {role} is only supported on Windows-native deployments"
        ));
    }
    let service_name = control
        .service_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing Windows service name for {role}"))?;
    let timeout_ms = timeout_secs.saturating_mul(1000);
    let command = format!(
        "Restart-Service -Name '{}' -Force; $svc = Get-Service -Name '{}'; $svc.WaitForStatus('Running', [TimeSpan]::FromMilliseconds({timeout_ms}))",
        service_name.replace('\'', "''"),
        service_name.replace('\'', "''"),
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(anyhow::anyhow!(
            "failed to restart Windows service {service_name} for {role} (stdout={stdout}, stderr={stderr})"
        ))
    }
}

fn run_restart_command(
    role: &str,
    control: &ManagedRuntimeHostConfig,
    timeout_secs: u64,
) -> anyhow::Result<String> {
    let program = control
        .restart_command
        .first()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing restart command for {role}"))?;
    let mut child = Command::new(program)
        .args(control.restart_command.iter().skip(1))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(anyhow::anyhow!(
                "restart command failed for {role} (stdout={stdout}, stderr={stderr})"
            ));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output()?;
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(anyhow::anyhow!(
                "restart command timed out for {role} after {timeout_secs}s (stdout={stdout}, stderr={stderr})"
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub async fn get_config(State(state): State<AppState>) -> Json<benshu_brain::config::AppConfig> {
    let mut config = state.app_config.read().clone();
    // Mask API keys for safety
    if config.providers.openai_api_key.is_some() {
        config.providers.openai_api_key = Some("********".to_string());
    }
    if config.providers.anthropic_api_key.is_some() {
        config.providers.anthropic_api_key = Some("********".to_string());
    }
    if config.providers.gemini_api_key.is_some() {
        config.providers.gemini_api_key = Some("********".to_string());
    }
    if config.providers.deepseek_api_key.is_some() {
        config.providers.deepseek_api_key = Some("********".to_string());
    }
    if config.providers.minimax_api_key.is_some() {
        config.providers.minimax_api_key = Some("********".to_string());
    }
    if config.providers.openrouter_api_key.is_some() {
        config.providers.openrouter_api_key = Some("********".to_string());
    }
    if config.providers.moonshot_api_key.is_some() {
        config.providers.moonshot_api_key = Some("********".to_string());
    }
    if config.providers.doubao_api_key.is_some() {
        config.providers.doubao_api_key = Some("********".to_string());
    }
    Json(config)
}

pub async fn get_continuation_runtime_status(
    State(state): State<AppState>,
) -> Json<ContinuationRuntimeStatus> {
    let cache_dir = continuation_cache_dir(&state);
    let config = state.app_config.read().continuation_runtime.clone();
    let cleanup_allowed =
        continuation_cache_cleanup_allowed(&state.kernel.base_dir(), cache_dir.as_path());
    let index_present = cache_dir.join("index.json").is_file();
    let entries_dir_present = cache_dir.join("entries").is_dir();
    let (entry_file_count, total_bytes) = scan_cache_dir(&cache_dir);

    Json(ContinuationRuntimeStatus {
        disk_cache_enabled: config.disk_cache_enabled,
        cache_dir: cache_dir.display().to_string(),
        cache_budget_mb: config.cache_budget_mb,
        cache_max_entries: config.cache_max_entries,
        disable_disk_cache_for_sensitive_tasks: config.disable_disk_cache_for_sensitive_tasks,
        cleanup_allowed,
        index_present,
        entries_dir_present,
        entry_file_count,
        total_bytes,
    })
}

pub async fn cleanup_continuation_cache(
    State(state): State<AppState>,
    Json(request): Json<ContinuationCacheCleanupRequest>,
) -> Json<ContinuationCacheCleanupReport> {
    let cache_dir = continuation_cache_dir(&state);
    let cleanup_allowed =
        continuation_cache_cleanup_allowed(&state.kernel.base_dir(), cache_dir.as_path());
    let (scanned, bytes_matched) = scan_cache_dir(&cache_dir);

    if !cleanup_allowed {
        return Json(ContinuationCacheCleanupReport {
            dry_run: request.dry_run,
            cache_dir: cache_dir.display().to_string(),
            scanned,
            deleted: 0,
            bytes_matched,
            bytes_deleted: 0,
            cleanup_allowed,
            skipped_reason: Some(
                "cache path is outside the gateway data directory or resolves unsafely".to_string(),
            ),
        });
    }

    if request.dry_run || !cache_dir.exists() {
        return Json(ContinuationCacheCleanupReport {
            dry_run: request.dry_run,
            cache_dir: cache_dir.display().to_string(),
            scanned,
            deleted: 0,
            bytes_matched,
            bytes_deleted: 0,
            cleanup_allowed,
            skipped_reason: None,
        });
    }

    let (deleted, bytes_deleted) = delete_cache_contents(&cache_dir);
    Json(ContinuationCacheCleanupReport {
        dry_run: request.dry_run,
        cache_dir: cache_dir.display().to_string(),
        scanned,
        deleted,
        bytes_matched,
        bytes_deleted,
        cleanup_allowed,
        skipped_reason: None,
    })
}

#[axum::debug_handler]
pub async fn update_config(
    State(state): State<AppState>,
    Json(new_config): Json<benshu_brain::config::AppConfig>,
) -> impl axum::response::IntoResponse {
    let result: Result<Json<ConfigUpdateResult>, AppError> =
        async {
            let (
                hub_config,
                se_config,
                runtime_host_control,
                restart_main_brain,
                restart_windows_ml,
            ) = {
                let mut config = state.app_config.write();

                // Preserve API keys if they were masked in the request
                let mut updated_config = new_config;
                if updated_config.providers.openai_api_key.as_deref() == Some("********") {
                    updated_config.providers.openai_api_key =
                        config.providers.openai_api_key.clone();
                }
                if updated_config.providers.anthropic_api_key.as_deref() == Some("********") {
                    updated_config.providers.anthropic_api_key =
                        config.providers.anthropic_api_key.clone();
                }
                if updated_config.providers.gemini_api_key.as_deref() == Some("********") {
                    updated_config.providers.gemini_api_key =
                        config.providers.gemini_api_key.clone();
                }
                if updated_config.providers.deepseek_api_key.as_deref() == Some("********") {
                    updated_config.providers.deepseek_api_key =
                        config.providers.deepseek_api_key.clone();
                }
                if updated_config.providers.minimax_api_key.as_deref() == Some("********") {
                    updated_config.providers.minimax_api_key =
                        config.providers.minimax_api_key.clone();
                }
                if updated_config.providers.openrouter_api_key.as_deref() == Some("********") {
                    updated_config.providers.openrouter_api_key =
                        config.providers.openrouter_api_key.clone();
                }
                if updated_config.providers.moonshot_api_key.as_deref() == Some("********") {
                    updated_config.providers.moonshot_api_key =
                        config.providers.moonshot_api_key.clone();
                }
                if updated_config.providers.doubao_api_key.as_deref() == Some("********") {
                    updated_config.providers.doubao_api_key =
                        config.providers.doubao_api_key.clone();
                }

                apply_llama_cpp_runtime_planning(&mut updated_config);
                sync_windows_ml_bridge_config(&mut updated_config);
                maybe_auto_configure_runtime_host_control(&mut updated_config);

                let restart_main_brain = main_brain_runtime_changed(&config, &updated_config);
                let restart_windows_ml =
                    config.windows_ml_runtime != updated_config.windows_ml_runtime;
                let runtime_host_control = updated_config.runtime_host_control.clone();

                *config = updated_config;
                config.save_to_file(&state.config_path)?;

                let c = &config.sensory;
                let k = &config.knowledge;

                let hub_config = benshu_sensory::hub::SensoryConfig {
                    fallback_policy: benshu_sensory::protocol::FallbackPolicy::SwitchToCloud,
                    vram_budget: (k.model_vram_limit_gb as u64) * 1024 * 1024 * 1024,
                    max_image_dimension: 2048,
                    vision_fallback: None,
                    audio_fallback: None,
                    video_frame_buffer_size: c.video_buffer_size.unwrap_or(10),
                };

                let se_config = benshu_engram::prelude::HybridSearchConfig {
                    db_path: state.kernel.base_dir().join("engram.db"),
                    vector_dimension: 384,
                    max_vectors: 100_000,
                    rrf_k: 60.0,
                    bm25_weight: 0.4,
                    vector_weight: 0.6,
                    dedup_threshold: 0.85,
                    enable_semantic_dedup: true,
                    vector_metric: benshu_engram::vector_store::VectorMetric::Cosine,
                    use_vector: k.enable_vector,
                    use_hierarchy_projection: k.enable_vector,
                    use_reranker: true,
                    embed_model: k.embed_model.clone(),
                    rerank_model: k.rerank_model.clone(),
                };

                (
                    hub_config,
                    se_config,
                    runtime_host_control,
                    restart_main_brain,
                    restart_windows_ml,
                )
            };

            // Push new sensory budget and policy to the SensoryHub (Async)
            state.kernel.sensory().reconfigure(hub_config).await;

            // Push new knowledge configuration to the HybridSearchEngine (Sync in this version)
            state.kernel.search_engine().reconfigure(se_config);

            // Trigger hot-reload of connectors
            let _ = state.connector_trigger.send(());

            let (main_brain_restart_outcome, windows_ml_restart_outcome) =
                apply_runtime_host_restarts(
                    runtime_host_control,
                    restart_main_brain,
                    restart_windows_ml,
                )
                .await;
            if main_brain_restart_outcome.started {
                crate::api::handlers::system::sync_runtime_config_after_host_restart_with_base_url(
                    &state,
                    "main_brain",
                    main_brain_restart_outcome.actual_base_url.as_deref(),
                )
                .await;
            }
            if restart_main_brain {
                reload_live_agents_after_runtime_budget_change(&state).await;
            }

            Ok(Json(ConfigUpdateResult {
                saved: true,
                main_brain_restart_needed: restart_main_brain,
                windows_ml_restart_needed: restart_windows_ml,
                main_brain_restart_requested: main_brain_restart_outcome.started,
                windows_ml_restart_requested: windows_ml_restart_outcome.started,
            }))
        }
        .await;
    result
}

pub async fn get_agent_identity(
    State(state): State<AppState>,
) -> Json<Option<benshu_brain::agent::agent_identity::AgentIdentity>> {
    let config = state.app_config.read();
    Json(config.agent_identity.clone())
}

pub async fn update_agent_identity(
    State(state): State<AppState>,
    Json(new_agent_identity): Json<benshu_brain::agent::agent_identity::AgentIdentity>,
) -> Result<StatusCode, AppError> {
    let mut config = state.app_config.write();
    config.agent_identity = Some(new_agent_identity);
    config.save_to_file(&state.config_path)?;
    Ok(StatusCode::OK)
}
