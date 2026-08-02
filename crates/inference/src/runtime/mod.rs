//! Runtime planning primitives for local model execution.
//!
//! This module deliberately stays free of agent orchestration.  It turns model,
//! hardware, and user preference inputs into explainable runtime diagnostics
//! that higher layers may persist, display, or apply.

pub mod lifecycle;

use crate::HardwareStatus;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use lifecycle::{
    ModelRuntimeBinding, ModelRuntimeKind, ModelRuntimeManager, ModelRuntimeState,
    ModelRuntimeStatus,
};

pub const LLAMA_TUNING_AUTO: &str = "auto";
pub const LLAMA_TUNING_MANUAL: &str = "manual";
pub const PROFILE_LOW_VRAM: &str = "low_vram";
pub const PROFILE_BALANCED: &str = "balanced";
pub const PROFILE_SPEED: &str = "speed";

const QWEN_REASONING_FORMAT: &str = "deepseek";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlamaCppRuntimePreference {
    #[serde(default = "default_tuning_mode")]
    pub tuning_mode: String,
    #[serde(default = "default_performance_profile")]
    pub performance_profile: String,
}

impl Default for LlamaCppRuntimePreference {
    fn default() -> Self {
        Self {
            tuning_mode: default_tuning_mode(),
            performance_profile: default_performance_profile(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlamaCppRuntimeInput {
    pub model_path: Option<String>,
    pub mmproj_path: Option<String>,
    pub ctx_size: u32,
    pub requested_gpu_layers: u32,
    pub tuning_mode: String,
    pub performance_profile: String,
    pub vram_limit_gb: u32,
    pub ram_limit_gb: u32,
    pub hardware: RuntimeHardwareSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeHardwareSummary {
    pub gpu_vendor: String,
    pub gpu_name: String,
    pub detected_vram_mb: Option<u64>,
    pub probe_confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeMemoryPlan {
    pub model_weight_mb: u64,
    pub mmproj_weight_mb: u64,
    pub estimated_vram_mb: u64,
    pub estimated_ram_mb: u64,
    pub kv_cache_budget_mb: u64,
    pub safety_margin_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlamaCppRuntimeRecommendation {
    // Kept as `recommended_*` for config/API compatibility. New UI should present
    // these as a resource estimate, not as a value that overrides user intent.
    pub recommended_ctx_size: u32,
    pub recommended_gpu_layers: u32,
    pub recommended_batch_size: u32,
    pub recommended_ubatch_size: u32,
    pub recommended_threads: i32,
    pub recommended_mmap: bool,
    pub recommended_mlock: bool,
    pub recommended_kv_offload: bool,
    pub recommended_flash_attn_mode: String,
    pub recommended_cache_prompt: bool,
    pub recommended_cont_batching: bool,
    pub recommended_parallel_slots: u32,
    pub memory_plan: RuntimeMemoryPlan,
    pub reason: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlamaCppReasoningCompatibility {
    pub model_family: String,
    pub reasoning_mode: String,
    pub reasoning_format: String,
    pub adjusted: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlamaCppEffectiveDiagnostics {
    pub tuning_mode: String,
    pub performance_profile: String,
    pub recommended_value_summary: String,
    pub user_override_summary: String,
    pub effective_value_summary: String,
    #[serde(default)]
    pub effective_memory_plan: Option<RuntimeMemoryPlan>,
    #[serde(default)]
    pub effective_memory_summary: String,
    #[serde(default)]
    pub effective_kv_location: String,
    pub reload_required: bool,
    pub status: String,
    pub notes: Vec<String>,
}

pub fn resolve_llama_cpp_reasoning_compatibility(
    model_path: Option<&str>,
    requested_reasoning_mode: &str,
    requested_reasoning_format: &str,
) -> LlamaCppReasoningCompatibility {
    let model_family = infer_llama_cpp_model_family(model_path);
    let requested_reasoning_mode = normalize_runtime_text(requested_reasoning_mode, "auto");
    let requested_reasoning_format = normalize_runtime_text(requested_reasoning_format, "auto");
    let mut reasoning_mode = normalize_reasoning_mode(&requested_reasoning_mode);
    let mut reasoning_format = normalize_reasoning_format(&requested_reasoning_format);
    let mut adjusted = reasoning_mode != requested_reasoning_mode
        || reasoning_format != requested_reasoning_format;
    let mut note = None;

    if reasoning_mode_disables_thinking(&reasoning_mode) {
        if reasoning_mode != "off" {
            reasoning_mode = "off".to_string();
            adjusted = true;
        }
        if reasoning_format != "none" {
            reasoning_format = "none".to_string();
            adjusted = true;
        }
        return LlamaCppReasoningCompatibility {
            model_family,
            reasoning_mode,
            reasoning_format,
            adjusted,
            note,
        };
    }

    if model_family == "qwen" {
        if reasoning_format_is_incompatible_with_qwen(&reasoning_format) {
            reasoning_format = QWEN_REASONING_FORMAT.to_string();
            adjusted = true;
        }
        if adjusted {
            note = Some(
                "qwen_reasoning_compatibility: Qwen reasoning models require llama.cpp thinking tag parsing"
                    .to_string(),
            );
        }
    }

    LlamaCppReasoningCompatibility {
        model_family,
        reasoning_mode,
        reasoning_format,
        adjusted,
        note,
    }
}

fn default_llama_runtime_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|threads| (threads.get() / 4).clamp(4, 8) as i32)
        .unwrap_or(8)
}

pub fn summarize_hardware(status: &HardwareStatus) -> RuntimeHardwareSummary {
    RuntimeHardwareSummary {
        gpu_vendor: status
            .gpu_vendor
            .map(|vendor| format!("{vendor:?}"))
            .unwrap_or_else(|| "Unknown".to_string()),
        gpu_name: status
            .gpu_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        detected_vram_mb: (status.vram_total_mb > 0).then_some(status.vram_total_mb),
        probe_confidence: format!("{:?}", status.gpu_probe_confidence),
    }
}

pub fn recommend_llama_cpp_runtime(input: &LlamaCppRuntimeInput) -> LlamaCppRuntimeRecommendation {
    let model_weight_mb = input
        .model_path
        .as_deref()
        .and_then(path_size_mb)
        .unwrap_or(0);
    let mmproj_weight_mb = input
        .mmproj_path
        .as_deref()
        .and_then(path_size_mb)
        .unwrap_or(0);
    let detected_vram_mb = input.hardware.detected_vram_mb.unwrap_or(0);
    let configured_vram_mb = u64::from(input.vram_limit_gb).saturating_mul(1024);
    let configured_ram_mb = u64::from(input.ram_limit_gb).saturating_mul(1024);
    let usable_vram_mb = match (detected_vram_mb, configured_vram_mb) {
        (0, 0) => 0,
        (0, configured) => configured,
        (detected, 0) => detected,
        (detected, configured) => detected.min(configured),
    };
    let safety_margin_mb = match normalized_profile(&input.performance_profile) {
        PROFILE_LOW_VRAM => 4096,
        PROFILE_SPEED => 1536,
        _ => 2048,
    };
    let recommended_ctx_size = auto_fit_context_size(
        input,
        model_weight_mb,
        mmproj_weight_mb,
        usable_vram_mb,
        configured_ram_mb,
        safety_margin_mb,
    );
    let kv_cache_budget_mb =
        estimate_kv_cache_budget_mb(recommended_ctx_size, &input.performance_profile);
    let runtime_overhead_mb = 1024;
    let available_for_weights = usable_vram_mb
        .saturating_sub(safety_margin_mb)
        .saturating_sub(kv_cache_budget_mb)
        .saturating_sub(runtime_overhead_mb);

    let mut warnings = Vec::new();
    if model_weight_mb == 0 {
        warnings.push("model_size_unknown: unable to estimate full model weight size".to_string());
    }
    if usable_vram_mb == 0 {
        warnings.push(
            "vram_unknown: resource estimate is conservative because usable VRAM is unknown"
                .to_string(),
        );
    }

    let total_layers =
        infer_total_layers_from_model_name(input.model_path.as_deref()).unwrap_or(64);
    let recommended_gpu_layers = if model_weight_mb == 0 || available_for_weights == 0 {
        input.requested_gpu_layers.min(total_layers)
    } else {
        let raw = ((available_for_weights as f64 / model_weight_mb.max(1) as f64)
            * total_layers as f64)
            .floor() as u32;
        let profile_floor = match normalized_profile(&input.performance_profile) {
            PROFILE_LOW_VRAM => 8,
            PROFILE_SPEED => total_layers,
            _ => 16,
        };
        raw.clamp(profile_floor.min(total_layers), total_layers)
    };

    let recommended_batch_size = match normalized_profile(&input.performance_profile) {
        PROFILE_LOW_VRAM => 1024,
        PROFILE_SPEED => 2048,
        _ => 2048,
    };
    let recommended_ubatch_size = match normalized_profile(&input.performance_profile) {
        PROFILE_LOW_VRAM => 256,
        PROFILE_SPEED => 512,
        _ => 512,
    };

    let estimated_vram_with_kv_mb = estimate_vram_usage_mb(
        model_weight_mb,
        mmproj_weight_mb,
        total_layers,
        recommended_gpu_layers,
        kv_cache_budget_mb,
        runtime_overhead_mb,
    );
    let total_model_mb = model_weight_mb.saturating_add(mmproj_weight_mb);
    let text_weight_on_gpu = if total_layers == 0 {
        0
    } else {
        model_weight_mb.saturating_mul(u64::from(recommended_gpu_layers)) / u64::from(total_layers)
    };
    let base_ram_mb = total_model_mb
        .saturating_sub(text_weight_on_gpu)
        .saturating_add(1024);
    let estimated_ram_with_kv_mb = base_ram_mb.saturating_add(kv_cache_budget_mb);
    let kv_fits_vram = usable_vram_mb == 0
        || estimated_vram_with_kv_mb.saturating_add(safety_margin_mb) <= usable_vram_mb;
    let kv_fits_ram = configured_ram_mb == 0 || estimated_ram_with_kv_mb <= configured_ram_mb;
    let recommended_kv_offload = kv_fits_vram || !kv_fits_ram;
    let estimated_vram_mb = estimate_vram_usage_mb(
        model_weight_mb,
        mmproj_weight_mb,
        total_layers,
        recommended_gpu_layers,
        if recommended_kv_offload {
            kv_cache_budget_mb
        } else {
            0
        },
        runtime_overhead_mb,
    );
    let estimated_ram_mb = base_ram_mb.saturating_add(
        (!recommended_kv_offload)
            .then_some(kv_cache_budget_mb)
            .unwrap_or(0),
    );

    if !recommended_kv_offload {
        warnings.push(format!(
            "kv_cache_moved_to_ram: vram_with_kv={}MiB safety_margin={}MiB budget={}MiB",
            estimated_vram_with_kv_mb, safety_margin_mb, usable_vram_mb
        ));
    } else if !kv_fits_vram && !kv_fits_ram {
        warnings.push(format!(
            "kv_cache_has_no_budget_compliant_location: vram_with_kv={}MiB vram_budget={}MiB ram_with_kv={}MiB ram_budget={}MiB",
            estimated_vram_with_kv_mb, usable_vram_mb, estimated_ram_with_kv_mb, configured_ram_mb
        ));
    }

    if estimated_vram_mb > usable_vram_mb && usable_vram_mb > 0 {
        warnings.push(format!(
            "estimated_vram_exceeds_budget: estimated={}MiB budget={}MiB",
            estimated_vram_mb, usable_vram_mb
        ));
    }

    let memory_plan = RuntimeMemoryPlan {
        model_weight_mb,
        mmproj_weight_mb,
        estimated_vram_mb,
        estimated_ram_mb,
        kv_cache_budget_mb,
        safety_margin_mb,
    };

    LlamaCppRuntimeRecommendation {
        recommended_ctx_size,
        recommended_gpu_layers,
        recommended_batch_size,
        recommended_ubatch_size,
        recommended_threads: default_llama_runtime_threads(),
        recommended_mmap: false,
        recommended_mlock: false,
        recommended_kv_offload,
        recommended_flash_attn_mode: "auto".to_string(),
        recommended_cache_prompt: false,
        recommended_cont_batching: false,
        recommended_parallel_slots: 1,
        memory_plan,
        reason: format!(
            "profile={} usable_vram={}MiB model={}MiB ctx={} total_layers={} estimated_gpu_layers={}",
            normalized_profile(&input.performance_profile),
            usable_vram_mb,
            model_weight_mb,
            recommended_ctx_size,
            total_layers,
            recommended_gpu_layers
        ),
        warnings,
    }
}

fn auto_fit_context_size(
    input: &LlamaCppRuntimeInput,
    model_weight_mb: u64,
    mmproj_weight_mb: u64,
    usable_vram_mb: u64,
    configured_ram_mb: u64,
    safety_margin_mb: u64,
) -> u32 {
    if model_weight_mb == 0
        || usable_vram_mb == 0
        || configured_ram_mb == 0
        || input.ctx_size <= 4096
    {
        return input.ctx_size;
    }

    let requested_kv_mb = estimate_kv_cache_budget_mb(input.ctx_size, &input.performance_profile);
    let runtime_overhead_mb = 1024;
    let model_on_vram_with_requested_kv = model_weight_mb
        .saturating_add(mmproj_weight_mb)
        .saturating_add(requested_kv_mb)
        .saturating_add(runtime_overhead_mb)
        .saturating_add(safety_margin_mb);
    let kv_in_ram_with_requested_ctx = model_weight_mb
        .saturating_add(mmproj_weight_mb)
        .saturating_add(requested_kv_mb)
        .saturating_add(runtime_overhead_mb);
    if model_on_vram_with_requested_kv <= usable_vram_mb
        || kv_in_ram_with_requested_ctx <= configured_ram_mb
    {
        return input.ctx_size;
    }

    // Keep the largest standard context that leaves the complete model and its
    // KV cache inside the detected VRAM budget. The automatic planner already
    // uses the chosen context when deciding GPU layers, so this is a single
    // resource decision rather than a second runtime fallback.
    [131_072, 65_536, 32_768, 16_384, 8_192, 4_096]
        .into_iter()
        .filter(|candidate| *candidate <= input.ctx_size)
        .find(|candidate| {
            let kv_mb = estimate_kv_cache_budget_mb(*candidate, &input.performance_profile);
            model_weight_mb
                .saturating_add(mmproj_weight_mb)
                .saturating_add(kv_mb)
                .saturating_add(runtime_overhead_mb)
                .saturating_add(safety_margin_mb)
                <= usable_vram_mb
        })
        .unwrap_or(input.ctx_size)
}

pub fn build_effective_diagnostics(
    input: &LlamaCppRuntimeInput,
    recommendation: &LlamaCppRuntimeRecommendation,
    effective_gpu_layers: u32,
    effective_ctx_size: u32,
) -> LlamaCppEffectiveDiagnostics {
    build_effective_diagnostics_with_runtime(
        input,
        recommendation,
        effective_gpu_layers,
        effective_ctx_size,
        recommendation.recommended_kv_offload,
    )
}

pub fn build_effective_diagnostics_with_runtime(
    input: &LlamaCppRuntimeInput,
    recommendation: &LlamaCppRuntimeRecommendation,
    effective_gpu_layers: u32,
    effective_ctx_size: u32,
    effective_kv_offload: bool,
) -> LlamaCppEffectiveDiagnostics {
    let tuning_mode = normalized_tuning_mode(&input.tuning_mode).to_string();
    let profile = normalized_profile(&input.performance_profile).to_string();
    let user_override = if tuning_mode == LLAMA_TUNING_MANUAL {
        format!(
            "manual gpu_layers={} ctx_size={}",
            input.requested_gpu_layers, input.ctx_size
        )
    } else {
        "none: automatic runtime planning active".to_string()
    };
    let effective_memory_plan = estimate_effective_memory_plan(
        input,
        effective_gpu_layers,
        effective_ctx_size,
        effective_kv_offload,
    );
    let kv_location = if effective_kv_offload { "VRAM" } else { "RAM" };
    let usable_vram_mb = usable_vram_mb(input);
    let configured_ram_mb = u64::from(input.ram_limit_gb).saturating_mul(1024);
    let mut notes = recommendation.warnings.clone();
    if usable_vram_mb > 0 && effective_memory_plan.estimated_vram_mb > usable_vram_mb {
        notes.push(format!(
            "effective_vram_exceeds_budget: estimated={}MiB budget={}MiB",
            effective_memory_plan.estimated_vram_mb, usable_vram_mb
        ));
    }
    if configured_ram_mb > 0 && effective_memory_plan.estimated_ram_mb > configured_ram_mb {
        notes.push(format!(
            "effective_ram_exceeds_budget: estimated={}MiB budget={}MiB",
            effective_memory_plan.estimated_ram_mb, configured_ram_mb
        ));
    }
    if effective_ctx_size >= 131_072 {
        notes.push(format!(
            "large_context_kv_cache: ctx={} kv_cache≈{}MiB location={}",
            effective_ctx_size, effective_memory_plan.kv_cache_budget_mb, kv_location
        ));
    }

    LlamaCppEffectiveDiagnostics {
        tuning_mode,
        performance_profile: profile,
        recommended_value_summary: format!(
            "capacity_estimate gpu_layers={} ctx_size={} batch={} ubatch={} est_vram={}MiB est_ram={}MiB",
            recommendation.recommended_gpu_layers,
            recommendation.recommended_ctx_size,
            recommendation.recommended_batch_size,
            recommendation.recommended_ubatch_size,
            recommendation.memory_plan.estimated_vram_mb,
            recommendation.memory_plan.estimated_ram_mb
        ),
        user_override_summary: user_override,
        effective_value_summary: format!(
            "gpu_layers={} ctx_size={} kv_offload={}",
            effective_gpu_layers, effective_ctx_size, effective_kv_offload
        ),
        effective_memory_summary: format!(
            "effective_est_vram={}MiB effective_est_ram_or_commit={}MiB kv_cache={}MiB kv_location={}",
            effective_memory_plan.estimated_vram_mb,
            effective_memory_plan.estimated_ram_mb,
            effective_memory_plan.kv_cache_budget_mb,
            kv_location
        ),
        effective_memory_plan: Some(effective_memory_plan),
        effective_kv_location: kv_location.to_string(),
        reload_required: false,
        status: if notes.is_empty() {
            "ready".to_string()
        } else {
            "ready_with_warnings".to_string()
        },
        notes,
    }
}

pub fn should_apply_llama_cpp_auto_recommendation(tuning_mode: &str) -> bool {
    normalized_tuning_mode(tuning_mode) == LLAMA_TUNING_AUTO || tuning_mode.trim().is_empty()
}

pub fn estimate_llama_kv_cache_budget_mb(ctx_size: u32, profile: &str) -> u64 {
    let base = (u64::from(ctx_size).saturating_mul(384)) / 1024;
    let floor = match normalized_profile(profile) {
        PROFILE_LOW_VRAM => 512,
        PROFILE_SPEED => 2048,
        _ => 1024,
    };
    base.max(floor)
}

fn estimate_kv_cache_budget_mb(ctx_size: u32, profile: &str) -> u64 {
    estimate_llama_kv_cache_budget_mb(ctx_size, profile)
}

fn estimate_effective_memory_plan(
    input: &LlamaCppRuntimeInput,
    effective_gpu_layers: u32,
    effective_ctx_size: u32,
    effective_kv_offload: bool,
) -> RuntimeMemoryPlan {
    let model_weight_mb = input
        .model_path
        .as_deref()
        .and_then(path_size_mb)
        .unwrap_or(0);
    let mmproj_weight_mb = input
        .mmproj_path
        .as_deref()
        .and_then(path_size_mb)
        .unwrap_or(0);
    let total_layers =
        infer_total_layers_from_model_name(input.model_path.as_deref()).unwrap_or(64);
    let gpu_layers = effective_gpu_layers.min(total_layers);
    let text_weight_on_gpu = if total_layers == 0 {
        0
    } else {
        model_weight_mb.saturating_mul(u64::from(gpu_layers)) / u64::from(total_layers)
    };
    let text_weight_on_ram = model_weight_mb.saturating_sub(text_weight_on_gpu);
    let kv_cache_budget_mb =
        estimate_llama_kv_cache_budget_mb(effective_ctx_size, &input.performance_profile);
    let runtime_overhead_mb = 1024;
    let safety_margin_mb = match normalized_profile(&input.performance_profile) {
        PROFILE_LOW_VRAM => 4096,
        PROFILE_SPEED => 1536,
        _ => 2048,
    };
    let kv_vram_mb = if effective_kv_offload {
        kv_cache_budget_mb
    } else {
        0
    };
    let kv_ram_mb = if effective_kv_offload {
        0
    } else {
        kv_cache_budget_mb
    };
    RuntimeMemoryPlan {
        model_weight_mb,
        mmproj_weight_mb,
        estimated_vram_mb: text_weight_on_gpu
            .saturating_add(mmproj_weight_mb)
            .saturating_add(kv_vram_mb)
            .saturating_add(runtime_overhead_mb),
        estimated_ram_mb: text_weight_on_ram
            .saturating_add(kv_ram_mb)
            .saturating_add(runtime_overhead_mb),
        kv_cache_budget_mb,
        safety_margin_mb,
    }
}

fn usable_vram_mb(input: &LlamaCppRuntimeInput) -> u64 {
    let detected_vram_mb = input.hardware.detected_vram_mb.unwrap_or(0);
    let configured_vram_mb = u64::from(input.vram_limit_gb).saturating_mul(1024);
    match (detected_vram_mb, configured_vram_mb) {
        (0, 0) => 0,
        (0, configured) => configured,
        (detected, 0) => detected,
        (detected, configured) => detected.min(configured),
    }
}

fn estimate_vram_usage_mb(
    model_weight_mb: u64,
    mmproj_weight_mb: u64,
    total_layers: u32,
    gpu_layers: u32,
    kv_cache_budget_mb: u64,
    runtime_overhead_mb: u64,
) -> u64 {
    let text_weight_on_gpu = if total_layers == 0 {
        0
    } else {
        model_weight_mb.saturating_mul(u64::from(gpu_layers)) / u64::from(total_layers)
    };
    text_weight_on_gpu
        .saturating_add(mmproj_weight_mb)
        .saturating_add(kv_cache_budget_mb)
        .saturating_add(runtime_overhead_mb)
}

fn infer_total_layers_from_model_name(model_path: Option<&str>) -> Option<u32> {
    let value = model_path?.to_ascii_lowercase();
    if value.contains("26b") || value.contains("27b") {
        Some(64)
    } else if value.contains("70b") || value.contains("72b") {
        Some(80)
    } else if value.contains("13b") || value.contains("14b") {
        Some(40)
    } else if value.contains("7b") || value.contains("8b") || value.contains("9b") {
        Some(32)
    } else if value.contains("3b") || value.contains("4b") {
        Some(28)
    } else {
        None
    }
}

fn path_size_mb(path: &str) -> Option<u64> {
    let metadata = std::fs::metadata(Path::new(path)).ok()?;
    if metadata.is_file() {
        Some(bytes_to_mb(metadata.len()))
    } else {
        None
    }
}

fn bytes_to_mb(bytes: u64) -> u64 {
    bytes.saturating_add(1024 * 1024 - 1) / (1024 * 1024)
}

fn normalized_tuning_mode(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        LLAMA_TUNING_MANUAL => LLAMA_TUNING_MANUAL,
        _ => LLAMA_TUNING_AUTO,
    }
}

fn normalized_profile(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "low_vram" | "save_vram" | "memory_saver" => PROFILE_LOW_VRAM,
        "speed" | "fast" | "performance" => PROFILE_SPEED,
        _ => PROFILE_BALANCED,
    }
}

fn normalize_runtime_text(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

fn normalize_reasoning_mode(value: &str) -> String {
    let normalized = normalize_runtime_text(value, "auto");
    match normalized.as_str() {
        "false" | "none" | "disabled" | "0" => "off".to_string(),
        "true" | "enabled" | "1" => "on".to_string(),
        _ => normalized,
    }
}

fn normalize_reasoning_format(value: &str) -> String {
    let normalized = normalize_runtime_text(value, "auto");
    match normalized.as_str() {
        "false" | "off" | "disabled" | "0" => "none".to_string(),
        _ => normalized,
    }
}

fn infer_llama_cpp_model_family(model_path: Option<&str>) -> String {
    let Some(model_path) = model_path else {
        return "unknown".to_string();
    };
    let lowered = model_path.to_ascii_lowercase();
    if lowered.contains("qwen") {
        "qwen".to_string()
    } else if lowered.contains("gemma") {
        "gemma".to_string()
    } else if lowered.contains("deepseek") {
        "deepseek".to_string()
    } else {
        "unknown".to_string()
    }
}

fn reasoning_mode_disables_thinking(value: &str) -> bool {
    matches!(value.trim(), "off" | "false" | "none" | "disabled" | "0")
}

fn reasoning_format_is_incompatible_with_qwen(value: &str) -> bool {
    matches!(
        value.trim(),
        "" | "false" | "off" | "none" | "disabled" | "0" | "auto"
    )
}

fn default_tuning_mode() -> String {
    LLAMA_TUNING_AUTO.to_string()
}

fn default_performance_profile() -> String {
    PROFILE_BALANCED.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_keeps_gpu_layers_inside_layer_count() {
        let input = LlamaCppRuntimeInput {
            model_path: Some("/models/gemma4-26b-q3.gguf".to_string()),
            mmproj_path: None,
            ctx_size: 8192,
            requested_gpu_layers: 120,
            tuning_mode: LLAMA_TUNING_AUTO.to_string(),
            performance_profile: PROFILE_BALANCED.to_string(),
            vram_limit_gb: 24,
            ram_limit_gb: 64,
            hardware: RuntimeHardwareSummary {
                gpu_vendor: "Amd".to_string(),
                gpu_name: "RX 7900 XTX".to_string(),
                detected_vram_mb: Some(24 * 1024),
                probe_confidence: "High".to_string(),
            },
        };

        let recommendation = recommend_llama_cpp_runtime(&input);
        assert!(recommendation.recommended_gpu_layers <= 64);
        assert!(recommendation.recommended_gpu_layers >= 16);
    }

    #[test]
    fn recommendation_moves_large_context_kv_to_ram_when_vram_would_overcommit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let model_path = temp.path().join("gemma4-26b-q4_k_m.gguf");
        let model = std::fs::File::create(&model_path).expect("sparse model file");
        model
            .set_len(17_862 * 1024 * 1024)
            .expect("sparse model size");
        let input = LlamaCppRuntimeInput {
            model_path: Some(model_path.to_string_lossy().into_owned()),
            mmproj_path: None,
            ctx_size: 65_536,
            requested_gpu_layers: 24,
            tuning_mode: LLAMA_TUNING_AUTO.to_string(),
            performance_profile: PROFILE_BALANCED.to_string(),
            vram_limit_gb: 24,
            ram_limit_gb: 64,
            hardware: RuntimeHardwareSummary {
                gpu_vendor: "Amd".to_string(),
                gpu_name: "RX 7900 XTX".to_string(),
                detected_vram_mb: Some(24 * 1024),
                probe_confidence: "High".to_string(),
            },
        };

        let recommendation = recommend_llama_cpp_runtime(&input);

        assert_eq!(recommendation.recommended_gpu_layers, 24);
        assert!(!recommendation.recommended_kv_offload);
        assert!(recommendation.memory_plan.estimated_vram_mb < 24 * 1024);
        assert!(recommendation.memory_plan.estimated_ram_mb >= 24_576);
        assert!(recommendation
            .warnings
            .iter()
            .any(|warning| warning.contains("kv_cache_moved_to_ram")));
    }

    #[test]
    fn recommendation_fits_context_when_vram_and_ram_cannot_hold_requested_kv() {
        let temp = tempfile::tempdir().expect("tempdir");
        let model_path = temp.path().join("gemma4-26b-q4_k_m.gguf");
        let model = std::fs::File::create(&model_path).expect("sparse model file");
        model
            .set_len(17_862 * 1024 * 1024)
            .expect("sparse model size");
        let input = LlamaCppRuntimeInput {
            model_path: Some(model_path.to_string_lossy().into_owned()),
            mmproj_path: None,
            ctx_size: 65_536,
            requested_gpu_layers: 24,
            tuning_mode: LLAMA_TUNING_AUTO.to_string(),
            performance_profile: PROFILE_BALANCED.to_string(),
            vram_limit_gb: 24,
            ram_limit_gb: 4,
            hardware: RuntimeHardwareSummary {
                gpu_vendor: "Amd".to_string(),
                gpu_name: "RX 7900 XTX".to_string(),
                detected_vram_mb: Some(24 * 1024),
                probe_confidence: "High".to_string(),
            },
        };

        let recommendation = recommend_llama_cpp_runtime(&input);

        assert_eq!(recommendation.recommended_ctx_size, 8192);
        assert_eq!(recommendation.memory_plan.kv_cache_budget_mb, 3072);
        assert_eq!(recommendation.recommended_gpu_layers, 64);
        assert!(recommendation.recommended_kv_offload);
        assert!(
            recommendation.memory_plan.estimated_vram_mb
                + recommendation.memory_plan.safety_margin_mb
                <= 24 * 1024
        );
        assert!(recommendation.memory_plan.estimated_ram_mb <= 4 * 1024);
        assert!(!recommendation
            .warnings
            .iter()
            .any(|warning| warning.contains("kv_cache_has_no_budget_compliant_location")));
    }

    #[test]
    fn low_vram_profile_preserves_requested_context() {
        let input = LlamaCppRuntimeInput {
            model_path: None,
            mmproj_path: None,
            ctx_size: 32768,
            requested_gpu_layers: 24,
            tuning_mode: LLAMA_TUNING_AUTO.to_string(),
            performance_profile: PROFILE_LOW_VRAM.to_string(),
            vram_limit_gb: 8,
            ram_limit_gb: 32,
            hardware: RuntimeHardwareSummary {
                gpu_vendor: "Unknown".to_string(),
                gpu_name: "unknown".to_string(),
                detected_vram_mb: None,
                probe_confidence: "Low".to_string(),
            },
        };

        let recommendation = recommend_llama_cpp_runtime(&input);
        assert_eq!(recommendation.recommended_ctx_size, 32768);
        assert_eq!(recommendation.recommended_ubatch_size, 256);
    }

    #[test]
    fn manual_tuning_never_applies_auto_recommendation() {
        assert!(!should_apply_llama_cpp_auto_recommendation(
            LLAMA_TUNING_MANUAL
        ));
        assert!(!should_apply_llama_cpp_auto_recommendation(" Manual "));
        assert!(should_apply_llama_cpp_auto_recommendation(
            LLAMA_TUNING_AUTO
        ));
        assert!(should_apply_llama_cpp_auto_recommendation(""));
    }

    #[test]
    fn qwen_reasoning_compatibility_preserves_disabled_thinking() {
        let compatibility = resolve_llama_cpp_reasoning_compatibility(
            Some("/models/Qwen3.6-27B-IQ4_XS.gguf"),
            "false",
            "none",
        );

        assert_eq!(compatibility.model_family, "qwen");
        assert_eq!(compatibility.reasoning_mode, "off");
        assert_eq!(compatibility.reasoning_format, "none");
        assert!(compatibility.adjusted);
    }

    #[test]
    fn qwen_reasoning_compatibility_preserves_explicit_thinking_off() {
        let compatibility = resolve_llama_cpp_reasoning_compatibility(
            Some("/models/Qwen3.6-27B-IQ4_XS.gguf"),
            "off",
            "none",
        );

        assert_eq!(compatibility.model_family, "qwen");
        assert_eq!(compatibility.reasoning_mode, "off");
        assert_eq!(compatibility.reasoning_format, "none");
        assert!(!compatibility.adjusted);
    }

    #[test]
    fn qwen_reasoning_compatibility_repairs_auto_format_for_thinking() {
        let compatibility = resolve_llama_cpp_reasoning_compatibility(
            Some("/models/Qwen3.6-27B-IQ4_XS.gguf"),
            "auto",
            "auto",
        );

        assert_eq!(compatibility.model_family, "qwen");
        assert_eq!(compatibility.reasoning_mode, "auto");
        assert_eq!(compatibility.reasoning_format, "deepseek");
        assert!(compatibility.adjusted);
    }

    #[test]
    fn non_qwen_reasoning_compatibility_canonicalizes_disabled_values() {
        let compatibility = resolve_llama_cpp_reasoning_compatibility(
            Some("/models/gemma4-e4b-q2k.gguf"),
            "false",
            "none",
        );

        assert_eq!(compatibility.model_family, "gemma");
        assert_eq!(compatibility.reasoning_mode, "off");
        assert_eq!(compatibility.reasoning_format, "none");
        assert!(compatibility.adjusted);
    }

    #[test]
    fn non_qwen_reasoning_compatibility_keeps_explicit_off_values() {
        let compatibility = resolve_llama_cpp_reasoning_compatibility(
            Some("/models/gemma4-e4b-q2k.gguf"),
            "off",
            "none",
        );

        assert_eq!(compatibility.model_family, "gemma");
        assert_eq!(compatibility.reasoning_mode, "off");
        assert_eq!(compatibility.reasoning_format, "none");
        assert!(!compatibility.adjusted);
    }

    #[test]
    fn kv_cache_estimate_scales_with_large_context() {
        assert_eq!(
            estimate_llama_kv_cache_budget_mb(131_072, PROFILE_LOW_VRAM),
            49_152
        );
        assert_eq!(
            estimate_llama_kv_cache_budget_mb(65_536, PROFILE_BALANCED),
            24_576
        );
    }

    #[test]
    fn effective_diagnostics_puts_kv_on_ram_when_not_offloaded() {
        let input = LlamaCppRuntimeInput {
            model_path: None,
            mmproj_path: None,
            ctx_size: 131_072,
            requested_gpu_layers: 20,
            tuning_mode: LLAMA_TUNING_MANUAL.to_string(),
            performance_profile: PROFILE_LOW_VRAM.to_string(),
            vram_limit_gb: 24,
            ram_limit_gb: 64,
            hardware: RuntimeHardwareSummary {
                gpu_vendor: "Amd".to_string(),
                gpu_name: "RX 7900 XTX".to_string(),
                detected_vram_mb: Some(24 * 1024),
                probe_confidence: "High".to_string(),
            },
        };
        let recommendation = recommend_llama_cpp_runtime(&input);

        let diagnostics =
            build_effective_diagnostics_with_runtime(&input, &recommendation, 20, 131_072, false);
        let plan = diagnostics.effective_memory_plan.unwrap();

        assert_eq!(diagnostics.effective_kv_location, "RAM");
        assert_eq!(plan.kv_cache_budget_mb, 49_152);
        assert!(plan.estimated_ram_mb >= 49_152);
        assert!(plan.estimated_vram_mb < 49_152);
    }

    #[test]
    fn effective_diagnostics_puts_kv_on_vram_when_offloaded() {
        let input = LlamaCppRuntimeInput {
            model_path: None,
            mmproj_path: None,
            ctx_size: 131_072,
            requested_gpu_layers: 20,
            tuning_mode: LLAMA_TUNING_MANUAL.to_string(),
            performance_profile: PROFILE_LOW_VRAM.to_string(),
            vram_limit_gb: 80,
            ram_limit_gb: 64,
            hardware: RuntimeHardwareSummary {
                gpu_vendor: "Nvidia".to_string(),
                gpu_name: "large gpu".to_string(),
                detected_vram_mb: Some(80 * 1024),
                probe_confidence: "High".to_string(),
            },
        };
        let recommendation = recommend_llama_cpp_runtime(&input);

        let diagnostics =
            build_effective_diagnostics_with_runtime(&input, &recommendation, 20, 131_072, true);
        let plan = diagnostics.effective_memory_plan.unwrap();

        assert_eq!(diagnostics.effective_kv_location, "VRAM");
        assert_eq!(plan.kv_cache_budget_mb, 49_152);
        assert!(plan.estimated_vram_mb >= 49_152);
        assert!(plan.estimated_ram_mb < 49_152);
    }
}
