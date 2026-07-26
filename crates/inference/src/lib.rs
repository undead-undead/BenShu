//! Inference Engine modules for BenShu.

pub mod backend;
pub mod engine;
pub mod hardware;
pub mod kernels;
pub mod memory;
pub mod model_contract;
pub mod quant;
pub mod runtime;
pub mod windows_native;

pub use backend::candle::CandleBackend;
#[cfg(feature = "llama_cpp")]
pub use backend::llama_cpp::LlamaCppBackend;
pub use backend::vlm_candle::CandleVlmBackend;
pub use backend::{
    GenerationConfig, InferenceError, ModelBackend, Result, VisionModelBackend, VisionTask,
};
pub use memory::InferenceArena;
pub use model_contract::{
    describe_local_model_contract, LocalModelArtifactKind, LocalModelContractDescriptor,
};

// Re-exports for convenience
pub use engine::{CachePage, EngineError, EngineLoad, InferenceConfig, KvEngine};
pub use hardware::{
    AccelerationProfile, GpuProbeConfidence, GpuProbeSource, GpuVendor, HardwareBudgets,
    HardwareStatus, HardwareTelemetry, MemoryTopology,
};
pub use kernels::*;
pub use quant::{QuantLevel, Quantizer, ScalarQuantizer, TernaryQuantizer};
pub use runtime::{
    build_effective_diagnostics, build_effective_diagnostics_with_runtime,
    estimate_llama_kv_cache_budget_mb, recommend_llama_cpp_runtime,
    resolve_llama_cpp_reasoning_compatibility, should_apply_llama_cpp_auto_recommendation,
    summarize_hardware, LlamaCppEffectiveDiagnostics, LlamaCppReasoningCompatibility,
    LlamaCppRuntimeInput, LlamaCppRuntimePreference, LlamaCppRuntimeRecommendation,
    ModelRuntimeBinding, ModelRuntimeKind, ModelRuntimeManager, ModelRuntimeState,
    ModelRuntimeStatus, RuntimeHardwareSummary, RuntimeMemoryPlan, LLAMA_TUNING_AUTO,
    LLAMA_TUNING_MANUAL, PROFILE_BALANCED, PROFILE_LOW_VRAM, PROFILE_SPEED,
};
pub use windows_native::{
    detect_windows_native_runtime_status, diagnose_windows_native_small_model_error,
    windows_native_small_model_execution_linked, WindowsNativeRuntimeDiagnosis,
    WindowsNativeRuntimeStatus,
};
