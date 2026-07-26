//! Llama.cpp based native inference backend for GGUF models.
//! Supports Vulkan/CUDA/Metal acceleration for "Out-of-the-box" GPU performance.

use crate::backend::{GenerationConfig, InferenceError, ModelBackend, Result};
use crate::engine::KvEngine;
use async_trait::async_trait;
use dashmap::DashMap;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::mtmd::{
    mtmd_default_marker, MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText,
};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use parking_lot::Mutex;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Maximum number of concurrent active sessions to prevent OOM
const MAX_SESSIONS: usize = 8;
/// VRAM threshold (percentage) to trigger session cleanup
const VRAM_CLEANUP_THRESHOLD: f32 = 0.90;
const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
const CONTEXT_ALIGNMENT_TOKENS: u32 = 512;
const DEFAULT_GPU_MIN_CONTEXT_TOKENS: u32 = 4096;
const DEFAULT_SHARED_GPU_MIN_CONTEXT_TOKENS: u32 = 2048;
const DEFAULT_CPU_MIN_CONTEXT_TOKENS: u32 = 1024;
const LARGE_MULTIMODAL_MODEL_BYTES: u64 = 10 * GIB;
const CONSTRAINED_DEDICATED_VRAM_MB: u64 = 24 * 1024;

#[derive(Debug, Clone, Copy)]
struct ContextWindowFitTelemetry {
    prompt_tokens: usize,
    trimmed_tokens: usize,
    context_limit: usize,
    prompt_budget: usize,
    reserved_generation: usize,
    context_pressure: f32,
}

fn llama_cpp_gpu_layers(hw: &crate::hardware::HardwareStatus) -> usize {
    llama_cpp_gpu_layers_for_budget(hw, 0, 0)
}

fn llama_cpp_gpu_layers_override() -> Option<usize> {
    std::env::var("BENSHU_LLAMA_CPP_GPU_LAYERS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
}

fn llama_cpp_large_multimodal_layer_cap(
    hw: &crate::hardware::HardwareStatus,
    estimated_model_bytes: u64,
    estimated_mmproj_bytes: u64,
) -> Option<usize> {
    let is_large_multimodal =
        estimated_model_bytes >= LARGE_MULTIMODAL_MODEL_BYTES && estimated_mmproj_bytes > 0;
    let constrained_dedicated_gpu = matches!(
        hw.memory_topology,
        crate::hardware::MemoryTopology::DedicatedGpu
    ) && hw.vram_total_mb > 0
        && hw.vram_total_mb <= CONSTRAINED_DEDICATED_VRAM_MB;

    if is_large_multimodal && constrained_dedicated_gpu {
        Some(24)
    } else {
        None
    }
}

fn llama_cpp_gpu_layers_for_budget(
    hw: &crate::hardware::HardwareStatus,
    estimated_model_bytes: u64,
    estimated_mmproj_bytes: u64,
) -> usize {
    let supports_gpu_offload =
        if matches!(hw.gpu_vendor, Some(crate::hardware::GpuVendor::Amd)) && hw.rocm_available {
            true
        } else {
            matches!(
                hw.acceleration_profile(),
                crate::hardware::AccelerationProfile::CudaPreferred
                    | crate::hardware::AccelerationProfile::VulkanPreferred
            )
        };

    if !supports_gpu_offload {
        return 0;
    }

    let budgets = hw.budgets();
    let used_vram_bytes = hw.vram_used_mb.saturating_mul(MIB);
    let available_vram_budget_bytes = budgets.max_vram_bytes.saturating_sub(used_vram_bytes);
    let estimated_weight_bytes = estimated_model_bytes.saturating_add(estimated_mmproj_bytes);

    if estimated_weight_bytes == 0 {
        return match hw.memory_topology {
            crate::hardware::MemoryTopology::DedicatedGpu
            | crate::hardware::MemoryTopology::UnifiedMemory => 100,
            crate::hardware::MemoryTopology::SharedGpu => 24,
            crate::hardware::MemoryTopology::CpuOnly => 0,
        };
    }

    let reserve_bytes = match hw.memory_topology {
        crate::hardware::MemoryTopology::DedicatedGpu => {
            if estimated_mmproj_bytes > 0 {
                2 * GIB
            } else {
                GIB
            }
        }
        crate::hardware::MemoryTopology::UnifiedMemory => 2 * GIB,
        crate::hardware::MemoryTopology::SharedGpu => 3 * GIB,
        crate::hardware::MemoryTopology::CpuOnly => return 0,
    };

    let offload_budget_bytes = available_vram_budget_bytes.saturating_sub(reserve_bytes);
    let offload_ratio = offload_budget_bytes as f64 / estimated_weight_bytes.max(1) as f64;

    match hw.memory_topology {
        crate::hardware::MemoryTopology::DedicatedGpu
        | crate::hardware::MemoryTopology::UnifiedMemory => {
            if offload_ratio >= 1.20 {
                100
            } else if offload_ratio >= 1.00 {
                80
            } else if offload_ratio >= 0.85 {
                64
            } else if offload_ratio >= 0.70 {
                48
            } else if offload_ratio >= 0.55 {
                32
            } else if offload_ratio >= 0.40 {
                24
            } else if offload_ratio >= 0.25 {
                16
            } else {
                0
            }
        }
        crate::hardware::MemoryTopology::SharedGpu => {
            if offload_ratio >= 0.75 {
                24
            } else if offload_ratio >= 0.55 {
                16
            } else if offload_ratio >= 0.35 {
                8
            } else {
                0
            }
        }
        crate::hardware::MemoryTopology::CpuOnly => 0,
    }
}

/// A wrapper to make LlamaContext Send/Sync so it can be stored in DashMap and moved to spawn_blocking
/// A thread-safe wrapper for LlamaContext using a Mutex to ensure exclusive access.
struct LlamaContextWrapper(Mutex<llama_cpp_2::context::LlamaContext<'static>>);
// Safe to send because the Mutex provides the necessary synchronization for the underlying C pointers.
unsafe impl Send for LlamaContextWrapper {}
unsafe impl Sync for LlamaContextWrapper {}

/// Session state for llama.cpp context management
struct LlamaSession {
    context: LlamaContextWrapper,
    model: Arc<LlamaModel>, // Hold Arc to ensure lifetime
    backend: Arc<LlamaBackend>,
    tokens: Vec<LlamaToken>,
    last_used: Instant,
    priority: i8,
    session_reset_count: u64,
    context_trim_count: u64,
    prefix_reuse_hits: u64,
    prefix_reuse_misses: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LlamaRequestProfile {
    Standard,
    MultimodalVision,
}

pub struct LlamaCppBackend {
    model: Arc<LlamaModel>,
    backend: Arc<LlamaBackend>,
    model_path: PathBuf,
    sessions: DashMap<String, Arc<Mutex<LlamaSession>>>,
    mtmd: Option<Arc<Mutex<MtmdContext>>>,
    mmproj_path: Option<PathBuf>,
    hw_status: crate::hardware::HardwareStatus,
    runtime_n_ctx: u32,
    model_train_n_ctx: u32,
    estimated_model_bytes: u64,
    estimated_kv_bytes_per_token: u64,
}

impl LlamaCppBackend {
    fn shared_backend() -> Result<Arc<LlamaBackend>> {
        static LLAMA_BACKEND: OnceLock<std::result::Result<Arc<LlamaBackend>, String>> =
            OnceLock::new();

        match LLAMA_BACKEND.get_or_init(|| {
            LlamaBackend::init()
                .map(Arc::new)
                .map_err(|e| format!("Failed to init llama.cpp: {}", e))
        }) {
            Ok(backend) => Ok(Arc::clone(backend)),
            Err(message) => Err(InferenceError::Internal(message.clone())),
        }
    }

    fn multimodal_system_preamble(task: crate::backend::VisionTask) -> &'static str {
        match task {
            crate::backend::VisionTask::Describe => {
                "System: 你是 BenShu 的本地视觉分析器。请只描述图片中真实可见的内容，不要复述规则，不要解释流程。"
            }
            crate::backend::VisionTask::OCR => {
                "System: 你是 BenShu 的本地 OCR 视觉分析器。请只提取图片中清晰可见的文字，不要补充解释。"
            }
            crate::backend::VisionTask::Grounding => {
                "System: 你是 BenShu 的本地视觉定位分析器。请识别关键对象并简洁说明它们的大致位置。"
            }
        }
    }

    fn multimodal_output_contract(task: crate::backend::VisionTask) -> &'static str {
        match task {
            crate::backend::VisionTask::Describe => {
                "输出要求：直接给出图片描述；如果确实无法判断，再简短回答“不确定”。"
            }
            crate::backend::VisionTask::OCR => {
                "输出要求：只输出识别到的文字；如果没有清晰文字，再回答“未发现清晰可识别文字”。"
            }
            crate::backend::VisionTask::Grounding => {
                "回答要求：用简短中文说明主要对象和相对位置；如果位置不确定，就回答“不确定”。"
            }
        }
    }

    fn build_vision_instruction(task: crate::backend::VisionTask, user_prompt: &str) -> String {
        format!(
            "{system}\n{contract}\n\nUser: {user_prompt}\nAssistant:",
            system = Self::multimodal_system_preamble(task),
            contract = Self::multimodal_output_contract(task),
        )
    }

    fn multimodal_context_cap(hw: &crate::hardware::HardwareStatus) -> u32 {
        if hw.has_gpu
            && matches!(
                hw.memory_topology,
                crate::hardware::MemoryTopology::DedicatedGpu
            )
        {
            4_096
        } else if hw.has_gpu {
            2_048
        } else {
            1_024
        }
    }

    fn discover_mmproj_path(
        model_path: &Path,
        explicit_mmproj_path: Option<&PathBuf>,
    ) -> Option<PathBuf> {
        if let Some(path) = explicit_mmproj_path {
            if path.exists() {
                return Some(path.clone());
            }
        }

        let parent = model_path.parent()?;
        let stem = model_path.file_stem()?.to_string_lossy().to_string();
        let extension = model_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("gguf");

        let mut candidates = vec![
            parent.join(format!("{stem}.mmproj.{extension}")),
            parent.join(format!("{stem}-mmproj.{extension}")),
            parent.join(format!("{stem}_mmproj.{extension}")),
            parent.join(format!("mmproj-{stem}.{extension}")),
            parent.join(format!("mmproj.{extension}")),
            parent.join(format!("{stem}.mmproj.safetensors")),
            parent.join(format!("{stem}-mmproj.safetensors")),
            parent.join(format!("{stem}_mmproj.safetensors")),
            parent.join("mmproj.safetensors"),
        ];

        let replaced = stem.replace("model", "mmproj");
        if replaced != stem {
            candidates.push(parent.join(format!("{replaced}.gguf")));
            candidates.push(parent.join(format!("{replaced}.safetensors")));
        }

        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let lowered = name.to_lowercase();
                let valid_extension = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| matches!(e, "gguf" | "safetensors"))
                    .unwrap_or(false);
                if valid_extension && lowered.contains("mmproj") {
                    candidates.push(path);
                }
            }
        }

        candidates.into_iter().find(|candidate| candidate.exists())
    }

    fn is_ephemeral_session(session_id: &str) -> bool {
        session_id.starts_with("native-ephemeral-") || session_id.starts_with("ephemeral-")
    }

    fn build_multimodal_prompt(prompt: &str, media_count: usize) -> String {
        if media_count == 0 {
            return prompt.to_string();
        }

        let marker = mtmd_default_marker();
        if prompt.contains(marker) {
            return prompt.to_string();
        }

        let prefix = std::iter::repeat_n(marker, media_count)
            .collect::<Vec<_>>()
            .join("\n");
        format!("{prefix}\n{prompt}")
    }

    fn build_multimodal_chat_prompt(
        &self,
        task: crate::backend::VisionTask,
        user_prompt: &str,
        media_count: usize,
    ) -> String {
        let user_content = Self::build_multimodal_prompt(user_prompt, media_count);
        let system_content = format!(
            "{}\n{}",
            Self::multimodal_system_preamble(task),
            Self::multimodal_output_contract(task)
        );

        if let Ok(template) = self.model.chat_template(None) {
            let system_message =
                LlamaChatMessage::new("system".to_string(), system_content.clone());
            let user_message = LlamaChatMessage::new("user".to_string(), user_content.clone());

            if let (Ok(system_message), Ok(user_message)) = (system_message, user_message) {
                if let Ok(rendered) =
                    self.model
                        .apply_chat_template(&template, &[system_message, user_message], true)
                {
                    return rendered;
                }
            }
        }

        format!("{system_content}\n\nUser: {user_content}\nAssistant:")
    }

    fn vision_generation_config(
        task: crate::backend::VisionTask,
        config: Option<crate::backend::GenerationConfig>,
    ) -> crate::backend::GenerationConfig {
        let mut effective = config.unwrap_or_default();

        effective.max_new_tokens = effective.max_new_tokens.min(match task {
            crate::backend::VisionTask::Describe => 96,
            crate::backend::VisionTask::OCR => 160,
            crate::backend::VisionTask::Grounding => 128,
        });
        effective.temperature = effective.temperature.min(0.2);
        effective.top_p = effective.top_p.min(0.35);

        effective
    }

    fn minimum_runtime_n_ctx(hw: &crate::hardware::HardwareStatus) -> u32 {
        if hw.has_gpu
            && matches!(
                hw.memory_topology,
                crate::hardware::MemoryTopology::DedicatedGpu
            )
        {
            DEFAULT_GPU_MIN_CONTEXT_TOKENS
        } else if hw.has_gpu {
            DEFAULT_SHARED_GPU_MIN_CONTEXT_TOKENS
        } else {
            DEFAULT_CPU_MIN_CONTEXT_TOKENS
        }
    }

    fn hardware_context_cap(hw: &crate::hardware::HardwareStatus) -> u32 {
        if hw.has_gpu
            && matches!(
                hw.memory_topology,
                crate::hardware::MemoryTopology::DedicatedGpu
            )
        {
            match hw.vram_total_mb {
                mb if mb >= 48 * 1024 => 65_536,
                mb if mb >= 20 * 1024 => 32_768,
                mb if mb >= 12 * 1024 => 16_384,
                mb if mb >= 8 * 1024 => 8_192,
                _ => DEFAULT_GPU_MIN_CONTEXT_TOKENS,
            }
        } else if hw.has_gpu {
            8_192
        } else if hw.ram_total_mb >= 64 * 1024 {
            8_192
        } else if hw.ram_total_mb >= 32 * 1024 {
            4_096
        } else {
            DEFAULT_CPU_MIN_CONTEXT_TOKENS
        }
    }

    fn align_context_tokens(target: u32, minimum: u32) -> u32 {
        if target <= minimum {
            return minimum;
        }

        let aligned = target / CONTEXT_ALIGNMENT_TOKENS * CONTEXT_ALIGNMENT_TOKENS;
        aligned.max(minimum)
    }

    fn infer_split_model_prefix(filename: &str) -> Option<&str> {
        filename.find("-00001-of-").map(|idx| &filename[..idx])
    }

    fn estimate_model_bytes_from_path(model_path: &Path) -> u64 {
        let Some(file_name) = model_path.file_name().and_then(|name| name.to_str()) else {
            return std::fs::metadata(model_path)
                .map(|meta| meta.len())
                .unwrap_or(0);
        };

        let Some(prefix) = Self::infer_split_model_prefix(file_name) else {
            return std::fs::metadata(model_path)
                .map(|meta| meta.len())
                .unwrap_or(0);
        };

        let Some(parent) = model_path.parent() else {
            return std::fs::metadata(model_path)
                .map(|meta| meta.len())
                .unwrap_or(0);
        };

        std::fs::read_dir(parent)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(std::result::Result::ok))
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with(prefix) && name.ends_with(".gguf"))
                    .unwrap_or(false)
            })
            .filter_map(|entry| entry.metadata().ok().map(|meta| meta.len()))
            .sum::<u64>()
    }

    fn estimate_auxiliary_bytes_from_path(path: Option<&Path>) -> u64 {
        path.and_then(|resolved| std::fs::metadata(resolved).ok().map(|meta| meta.len()))
            .unwrap_or(0)
    }

    fn estimated_kv_bytes_per_token_for_model(model: &LlamaModel) -> u64 {
        let n_layer = u64::from(model.n_layer().max(1));
        let n_head = u64::from(model.n_head().max(1));
        let n_head_kv = u64::from(model.n_head_kv().max(1));
        let n_embd = model.n_embd().max(1) as u64;
        let head_dim = (n_embd / n_head).max(1);

        2 * 2 * n_layer * n_head_kv * head_dim
    }

    fn select_runtime_n_ctx(
        model_train_n_ctx: u32,
        estimated_model_bytes: u64,
        estimated_kv_bytes_per_token: u64,
        hw: &crate::hardware::HardwareStatus,
    ) -> u32 {
        let minimum = Self::minimum_runtime_n_ctx(hw);
        let fallback = if hw.has_gpu
            && matches!(
                hw.memory_topology,
                crate::hardware::MemoryTopology::DedicatedGpu
            ) {
            16_384
        } else if hw.has_gpu {
            8_192
        } else {
            2_048
        };
        let hardware_cap = Self::hardware_context_cap(hw).min(model_train_n_ctx);

        if model_train_n_ctx <= minimum {
            return model_train_n_ctx.max(512);
        }

        let budgets = hw.budgets();
        let dedicated_gpu = budgets.separate_vram_pool && budgets.max_vram_bytes > 0;
        let memory_budget_bytes = if dedicated_gpu {
            budgets.max_vram_bytes
        } else {
            budgets.max_ram_bytes
        };
        let safety_reserve_bytes = if dedicated_gpu { GIB } else { 2 * GIB };
        let kv_budget_bytes = memory_budget_bytes
            .saturating_sub(estimated_model_bytes.saturating_add(safety_reserve_bytes));

        let budget_limited_ctx = if estimated_kv_bytes_per_token > 0 && kv_budget_bytes > 0 {
            (kv_budget_bytes / estimated_kv_bytes_per_token) as u32
        } else {
            0
        };

        let candidate = if budget_limited_ctx > 0 {
            budget_limited_ctx.min(model_train_n_ctx)
        } else {
            fallback.min(model_train_n_ctx)
        };

        Self::align_context_tokens(
            candidate
                .max(minimum.min(model_train_n_ctx))
                .min(hardware_cap.max(minimum)),
            minimum,
        )
        .min(hardware_cap.max(minimum))
        .max(512)
    }

    fn currently_available_memory_bytes(hw: &crate::hardware::HardwareStatus) -> u64 {
        if hw.has_gpu
            && matches!(
                hw.memory_topology,
                crate::hardware::MemoryTopology::DedicatedGpu
            )
            && hw.vram_total_mb > 0
        {
            hw.vram_total_mb
                .saturating_sub(hw.vram_used_mb)
                .saturating_mul(MIB)
        } else if hw.ram_total_mb > 0 {
            hw.ram_total_mb.saturating_mul(MIB)
        } else {
            0
        }
    }

    fn session_target_n_ctx_for_profile(&self, profile: LlamaRequestProfile) -> u32 {
        let hw = crate::hardware::HardwareStatus::detect();
        let minimum = Self::minimum_runtime_n_ctx(&hw).min(self.model_train_n_ctx.max(512));
        let hardware_cap = Self::hardware_context_cap(&hw)
            .min(self.runtime_n_ctx)
            .min(self.model_train_n_ctx.max(minimum));
        let available_memory_bytes = Self::currently_available_memory_bytes(&hw);
        let active_sessions = self.sessions.len() as u64;
        let session_divisor = active_sessions.saturating_add(1).max(1);
        let per_session_budget_bytes = available_memory_bytes / session_divisor;
        let reserve_bytes = if hw.has_gpu
            && matches!(
                hw.memory_topology,
                crate::hardware::MemoryTopology::DedicatedGpu
            ) {
            2 * GIB
        } else {
            4 * GIB
        };
        let compute_headroom_bytes = (self.estimated_model_bytes / 8).max(512 * MIB);
        let kv_budget_bytes = per_session_budget_bytes
            .saturating_sub(reserve_bytes)
            .saturating_sub(compute_headroom_bytes);

        let candidate = if self.estimated_kv_bytes_per_token > 0 && kv_budget_bytes > 0 {
            (kv_budget_bytes / self.estimated_kv_bytes_per_token) as u32
        } else {
            hardware_cap
        };

        let effective_cap = match profile {
            LlamaRequestProfile::Standard => hardware_cap.max(minimum),
            LlamaRequestProfile::MultimodalVision => {
                let multimodal_cap = Self::multimodal_context_cap(&hw)
                    .min(self.runtime_n_ctx)
                    .min(self.model_train_n_ctx.max(minimum));
                multimodal_cap.max(minimum)
            }
        };

        Self::align_context_tokens(candidate.max(minimum), minimum)
            .min(effective_cap)
            .max(512)
    }

    fn context_retry_plan(&self, target_n_ctx: u32, profile: LlamaRequestProfile) -> Vec<u32> {
        let minimum = Self::minimum_runtime_n_ctx(&crate::hardware::HardwareStatus::detect())
            .min(self.model_train_n_ctx.max(512));
        let profile_cap = match profile {
            LlamaRequestProfile::Standard => self.runtime_n_ctx.max(minimum),
            LlamaRequestProfile::MultimodalVision => {
                let hw = crate::hardware::HardwareStatus::detect();
                Self::multimodal_context_cap(&hw)
                    .min(self.runtime_n_ctx)
                    .min(self.model_train_n_ctx.max(minimum))
                    .max(minimum)
            }
        };
        let mut candidates = Vec::new();
        let mut current = target_n_ctx.max(minimum);

        loop {
            let aligned = Self::align_context_tokens(current, minimum)
                .min(profile_cap)
                .min(self.model_train_n_ctx.max(minimum));
            if !candidates.contains(&aligned) {
                candidates.push(aligned);
            }
            if aligned <= minimum {
                break;
            }

            let halved = aligned / 2;
            current = if halved <= minimum { minimum } else { halved };
        }

        candidates
    }

    fn estimate_kv_bytes_for_tokens(&self, token_count: usize) -> u64 {
        self.estimated_kv_bytes_per_token
            .saturating_mul(token_count as u64)
    }

    fn push_unique_stop_sequence(stop_sequences: &mut Vec<String>, stop: impl Into<String>) {
        let stop = stop.into();
        if !stop.is_empty() && !stop_sequences.iter().any(|existing| existing == &stop) {
            stop_sequences.push(stop);
        }
    }

    fn effective_stop_sequences(prompt: &str, config: &GenerationConfig) -> Vec<String> {
        let mut stop_sequences = config.stop_sequences.clone();

        for marker in ["[CRITIQUE", "Final Answer:", "<|end|>", "<|im_end|>"] {
            Self::push_unique_stop_sequence(&mut stop_sequences, marker);
        }

        if prompt.contains("<|assistant|>") || prompt.contains("<|user|>") {
            for marker in ["<|assistant|>", "<|user|>", "<|system|>"] {
                Self::push_unique_stop_sequence(&mut stop_sequences, marker);
            }
        }

        if prompt.contains("Assistant:") || prompt.contains("User:") {
            for marker in ["\nAssistant:", "\nUser:", "\nSystem:"] {
                Self::push_unique_stop_sequence(&mut stop_sequences, marker);
            }
        }

        stop_sequences
    }

    fn find_earliest_stop(text: &str, stop_sequences: &[String]) -> Option<usize> {
        stop_sequences
            .iter()
            .filter(|stop| !stop.is_empty())
            .filter_map(|stop| text.find(stop))
            .min()
    }

    fn trailing_stop_overlap_len(text: &str, stop_sequences: &[String]) -> usize {
        stop_sequences
            .iter()
            .filter(|stop| !stop.is_empty())
            .flat_map(|stop| {
                let max_len = stop.len().min(text.len());
                (1..=max_len).rev().filter_map(move |candidate_len| {
                    let text_start = text.len().saturating_sub(candidate_len);
                    if !text.is_char_boundary(text_start) || !stop.is_char_boundary(candidate_len) {
                        return None;
                    }

                    let text_suffix = &text[text_start..];
                    let stop_prefix = &stop[..candidate_len];
                    if text_suffix == stop_prefix {
                        Some(candidate_len)
                    } else {
                        None
                    }
                })
            })
            .max()
            .unwrap_or(0)
    }

    fn cleanup_output_suffix(text: &str) -> String {
        let mut cleaned = text.trim_end().to_string();

        loop {
            let trimmed = cleaned.trim_end().to_string();
            let mut changed = false;
            for suffix in [
                "---",
                "Assistant:",
                "Assistant",
                "User:",
                "User",
                "<|assistant|>",
                "<|user|>",
                "<|system|>",
                "<|im_end|>",
                "<|end|>",
            ] {
                if trimmed.ends_with(suffix) {
                    cleaned = trimmed[..trimmed.len() - suffix.len()]
                        .trim_end()
                        .to_string();
                    changed = true;
                    break;
                }
            }

            if !changed {
                return trimmed;
            }
        }
    }

    fn cleanup_output_prefix(text: &str) -> String {
        let mut cleaned = text.trim_start().to_string();

        loop {
            let trimmed = cleaned.trim_start().to_string();
            let mut changed = false;

            if let Some(rest) = trimmed.strip_prefix("<|channel>") {
                if let Some(end_idx) = rest.find("<channel|>") {
                    cleaned = rest[end_idx + "<channel|>".len()..]
                        .trim_start()
                        .to_string();
                    changed = true;
                }
            }

            if changed {
                continue;
            }

            for prefix in [
                "<|assistant|>",
                "<|user|>",
                "<|system|>",
                "<|im_end|>",
                "<|end|>",
                "Assistant:",
                "User:",
                "System:",
            ] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    cleaned = rest.trim_start().to_string();
                    changed = true;
                    break;
                }
            }

            if !changed {
                return trimmed;
            }
        }
    }

    fn finalize_generated_output(text: &str, stop_sequences: &[String]) -> String {
        let truncated = match Self::find_earliest_stop(text, stop_sequences) {
            Some(stop_idx) => &text[..stop_idx],
            None => text,
        };
        let without_prefix = Self::cleanup_output_prefix(truncated);
        Self::cleanup_output_suffix(&without_prefix)
    }

    fn normalize_echo_text(text: &str) -> String {
        text.split_whitespace().collect::<String>().to_lowercase()
    }

    fn looks_like_prompt_echo(output: &str, prompt: &str) -> bool {
        let trimmed = output.trim();
        if trimmed.is_empty() {
            return false;
        }

        let normalized_output = Self::normalize_echo_text(trimmed);
        if normalized_output.is_empty() {
            return false;
        }

        let normalized_prompt = Self::normalize_echo_text(prompt);
        if normalized_prompt.contains(&normalized_output) {
            return true;
        }

        [
            "你是一个有用的助手",
            "请用中文简洁描述用户提供的图片内容",
            "youareahelpfulaiassistant",
            "describetheimage",
            "directlyanswertheimagecontent",
        ]
        .iter()
        .any(|marker| normalized_output.contains(marker))
    }

    pub fn new(model_path: impl Into<PathBuf>, mmproj_path: Option<PathBuf>) -> Result<Self> {
        let backend = Self::shared_backend()?;

        let hw = crate::hardware::HardwareStatus::detect();
        let path = model_path.into();
        let effective_mmproj = Self::discover_mmproj_path(&path, mmproj_path.as_ref());
        let estimated_model_bytes = Self::estimate_model_bytes_from_path(&path);
        let estimated_mmproj_bytes =
            Self::estimate_auxiliary_bytes_from_path(effective_mmproj.as_deref());
        let adaptive_gpu_layers =
            llama_cpp_gpu_layers_for_budget(&hw, estimated_model_bytes, estimated_mmproj_bytes);
        let gpu_layers = if let Some(override_layers) = llama_cpp_gpu_layers_override() {
            override_layers
        } else if let Some(cap) =
            llama_cpp_large_multimodal_layer_cap(&hw, estimated_model_bytes, estimated_mmproj_bytes)
        {
            adaptive_gpu_layers.min(cap)
        } else {
            adaptive_gpu_layers
        };

        if gpu_layers > 0 {
            info!(
                "🎮 GPU acceleration candidate detected: {} ({:?}, {:?}). Enabling adaptive llama.cpp hardware offloading with gpu_layers={} (estimated_model_mb={}, estimated_mmproj_mb={}, vram_used_mb={}, vram_budget_mb={:?}).",
                hw.gpu_name.clone().unwrap_or_else(|| "Generic GPU".to_string()),
                hw.gpu_vendor,
                hw.gpu_probe_confidence
                ,
                gpu_layers,
                estimated_model_bytes / MIB,
                estimated_mmproj_bytes / MIB,
                hw.vram_used_mb,
                hw.vram_budget_mb
            );

            if let Some(override_layers) = llama_cpp_gpu_layers_override() {
                info!(
                    "🛠️ BENSHU_LLAMA_CPP_GPU_LAYERS override active. Using gpu_layers={} instead of adaptive recommendation {}.",
                    override_layers,
                    adaptive_gpu_layers
                );
            } else if let Some(cap) = llama_cpp_large_multimodal_layer_cap(
                &hw,
                estimated_model_bytes,
                estimated_mmproj_bytes,
            ) {
                if adaptive_gpu_layers > cap {
                    info!(
                        "🧯 Large multimodal model safety cap applied on constrained VRAM. Capping gpu_layers from {} to {} (vram_total_mb={}, estimated_model_mb={}, estimated_mmproj_mb={}).",
                        adaptive_gpu_layers,
                        cap,
                        hw.vram_total_mb,
                        estimated_model_bytes / MIB,
                        estimated_mmproj_bytes / MIB
                    );
                }
            }
        } else {
            info!(
                "💻 No llama.cpp GPU offload path selected ({:?}, Vulkan={}, ROCm={}, estimated_model_mb={}, estimated_mmproj_mb={}, vram_used_mb={}, vram_budget_mb={:?}). Running in optimized CPU mode on {} cores.",
                hw.acceleration_profile(),
                hw.vulkan_supported,
                hw.rocm_available,
                estimated_model_bytes / MIB,
                estimated_mmproj_bytes / MIB,
                hw.vram_used_mb,
                hw.vram_budget_mb,
                hw.cpu_cores
            );
        }

        let model_params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers as u32);

        let is_quantized = path.extension().and_then(|e| e.to_str()) == Some("gguf");

        if let Some(ref p) = effective_mmproj {
            info!("📸 Multimodal components found at: {}.", p.display());
        }

        if is_quantized {
            info!(
                "🚀 Quantized model detected. Using SIMD-optimized KV Cache. (VRAM: {}MB used)",
                hw.vram_used_mb
            );
        }

        // 2. Load Language Model
        let model = LlamaModel::load_from_file(&backend, &path, &model_params)
            .map_err(|e| InferenceError::LoadFailed(format!("Failed to load GGUF: {}", e)))?;

        // 3. Integrated Multimodal Components (llama.cpp mtmd)
        let mtmd = if let Some(p) = effective_mmproj.as_ref() {
            let mtmd_params = MtmdContextParams {
                use_gpu: gpu_layers > 0,
                print_timings: false,
                n_threads: hw.cpu_cores.max(1) as i32,
                ..Default::default()
            };

            match MtmdContext::init_from_file(&p.to_string_lossy(), &model, &mtmd_params) {
                Ok(ctx) => {
                    if ctx.support_vision() {
                        info!(
                            "📸 llama.cpp mtmd multimodal bridge initialized from {}",
                            p.display()
                        );
                        Some(Arc::new(Mutex::new(ctx)))
                    } else {
                        warn!(
                            "⚠️ Resolved mmproj at {} but the current llama.cpp mtmd runtime does not report vision support. Falling back to text-only llama.cpp execution.",
                            p.display()
                        );
                        None
                    }
                }
                Err(e) => {
                    warn!(
                        "⚠️ Failed to initialize llama.cpp mtmd from {}: {}. Falling back to text-only llama.cpp execution.",
                        p.display(),
                        e
                    );
                    None
                }
            }
        } else {
            None
        };

        let model_train_n_ctx = model.n_ctx_train();
        let estimated_kv_bytes_per_token = Self::estimated_kv_bytes_per_token_for_model(&model);
        let runtime_n_ctx = Self::select_runtime_n_ctx(
            model_train_n_ctx,
            estimated_model_bytes,
            estimated_kv_bytes_per_token,
            &hw,
        );
        let estimated_kv_capacity_bytes =
            estimated_kv_bytes_per_token.saturating_mul(runtime_n_ctx as u64);

        info!(
            "🧠 llama.cpp runtime context configured: train_n_ctx={} runtime_n_ctx={} estimated_model_mb={} estimated_kv_capacity_mb={} kv_bytes_per_token={} budget_source={}",
            model_train_n_ctx,
            runtime_n_ctx,
            estimated_model_bytes / MIB,
            estimated_kv_capacity_bytes / MIB,
            estimated_kv_bytes_per_token,
            if hw.budgets().separate_vram_pool {
                "vram"
            } else {
                "ram"
            }
        );

        let mut backend_obj = Self {
            model: Arc::new(model),
            backend,
            model_path: path.to_path_buf(),
            sessions: DashMap::new(),
            mtmd,
            mmproj_path: effective_mmproj,
            hw_status: hw,
            runtime_n_ctx,
            model_train_n_ctx,
            estimated_model_bytes,
            estimated_kv_bytes_per_token,
        };

        // Final sanity check: warmup vision components and KV Cache
        let _ = backend_obj.warmup();

        Ok(backend_obj)
    }

    /// Helper to convert tokens back to strings (Internal)
    fn tokens_to_str(&self, tokens: &[llama_cpp_2::token::LlamaToken]) -> Result<String> {
        let mut output = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        for &token in tokens {
            let piece = self
                .model
                .token_to_piece(token, &mut decoder, true, None)
                .map_err(|e| {
                    InferenceError::execution(format!("Token piece err: {}", e), "translation")
                })?;
            output.push_str(&piece);
        }
        Ok(output)
    }

    /// Perform a dummy generation to pre-fault memory and initialize CUDA/Vulkan kernels
    pub fn warmup(&mut self) -> Result<()> {
        info!("🔥 Warmup: Initializing inference kernels...");

        // 1. Text warmup
        // 1. Text warmup (model info triggered)
        info!("Model metadata loaded: {} layers", self.model.n_layer());

        // 2. Vision warmup (if multimodal)
        if let Some(mtmd) = &self.mtmd {
            let mtmd = mtmd.lock();
            info!(
                "📸 llama.cpp mtmd warmup ready: support_vision={} support_audio={} mmproj={}",
                mtmd.support_vision(),
                mtmd.support_audio(),
                self.mmproj_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "none".to_string())
            );
        }

        Ok(())
    }

    pub fn supports_multimodal_vision(&self) -> bool {
        self.mtmd.is_some()
    }

    /// Check system VRAM and purge old sessions if over threshold
    fn monitor_vram(&self) {
        let hw = crate::hardware::HardwareStatus::detect();
        if hw.has_gpu && hw.vram_total_mb > 0 {
            let usage_ratio = hw.vram_used_mb as f32 / hw.vram_total_mb as f32;
            if usage_ratio > VRAM_CLEANUP_THRESHOLD {
                info!(
                    "⚠️ VRAM usage high ({:.1}%). Purging lower-priority inference sessions.",
                    usage_ratio * 100.0
                );

                // Sort sessions by [Priority (Desc)] then [Last Used (Asc)]
                // We want to kill high priority value (low importance) first.
                let mut session_keys: Vec<_> = self
                    .sessions
                    .iter()
                    .map(|r| {
                        let s = r.value().lock();
                        (r.key().clone(), s.priority, s.last_used)
                    })
                    .collect();

                // Sort: Higher priority value (background) comes first as eviction candidate
                session_keys.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));

                let to_purge = (session_keys.len() / 2).max(1);
                for i in 0..to_purge {
                    if let Some(meta) = session_keys.get(i) {
                        self.sessions.remove(&meta.0);
                        info!(
                            "🗑️ VRAM Arbitration: Evicted session {} (Priority: {})",
                            meta.0, meta.1
                        );
                    }
                }
            }
        }
    }

    fn get_or_create_session(
        &self,
        session_id: &str,
        priority: i8,
        profile: LlamaRequestProfile,
    ) -> Result<Arc<Mutex<LlamaSession>>> {
        // Monitor VRAM before creating or reusing a session
        self.monitor_vram();

        if let Some(s) = self.sessions.get(session_id) {
            let session = s.value().clone();
            session.lock().last_used = Instant::now();
            return Ok(session);
        }

        // LRU cleanup if too many sessions
        if self.sessions.len() >= MAX_SESSIONS {
            let oldest = self
                .sessions
                .iter()
                .min_by_key(|r| r.value().lock().last_used)
                .map(|r| r.key().clone());

            if let Some(old_key) = oldest {
                self.sessions.remove(&old_key);
            }
        }

        // Safety: We ensure model and backend live as long as the session
        // by storing the Arcs directly in LlamaSession.
        // We transmute the references to 'static because the Arcs guarantee their lifetime.
        let model = self.model.clone();
        let backend = self.backend.clone();

        let m_static: &'static LlamaModel = unsafe { std::mem::transmute(&*model) };
        let b_static: &'static LlamaBackend = unsafe { std::mem::transmute(&*backend) };

        let target_n_ctx = self.session_target_n_ctx_for_profile(profile);
        let retry_plan = self.context_retry_plan(target_n_ctx, profile);
        let mut last_error = None;
        let mut selected_n_ctx = target_n_ctx;
        let mut context = None;

        for candidate_n_ctx in retry_plan {
            let (n_batch, n_ubatch) = match profile {
                LlamaRequestProfile::Standard => {
                    (candidate_n_ctx.min(2048), candidate_n_ctx.min(512))
                }
                LlamaRequestProfile::MultimodalVision => {
                    (candidate_n_ctx.min(512), candidate_n_ctx.min(512))
                }
            };
            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(candidate_n_ctx))
                .with_n_batch(n_batch)
                .with_n_ubatch(n_ubatch);

            match m_static.new_context(b_static, ctx_params) {
                Ok(created) => {
                    selected_n_ctx = candidate_n_ctx;
                    context = Some(created);
                    break;
                }
                Err(err) => {
                    warn!(
                        "⚠️ llama.cpp context allocation failed for session {} at n_ctx={}: {}. Retrying with a smaller context window.",
                        session_id, candidate_n_ctx, err
                    );
                    last_error = Some(err.to_string());
                }
            }
        }

        let context = context.ok_or_else(|| {
            InferenceError::Internal(format!(
                "Context failed after adaptive retries (target_n_ctx={}, runtime_ceiling={}): {}",
                target_n_ctx,
                self.runtime_n_ctx,
                last_error.unwrap_or_else(|| "unknown llama.cpp allocation failure".to_string())
            ))
        })?;

        if selected_n_ctx < target_n_ctx {
            info!(
                "📉 llama.cpp adapted session {} from requested n_ctx={} down to n_ctx={} based on live memory pressure.",
                session_id, target_n_ctx, selected_n_ctx
            );
        }

        let session = Arc::new(Mutex::new(LlamaSession {
            context: LlamaContextWrapper(Mutex::new(context)),
            model,
            backend,
            tokens: Vec::new(),
            last_used: Instant::now(),
            priority,
            session_reset_count: 0,
            context_trim_count: 0,
            prefix_reuse_hits: 0,
            prefix_reuse_misses: 0,
        }));

        self.sessions
            .insert(session_id.to_string(), session.clone());
        Ok(session)
    }

    /// Create a sampler based on generation config (Internal helper)
    fn create_sampler_internal(config: &GenerationConfig) -> LlamaSampler {
        if config.temperature <= 0.0 {
            LlamaSampler::greedy()
        } else {
            // Keep generation conservative to reduce repetition cascades and gibberish drift.
            let mut samplers = Vec::new();
            samplers.push(LlamaSampler::penalties(64, 1.12, 0.0, 0.0));
            samplers.push(LlamaSampler::temp(config.temperature));
            samplers.push(LlamaSampler::top_p(config.top_p, 1));
            samplers.push(LlamaSampler::dist(rand::random()));

            LlamaSampler::chain(samplers, false)
        }
    }

    fn reset_session_prefix(session: &mut LlamaSession, prefix_match: usize) -> Result<bool> {
        if prefix_match < session.tokens.len() {
            let mut ctx = session.context.0.lock();
            if prefix_match == 0 {
                ctx.clear_kv_cache();
            } else {
                let _ = ctx.clear_kv_cache_seq(Some(0), Some(prefix_match as u32), None);
            }
            session.tokens.truncate(prefix_match);
            session.session_reset_count = session.session_reset_count.saturating_add(1);
            return Ok(true);
        }
        Ok(false)
    }

    fn fit_tokens_to_context_window(
        session: &mut LlamaSession,
        tokens: Vec<LlamaToken>,
        max_new_tokens: usize,
    ) -> (Vec<LlamaToken>, ContextWindowFitTelemetry) {
        let context_limit = {
            let ctx = session.context.0.lock();
            ctx.n_ctx() as usize
        };
        let reserved_generation = max_new_tokens.max(1).min(context_limit.saturating_sub(1));
        let prompt_budget = context_limit.saturating_sub(reserved_generation).max(1);
        let original_prompt_tokens = tokens.len();

        if tokens.len() <= prompt_budget {
            return (
                tokens,
                ContextWindowFitTelemetry {
                    prompt_tokens: original_prompt_tokens,
                    trimmed_tokens: 0,
                    context_limit,
                    prompt_budget,
                    reserved_generation,
                    context_pressure: ((original_prompt_tokens + reserved_generation) as f32
                        / context_limit.max(1) as f32)
                        .min(1.0),
                },
            );
        }

        warn!(
            "⚠️ llama.cpp context window exceeded (prompt_tokens={}, budget={}, n_ctx={}). Trimming oldest prompt tokens and resetting session state.",
            tokens.len(),
            prompt_budget,
            context_limit
        );

        let trimmed = if prompt_budget == 1 {
            vec![tokens[tokens.len() - 1]]
        } else {
            let mut kept = Vec::with_capacity(prompt_budget);
            kept.push(tokens[0]);
            kept.extend_from_slice(&tokens[tokens.len() - (prompt_budget - 1)..]);
            kept
        };

        {
            let mut ctx = session.context.0.lock();
            ctx.clear_kv_cache();
        }
        session.tokens.clear();
        session.context_trim_count = session.context_trim_count.saturating_add(1);

        (
            trimmed,
            ContextWindowFitTelemetry {
                prompt_tokens: original_prompt_tokens,
                trimmed_tokens: original_prompt_tokens.saturating_sub(prompt_budget),
                context_limit,
                prompt_budget,
                reserved_generation,
                context_pressure: ((original_prompt_tokens + reserved_generation) as f32
                    / context_limit.max(1) as f32)
                    .min(1.0),
            },
        )
    }

    fn effective_max_new_tokens_for_context(context_limit: usize, requested: usize) -> usize {
        let interactive_cap = (context_limit / 8).clamp(256, 4096);
        let hard_cap = context_limit.saturating_sub(1).max(1);
        let effective = requested.max(1).min(interactive_cap).min(hard_cap);

        if effective < requested {
            warn!(
                "⚠️ llama.cpp requested max_new_tokens={} clipped to {} based on runtime context budget (n_ctx={}).",
                requested,
                effective,
                context_limit
            );
        }

        effective
    }

    fn effective_max_new_tokens(session: &LlamaSession, requested: usize) -> usize {
        let context_limit = {
            let ctx = session.context.0.lock();
            ctx.n_ctx() as usize
        };

        Self::effective_max_new_tokens_for_context(context_limit, requested)
    }

    async fn run_multimodal_completion(
        &self,
        request_id: &str,
        prompt: &str,
        images: Vec<image::DynamicImage>,
        config: GenerationConfig,
    ) -> Result<String> {
        let mtmd = self.mtmd.clone().ok_or_else(|| {
            InferenceError::InvalidInput(
                "llama.cpp multimodal runtime is not ready for this GGUF binding; configure or resolve a matching mmproj first".to_string(),
            )
        })?;

        if images.is_empty() {
            return Err(InferenceError::InvalidInput(
                "multimodal generation requires at least one image".to_string(),
            ));
        }

        let session_id = config
            .session_id
            .clone()
            .unwrap_or_else(|| format!("native-ephemeral-vision-{request_id}"));
        let should_cleanup_session = Self::is_ephemeral_session(&session_id);
        let session_lock = self.get_or_create_session(
            &session_id,
            config.priority,
            LlamaRequestProfile::MultimodalVision,
        )?;
        {
            let mut session = session_lock.lock();
            session.priority = config.priority;
        }

        let model = self.model.clone();
        let stop_sequences = Self::effective_stop_sequences(prompt, &config);
        let prompt = prompt.to_string();
        let request_id_for_task = request_id.to_string();
        let session_id_for_task = session_id.clone();

        let task_handle = tokio::task::spawn_blocking(move || {
            let mut session = session_lock.lock();
            session.last_used = Instant::now();
            let effective_max_new_tokens =
                Self::effective_max_new_tokens(&session, config.max_new_tokens);

            {
                let mut ctx = session.context.0.lock();
                ctx.clear_kv_cache();
            }
            session.tokens.clear();
            session.session_reset_count = session.session_reset_count.saturating_add(1);

            let bitmaps = images
                .iter()
                .enumerate()
                .map(|(idx, image)| {
                    let rgb = image.to_rgb8();
                    let bitmap =
                        MtmdBitmap::from_image_data(image.width(), image.height(), rgb.as_raw())
                            .map_err(|e| {
                                InferenceError::execution(
                                    format!("Failed to construct mtmd bitmap: {}", e),
                                    request_id_for_task.clone(),
                                )
                            })?;
                    let _ = bitmap.set_id(&format!("{request_id_for_task}-img-{idx}"));
                    Ok(bitmap)
                })
                .collect::<Result<Vec<_>>>()?;
            let bitmap_refs: Vec<&MtmdBitmap> = bitmaps.iter().collect();

            let multimodal_prompt = Self::build_multimodal_prompt(&prompt, bitmap_refs.len());
            let chunks = {
                let mtmd = mtmd.lock();
                mtmd.tokenize(
                    MtmdInputText {
                        text: multimodal_prompt,
                        add_special: true,
                        parse_special: true,
                    },
                    &bitmap_refs,
                )
                .map_err(|e| {
                    InferenceError::execution(
                        format!("llama.cpp mtmd tokenize failed: {}", e),
                        request_id_for_task.clone(),
                    )
                })?
            };

            let mut n_past = {
                let mtmd = mtmd.lock();
                let ctx = session.context.0.lock();
                chunks
                    .eval_chunks(&mtmd, &ctx, 0, 0, ctx.n_batch() as i32, true)
                    .map_err(|e| {
                        InferenceError::execution(
                            format!("llama.cpp mtmd eval failed: {}", e),
                            request_id_for_task.clone(),
                        )
                    })?
            };

            info!(
                "📸 llama.cpp multimodal session telemetry: request_id={} session_id={} media_count={} chunk_tokens={} chunk_positions={} max_new_tokens={} n_past={}",
                request_id_for_task,
                session_id_for_task,
                bitmap_refs.len(),
                chunks.total_tokens(),
                chunks.total_positions(),
                effective_max_new_tokens,
                n_past,
            );

            let mut sampler = Self::create_sampler_internal(&config);
            let mut output = String::new();
            let mut decoder = encoding_rs::UTF_8.new_decoder();
            let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(1, 1);

            for _ in 0..effective_max_new_tokens {
                let next_token = {
                    let ctx = session.context.0.lock();
                    sampler.sample(&ctx, -1)
                };

                if model.is_eog_token(next_token) {
                    break;
                }

                let piece = model
                    .token_to_piece(next_token, &mut decoder, true, None)
                    .map_err(|e| {
                        InferenceError::execution(
                            format!("Decode piece failed: {}", e),
                            request_id_for_task.clone(),
                        )
                    })?;
                output.push_str(&piece);
                let should_stop = Self::find_earliest_stop(&output, &stop_sequences).is_some();

                batch.clear();
                batch.add(next_token, n_past, &[0], true).map_err(|e| {
                    InferenceError::execution(
                        format!("Batch extend failed: {}", e),
                        request_id_for_task.clone(),
                    )
                })?;

                {
                    let mut ctx = session.context.0.lock();
                    ctx.decode(&mut batch).map_err(|e| {
                        InferenceError::execution(
                            format!("Mid-gen decode failed: {}", e),
                            request_id_for_task.clone(),
                        )
                    })?;
                }

                n_past += 1;
                session.tokens.push(next_token);
                sampler.accept(next_token);

                if should_stop {
                    break;
                }
            }

            let finalized = Self::finalize_generated_output(&output, &stop_sequences);
            if Self::looks_like_prompt_echo(&finalized, &prompt) {
                warn!(
                    "⚠️ llama.cpp multimodal output was classified as prompt echo and suppressed. preview=\"{}\"",
                    finalized.chars().take(160).collect::<String>()
                );
                Ok(String::new())
            } else {
                Ok(finalized)
            }
        });

        let result =
            match tokio::time::timeout(std::time::Duration::from_secs(120), task_handle).await {
                Ok(Ok(res)) => res,
                Ok(Err(panic_err)) => Err(InferenceError::execution(
                    format!("Inference panicked: {:?}", panic_err),
                    request_id.to_string(),
                )),
                Err(_) => Err(InferenceError::execution(
                    "Inference timed out (120s)",
                    request_id.to_string(),
                )),
            };

        if should_cleanup_session && self.sessions.remove(&session_id).is_some() {
            info!(
                "🧹 Released ephemeral llama.cpp session {} after multimodal completion().",
                session_id
            );
        }

        result
    }
}

#[async_trait]
impl ModelBackend for LlamaCppBackend {
    fn is_quantized(&self) -> bool {
        // More robust check: GGUF models are almost always quantized in llama.cpp context,
        // or check for specific metadata if the binding exposes it.
        self.model_path
            .to_string_lossy()
            .to_lowercase()
            .contains("q4")
            || self
                .model_path
                .to_string_lossy()
                .to_lowercase()
                .contains("q5")
            || self
                .model_path
                .to_string_lossy()
                .to_lowercase()
                .contains("gguf")
    }

    fn device_info(&self) -> crate::backend::DeviceType {
        let hw = crate::hardware::HardwareStatus::detect();
        if hw.has_gpu {
            crate::backend::DeviceType::Gpu
        } else {
            crate::backend::DeviceType::Cpu
        }
    }

    fn estimated_memory_usage(&self) -> u64 {
        self.estimated_model_bytes
            .saturating_add(self.estimate_kv_bytes_for_tokens(self.runtime_n_ctx as usize))
    }

    async fn generate(
        &self,
        request_id: &str,
        prompt: &str,
        images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        _kv_engine: Arc<parking_lot::RwLock<KvEngine>>,
    ) -> Result<String> {
        if let Some(images) = images.filter(|images| !images.is_empty()) {
            return self
                .run_multimodal_completion(request_id, prompt, images, config)
                .await;
        }

        let session_id = config
            .session_id
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let should_cleanup_session = Self::is_ephemeral_session(&session_id);
        let session_lock = self.get_or_create_session(
            &session_id,
            config.priority,
            LlamaRequestProfile::Standard,
        )?;
        {
            let mut session = session_lock.lock();
            session.priority = config.priority;
        }
        let model = self.model.clone();
        let prompt = prompt.to_string();
        let stop_sequences = Self::effective_stop_sequences(&prompt, &config);
        let runtime_n_ctx = self.runtime_n_ctx;
        let model_train_n_ctx = self.model_train_n_ctx;
        let estimated_kv_bytes_per_token = self.estimated_kv_bytes_per_token;
        let estimated_kv_capacity_bytes =
            self.estimate_kv_bytes_for_tokens(self.runtime_n_ctx as usize);
        let request_id_for_task = request_id.to_string();
        let session_id_for_task = session_id.clone();

        let task_handle = tokio::task::spawn_blocking(move || {
            let mut session = session_lock.lock();
            session.last_used = Instant::now();
            let effective_max_new_tokens =
                Self::effective_max_new_tokens(&session, config.max_new_tokens);

            let tokens = model.str_to_token(&prompt, AddBos::Always).map_err(|e| {
                InferenceError::Execution(
                    format!("Tokenization failed: {}", e),
                    "tokenization".to_string(),
                )
            })?;
            let (tokens, fit_telemetry) =
                Self::fit_tokens_to_context_window(&mut session, tokens, effective_max_new_tokens);

            // Find common prefix to reuse KV cache
            let mut prefix_match = 0;
            for (a, b) in session.tokens.iter().zip(tokens.iter()) {
                if a == b {
                    prefix_match += 1;
                } else {
                    break;
                }
            }

            if prefix_match > 0 {
                session.prefix_reuse_hits = session.prefix_reuse_hits.saturating_add(1);
            } else {
                session.prefix_reuse_misses = session.prefix_reuse_misses.saturating_add(1);
            }
            let prefix_total = session.prefix_reuse_hits + session.prefix_reuse_misses;
            let prefix_hit_rate = if prefix_total > 0 {
                session.prefix_reuse_hits as f32 / prefix_total as f32
            } else {
                0.0
            };

            let _session_reset = Self::reset_session_prefix(&mut session, prefix_match)?;

            let tokens_to_decode = &tokens[prefix_match..];
            let mut last_prefill_batch_len = 0usize;

            info!(
                "🧭 llama.cpp session telemetry: request_id={} session_id={} runtime_n_ctx={} actual_n_ctx={} train_n_ctx={} prompt_tokens={} trimmed_prompt_tokens={} prompt_budget={} reserved_generation={} prefill_tokens={} prefix_match={} prefix_hit_rate={:.2} session_resets={} context_trims={} estimated_kv_live_mb={} estimated_kv_capacity_mb={} context_pressure={:.2} max_new_tokens={}",
                request_id_for_task,
                session_id_for_task,
                runtime_n_ctx,
                fit_telemetry.context_limit,
                model_train_n_ctx,
                fit_telemetry.prompt_tokens,
                fit_telemetry.trimmed_tokens,
                fit_telemetry.prompt_budget,
                fit_telemetry.reserved_generation,
                tokens_to_decode.len(),
                prefix_match,
                prefix_hit_rate,
                session.session_reset_count,
                session.context_trim_count,
                estimated_kv_bytes_per_token.saturating_mul(tokens.len() as u64) / MIB,
                estimated_kv_capacity_bytes / MIB,
                fit_telemetry.context_pressure,
                effective_max_new_tokens,
            );

            if !tokens_to_decode.is_empty() {
                let mut ctx = session.context.0.lock();
                let n_batch = ctx.n_batch() as usize;

                // 🟢 Batch Chunking: Process long prompts in hardware-friendly sizes
                let mut decoded_offset = prefix_match;
                for chunk in tokens_to_decode.chunks(n_batch) {
                    let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(chunk.len(), 1);
                    for (i, &t) in chunk.iter().enumerate() {
                        batch
                            .add(t, (decoded_offset + i) as i32, &[0], i == chunk.len() - 1)
                            .map_err(|e| {
                                InferenceError::execution(
                                    format!("Batch add failed: {}", e),
                                    "batch",
                                )
                            })?;
                    }
                    ctx.decode(&mut batch).map_err(|e| {
                        InferenceError::execution(format!("Decode failed: {}", e), "decode")
                    })?;
                    decoded_offset += chunk.len();
                    last_prefill_batch_len = chunk.len();
                }
            }

            let mut sampler = Self::create_sampler_internal(&config);
            let mut output = String::new();
            let mut decoder = encoding_rs::UTF_8.new_decoder();

            // Store prompt tokens as current state
            session.tokens = tokens.clone();

            let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(1, 1);
            for i in 0..effective_max_new_tokens {
                let idx_in_batch = if i == 0 {
                    (last_prefill_batch_len as i32 - 1).max(0)
                } else {
                    0
                };

                let next_token = {
                    let ctx = session.context.0.lock();
                    sampler.sample(&ctx, idx_in_batch)
                };

                if model.is_eog_token(next_token) {
                    break;
                }

                let piece = model
                    .token_to_piece(next_token, &mut decoder, true, None)
                    .map_err(|e| {
                        InferenceError::execution(format!("Decode error: {}", e), "piece")
                    })?;
                output.push_str(&piece);
                let should_stop = Self::find_earliest_stop(&output, &stop_sequences).is_some();

                batch.clear();
                batch
                    .add(next_token, (session.tokens.len()) as i32, &[0], true)
                    .map_err(|e| {
                        InferenceError::execution(format!("Batch extend failed: {}", e), "extend")
                    })?;

                {
                    let mut ctx = session.context.0.lock();
                    ctx.decode(&mut batch).map_err(|e| {
                        InferenceError::execution(
                            format!("Mid-gen decode failed: {}", e),
                            "mid_gen",
                        )
                    })?;
                }

                // Add generated token to session state
                session.tokens.push(next_token);
                // Inform sampler about the new token
                sampler.accept(next_token);

                if should_stop {
                    break;
                }
            }
            Ok(Self::finalize_generated_output(&output, &stop_sequences))
        });

        use tokio::time::timeout;
        let timeout_dur = std::time::Duration::from_secs(120);

        let result = match timeout(timeout_dur, task_handle).await {
            Ok(Ok(res)) => res, // This is Result<String, InferenceError>
            Ok(Err(panic_err)) => Err(InferenceError::execution(
                format!("Inference panicked: {:?}", panic_err),
                "generate",
            )),
            Err(_) => Err(InferenceError::execution(
                "Inference timed out (120s)",
                "generate",
            )),
        };

        if should_cleanup_session && self.sessions.remove(&session_id).is_some() {
            info!(
                "🧹 Released ephemeral llama.cpp session {} after generate().",
                session_id
            );
        }

        result
    }

    async fn stream_generate(
        &self,
        request_id: &str,
        prompt: &str,
        images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        _kv_engine: Arc<parking_lot::RwLock<KvEngine>>,
        tx: mpsc::Sender<Result<String>>,
    ) -> Result<()> {
        if let Some(images) = images.filter(|images| !images.is_empty()) {
            let result = self
                .run_multimodal_completion(request_id, prompt, images, config)
                .await?;
            if !result.is_empty() {
                let _ = tx.send(Ok(result)).await;
            }
            return Ok(());
        }

        let session_id = config
            .session_id
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let should_cleanup_session = Self::is_ephemeral_session(&session_id);
        let session_lock = self.get_or_create_session(
            &session_id,
            config.priority,
            LlamaRequestProfile::Standard,
        )?;
        {
            let mut session = session_lock.lock();
            session.priority = config.priority;
        }
        let model = self.model.clone();
        let prompt = prompt.to_string();
        let stop_sequences = Self::effective_stop_sequences(&prompt, &config);
        let runtime_n_ctx = self.runtime_n_ctx;
        let model_train_n_ctx = self.model_train_n_ctx;
        let estimated_kv_bytes_per_token = self.estimated_kv_bytes_per_token;
        let estimated_kv_capacity_bytes =
            self.estimate_kv_bytes_for_tokens(self.runtime_n_ctx as usize);
        let request_id = request_id.to_string();
        let request_id_for_task = request_id.clone();
        let session_id_for_task = session_id.clone();

        let task_handle = tokio::task::spawn_blocking(move || {
            let mut session = session_lock.lock();
            session.last_used = Instant::now();
            let effective_max_new_tokens =
                Self::effective_max_new_tokens(&session, config.max_new_tokens);

            let tokens = model.str_to_token(&prompt, AddBos::Always).map_err(|e| {
                InferenceError::Execution(e.to_string(), "tokenization".to_string())
            })?;
            let (tokens, fit_telemetry) =
                Self::fit_tokens_to_context_window(&mut session, tokens, effective_max_new_tokens);

            let mut prefix_match = 0;
            for (a, b) in session.tokens.iter().zip(tokens.iter()) {
                if a == b {
                    prefix_match += 1;
                } else {
                    break;
                }
            }

            if prefix_match > 0 {
                session.prefix_reuse_hits = session.prefix_reuse_hits.saturating_add(1);
            } else {
                session.prefix_reuse_misses = session.prefix_reuse_misses.saturating_add(1);
            }
            let prefix_total = session.prefix_reuse_hits + session.prefix_reuse_misses;
            let prefix_hit_rate = if prefix_total > 0 {
                session.prefix_reuse_hits as f32 / prefix_total as f32
            } else {
                0.0
            };

            let _session_reset = Self::reset_session_prefix(&mut session, prefix_match)?;

            let tokens_to_decode = &tokens[prefix_match..];
            let mut last_prefill_batch_len = 0usize;

            info!(
                "🧭 llama.cpp stream telemetry: request_id={} session_id={} runtime_n_ctx={} actual_n_ctx={} train_n_ctx={} prompt_tokens={} trimmed_prompt_tokens={} prompt_budget={} reserved_generation={} prefill_tokens={} prefix_match={} prefix_hit_rate={:.2} session_resets={} context_trims={} estimated_kv_live_mb={} estimated_kv_capacity_mb={} context_pressure={:.2} max_new_tokens={}",
                request_id_for_task,
                session_id_for_task,
                runtime_n_ctx,
                fit_telemetry.context_limit,
                model_train_n_ctx,
                fit_telemetry.prompt_tokens,
                fit_telemetry.trimmed_tokens,
                fit_telemetry.prompt_budget,
                fit_telemetry.reserved_generation,
                tokens_to_decode.len(),
                prefix_match,
                prefix_hit_rate,
                session.session_reset_count,
                session.context_trim_count,
                estimated_kv_bytes_per_token.saturating_mul(tokens.len() as u64) / MIB,
                estimated_kv_capacity_bytes / MIB,
                fit_telemetry.context_pressure,
                effective_max_new_tokens,
            );

            if !tokens_to_decode.is_empty() {
                let mut ctx = session.context.0.lock();
                let n_batch = ctx.n_batch() as usize;

                let mut decoded_offset = prefix_match;
                for chunk in tokens_to_decode.chunks(n_batch) {
                    let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(chunk.len(), 1);
                    for (i, &t) in chunk.iter().enumerate() {
                        batch
                            .add(t, (decoded_offset + i) as i32, &[0], i == chunk.len() - 1)
                            .map_err(|e| {
                                InferenceError::execution(
                                    format!("Batch add failed: {}", e),
                                    "batch",
                                )
                            })?;
                    }
                    ctx.decode(&mut batch).map_err(|e| {
                        InferenceError::execution(format!("Decode failed: {}", e), "decode")
                    })?;
                    decoded_offset += chunk.len();
                    last_prefill_batch_len = chunk.len();
                }
            }

            let mut sampler = Self::create_sampler_internal(&config);
            let mut decoder = encoding_rs::UTF_8.new_decoder();
            let mut pending = String::new();

            session.tokens = tokens.clone();

            let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(1, 1);
            for i in 0..effective_max_new_tokens {
                let idx_in_batch = if i == 0 {
                    (last_prefill_batch_len as i32 - 1).max(0)
                } else {
                    0
                };

                let next_token = {
                    let ctx = session.context.0.lock();
                    sampler.sample(&ctx, idx_in_batch)
                };
                if model.is_eog_token(next_token) {
                    break;
                }

                let piece = model
                    .token_to_piece(next_token, &mut decoder, true, None)
                    .map_err(|e| {
                        InferenceError::execution(format!("Decode piece failed: {}", e), "piece")
                    })?;
                pending.push_str(&piece);

                batch.clear();
                batch
                    .add(next_token, (session.tokens.len()) as i32, &[0], true)
                    .map_err(|e| {
                        InferenceError::execution(format!("Batch extend failed: {}", e), "extend")
                    })?;

                {
                    let mut ctx = session.context.0.lock();
                    ctx.decode(&mut batch).map_err(|e| {
                        InferenceError::execution(
                            format!("Mid-gen decode failed: {}", e),
                            "mid_gen",
                        )
                    })?;
                }

                session.tokens.push(next_token);
                sampler.accept(next_token);

                if let Some(stop_idx) = Self::find_earliest_stop(&pending, &stop_sequences) {
                    let final_chunk = Self::cleanup_output_suffix(&pending[..stop_idx]);
                    if !final_chunk.is_empty() && tx.blocking_send(Ok(final_chunk)).is_err() {
                        break;
                    }
                    pending.clear();
                    break;
                }

                let overlap_len = Self::trailing_stop_overlap_len(&pending, &stop_sequences);
                let flush_up_to = pending.len().saturating_sub(overlap_len);
                if flush_up_to > 0 {
                    let flush_chunk = pending[..flush_up_to].to_string();
                    pending.drain(..flush_up_to);
                    if tx.blocking_send(Ok(flush_chunk)).is_err() {
                        break;
                    }
                }
            }

            let final_chunk = Self::finalize_generated_output(&pending, &stop_sequences);
            if !final_chunk.is_empty() {
                let _ = tx.blocking_send(Ok(final_chunk));
            }
            Ok(())
        });

        use tokio::time::timeout;
        let timeout_dur = std::time::Duration::from_secs(120);

        let result = match timeout(timeout_dur, task_handle).await {
            Ok(Ok(res)) => res, // This is Result<(), InferenceError>
            Ok(Err(panic_err)) => Err(InferenceError::execution(
                format!("Inference panicked: {:?}", panic_err),
                request_id.to_string(),
            )),
            Err(_) => Err(InferenceError::execution(
                "Inference timed out (120s)",
                request_id.to_string(),
            )),
        };

        if should_cleanup_session && self.sessions.remove(&session_id).is_some() {
            info!(
                "🧹 Released ephemeral llama.cpp session {} after stream_generate().",
                session_id
            );
        }

        result
    }

    fn model_info(&self) -> String {
        format!(
            "Native-LlamaCpp: {} (Sessions: {})",
            self.model_path.display(),
            self.sessions.len()
        )
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[async_trait]
impl crate::backend::VisionModelBackend for LlamaCppBackend {
    async fn vision_analyze(
        &self,
        image: &image::DynamicImage,
        task: crate::backend::VisionTask,
        prompt: Option<&str>,
        config: Option<crate::backend::GenerationConfig>,
    ) -> Result<String> {
        let user_prompt = match task {
            crate::backend::VisionTask::Describe => {
                prompt.unwrap_or("请简要描述这张图片里最主要的视觉内容。")
            }
            crate::backend::VisionTask::OCR => prompt.unwrap_or("请提取这张图片中的可见文字。"),
            crate::backend::VisionTask::Grounding => {
                prompt.unwrap_or("请识别这张图片里的关键对象与位置线索。")
            }
        };
        let task_prompt = self.build_multimodal_chat_prompt(task, user_prompt, 1);
        let effective_config = Self::vision_generation_config(task, config);

        if self.supports_multimodal_vision() {
            return self
                .generate(
                    &uuid::Uuid::new_v4().to_string(),
                    &task_prompt,
                    Some(vec![image.clone()]),
                    effective_config,
                    Arc::new(parking_lot::RwLock::new(KvEngine::new(Default::default()))),
                )
                .await;
        }

        Err(InferenceError::execution(
            "llama.cpp vision runtime is not ready; mmproj is missing or mtmd vision support is unavailable on the current host",
            "vision",
        ))
    }

    async fn vision_analyze_video(
        &self,
        frames: &[image::DynamicImage],
        prompt: Option<&str>,
        config: Option<crate::backend::GenerationConfig>,
    ) -> Result<String> {
        if frames.is_empty() {
            return Err(InferenceError::InvalidInput(
                "video multimodal analysis requires at least one decoded frame".to_string(),
            ));
        }

        if self.supports_multimodal_vision() {
            let task_prompt = self.build_multimodal_chat_prompt(
                crate::backend::VisionTask::Describe,
                prompt.unwrap_or("请概括这些视频帧呈现的主要内容。"),
                frames.len(),
            );
            let effective_config =
                Self::vision_generation_config(crate::backend::VisionTask::Describe, config);
            return self
                .generate(
                    &uuid::Uuid::new_v4().to_string(),
                    &task_prompt,
                    Some(frames.to_vec()),
                    effective_config,
                    Arc::new(parking_lot::RwLock::new(KvEngine::new(Default::default()))),
                )
                .await;
        }

        Err(InferenceError::execution(
            "llama.cpp video vision runtime is not ready; mmproj is missing or mtmd vision support is unavailable on the current host",
            "vision_video",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        llama_cpp_gpu_layers, llama_cpp_gpu_layers_for_budget,
        llama_cpp_large_multimodal_layer_cap, LlamaCppBackend,
    };
    use crate::hardware::{
        GpuProbeConfidence, GpuProbeSource, GpuVendor, HardwareStatus, MemoryTopology,
    };
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn llama_cpp_gpu_layers_accepts_cuda_without_vulkan() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("NVIDIA RTX 4090".into()),
            gpu_vendor: Some(GpuVendor::Nvidia),
            gpu_probe_confidence: GpuProbeConfidence::Tooling,
            gpu_probe_source: Some(GpuProbeSource::NvidiaSmi),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: None,
            vram_used_mb: 0,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
            vulkan_supported: false,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: true,
            rocm_available: false,
            gpu_compute_capability: Some((8, 9)),
        };

        assert_eq!(llama_cpp_gpu_layers(&status), 100);
        assert_eq!(
            llama_cpp_gpu_layers_for_budget(&status, 8 * 1024 * 1024 * 1024, 0),
            100
        );
    }

    #[test]
    fn llama_cpp_gpu_layers_accepts_rocm_for_amd_without_vulkan() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("AMD Radeon RX 7900 XTX".into()),
            gpu_vendor: Some(GpuVendor::Amd),
            gpu_probe_confidence: GpuProbeConfidence::Tooling,
            gpu_probe_source: Some(GpuProbeSource::RocmInfo),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: None,
            vram_used_mb: 0,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
            vulkan_supported: false,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: false,
            rocm_available: true,
            gpu_compute_capability: None,
        };

        assert_eq!(llama_cpp_gpu_layers(&status), 100);
        assert_eq!(
            llama_cpp_gpu_layers_for_budget(&status, 8 * 1024 * 1024 * 1024, 0),
            100
        );
    }

    #[test]
    fn llama_cpp_gpu_layers_scales_down_when_vram_budget_is_tight() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("AMD Radeon RX 7900 XTX".into()),
            gpu_vendor: Some(GpuVendor::Amd),
            gpu_probe_confidence: GpuProbeConfidence::Tooling,
            gpu_probe_source: Some(GpuProbeSource::RocmInfo),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: Some(24 * 1024),
            vram_used_mb: 10 * 1024,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
            vulkan_supported: false,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: false,
            rocm_available: true,
            gpu_compute_capability: None,
        };

        let gpu_layers =
            llama_cpp_gpu_layers_for_budget(&status, 14 * 1024 * 1024 * 1024, 1024 * 1024 * 1024);
        assert!(gpu_layers > 0);
        assert!(gpu_layers < 100);
    }

    #[test]
    fn llama_cpp_gpu_layers_disables_offload_when_budget_is_exhausted() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("AMD Radeon RX 7900 XTX".into()),
            gpu_vendor: Some(GpuVendor::Amd),
            gpu_probe_confidence: GpuProbeConfidence::Tooling,
            gpu_probe_source: Some(GpuProbeSource::RocmInfo),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: Some(24 * 1024),
            vram_used_mb: 20 * 1024,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
            vulkan_supported: false,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: false,
            rocm_available: true,
            gpu_compute_capability: None,
        };

        assert_eq!(
            llama_cpp_gpu_layers_for_budget(&status, 8 * 1024 * 1024 * 1024, 0),
            0
        );
    }

    #[test]
    fn llama_cpp_gpu_layers_prefers_small_partial_offload_for_shared_gpu() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("Intel Arc integrated".into()),
            gpu_vendor: Some(GpuVendor::Intel),
            gpu_probe_confidence: GpuProbeConfidence::Heuristic,
            gpu_probe_source: None,
            memory_topology: MemoryTopology::SharedGpu,
            vram_total_mb: 8 * 1024,
            vram_budget_mb: Some(8 * 1024),
            vram_used_mb: 0,
            shared_memory_total_mb: Some(16 * 1024),
            shared_memory_budget_mb: Some(8 * 1024),
            vulkan_supported: true,
            cpu_cores: 8,
            ram_total_mb: 32 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: false,
            rocm_available: false,
            gpu_compute_capability: None,
        };

        let gpu_layers = llama_cpp_gpu_layers_for_budget(&status, 6 * 1024 * 1024 * 1024, 0);
        assert!(matches!(gpu_layers, 8 | 16 | 24));
    }

    #[test]
    fn llama_cpp_large_multimodal_layer_cap_limits_24g_dedicated_gpu() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("AMD Radeon RX 7900 XTX".into()),
            gpu_vendor: Some(GpuVendor::Amd),
            gpu_probe_confidence: GpuProbeConfidence::Tooling,
            gpu_probe_source: Some(GpuProbeSource::RocmInfo),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: None,
            vram_used_mb: 0,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
            vulkan_supported: false,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: false,
            rocm_available: true,
            gpu_compute_capability: None,
        };

        assert_eq!(
            llama_cpp_large_multimodal_layer_cap(
                &status,
                13 * 1024 * 1024 * 1024,
                768 * 1024 * 1024
            ),
            Some(24)
        );
    }

    #[test]
    fn runtime_context_budget_scales_beyond_library_default_for_capable_gpu() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("AMD Radeon RX 7900 XTX".into()),
            gpu_vendor: Some(GpuVendor::Amd),
            gpu_probe_confidence: GpuProbeConfidence::Tooling,
            gpu_probe_source: Some(GpuProbeSource::RocmInfo),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: Some(24 * 1024),
            vram_used_mb: 0,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
            vulkan_supported: false,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: false,
            rocm_available: true,
            gpu_compute_capability: None,
        };

        let runtime_n_ctx = LlamaCppBackend::select_runtime_n_ctx(
            131_072,
            5 * 1024 * 1024 * 1024,
            56 * 1024,
            &status,
        );

        assert!(runtime_n_ctx > 512);
        assert_eq!(runtime_n_ctx % 512, 0);
        assert_eq!(runtime_n_ctx, 32_768);
    }

    #[test]
    fn effective_max_new_tokens_is_no_longer_clamped_to_tiny_interactive_window() {
        let effective = LlamaCppBackend::effective_max_new_tokens_for_context(8_192, 1_024);
        assert_eq!(effective, 1_024);

        let clipped = LlamaCppBackend::effective_max_new_tokens_for_context(512, 4_096);
        assert_eq!(clipped, 256);
    }

    #[test]
    fn hardware_context_cap_tracks_gpu_tier() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("Midrange GPU".into()),
            gpu_vendor: Some(GpuVendor::Amd),
            gpu_probe_confidence: GpuProbeConfidence::Tooling,
            gpu_probe_source: Some(GpuProbeSource::RocmInfo),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 12 * 1024,
            vram_budget_mb: Some(12 * 1024),
            vram_used_mb: 0,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
            vulkan_supported: false,
            cpu_cores: 12,
            ram_total_mb: 32 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: false,
            rocm_available: true,
            gpu_compute_capability: None,
        };

        assert_eq!(LlamaCppBackend::hardware_context_cap(&status), 16_384);
        assert_eq!(LlamaCppBackend::minimum_runtime_n_ctx(&status), 4_096);
    }

    #[test]
    fn discover_mmproj_path_prefers_explicit_and_falls_back_to_sibling_scan() {
        let dir = tempdir().expect("tempdir");
        let model_path = dir.path().join("my-model.gguf");
        let sibling_mmproj = dir.path().join("mmproj-my-model.gguf");
        fs::write(&model_path, b"model").expect("write model");
        fs::write(&sibling_mmproj, b"mmproj").expect("write mmproj");

        let discovered = LlamaCppBackend::discover_mmproj_path(&model_path, None)
            .expect("discover sibling mmproj");
        assert_eq!(discovered, sibling_mmproj);

        let explicit = dir.path().join("explicit-mmproj.gguf");
        fs::write(&explicit, b"explicit").expect("write explicit mmproj");
        let explicit_discovered =
            LlamaCppBackend::discover_mmproj_path(&model_path, Some(&explicit))
                .expect("discover explicit mmproj");
        assert_eq!(explicit_discovered, explicit);
    }

    #[test]
    fn multimodal_child_sessions_are_not_treated_as_ephemeral_when_root_session_is_stable() {
        assert!(!LlamaCppBackend::is_ephemeral_session(
            "session-123::vision"
        ));
        assert!(!LlamaCppBackend::is_ephemeral_session("conversation-abc"));
        assert!(LlamaCppBackend::is_ephemeral_session(
            "native-ephemeral-vision-request-1"
        ));
        assert!(LlamaCppBackend::is_ephemeral_session("ephemeral-42"));
    }
}
