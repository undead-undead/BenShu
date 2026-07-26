mod artifact_policy;
pub mod vault;

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default, alias = "benshu_providers")]
    pub providers: ProviderConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default, alias = "agent")]
    #[cfg(not(target_arch = "wasm32"))]
    pub agent_identity: Option<crate::agent::agent_identity::AgentIdentity>,
    #[serde(default)]
    pub connectors: ConnectorsConfig,
    #[serde(default)]
    pub knowledge: KnowledgeConfig,
    #[serde(default)]
    pub sensory: SensoryConfig,
    #[serde(default)]
    pub llama_cpp_runtime: LlamaCppRuntimeConfig,
    #[serde(default)]
    pub windows_ml_runtime: WindowsMlRuntimeConfig,
    #[serde(default)]
    pub windows_ml_bridge: WindowsMlBridgeConfig,
    #[serde(default)]
    pub runtime_host_control: RuntimeHostControlConfig,
    #[serde(default)]
    pub continuation_runtime: ContinuationRuntimeConfig,
    /// Path to a folder containing .md files for agent_identity "agent" injection
    pub agent_path: Option<PathBuf>,
    /// Per-agent specific overrides (e.g., model, provider)
    #[serde(default)]
    pub agents: std::collections::HashMap<String, AgentConfigOverrides>,
    /// Global list of trusted workspace paths for file operations
    #[serde(default)]
    pub trusted_workspaces: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlamaCppRuntimeConfig {
    #[serde(default = "default_llama_tuning_mode")]
    pub tuning_mode: String,
    #[serde(default = "default_llama_performance_profile")]
    pub performance_profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_recommendation: Option<benshu_inference::runtime::LlamaCppRuntimeRecommendation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_diagnostics: Option<benshu_inference::runtime::LlamaCppEffectiveDiagnostics>,
    #[serde(default = "default_llama_ctx_size")]
    pub ctx_size: u32,
    #[serde(default = "default_llama_gpu_layers")]
    pub gpu_layers: u32,
    #[serde(default = "default_llama_threads")]
    pub threads: i32,
    #[serde(default)]
    pub threads_batch: Option<i32>,
    #[serde(default = "default_llama_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_llama_ubatch_size")]
    pub ubatch_size: u32,
    #[serde(default = "default_llama_parallel_slots")]
    pub parallel_slots: u32,
    #[serde(default = "default_llama_cache_ram")]
    pub cache_ram: Option<u32>,
    #[serde(default = "default_llama_ctx_checkpoints")]
    pub ctx_checkpoints: Option<u32>,
    #[serde(default = "default_llama_flash_attn_mode")]
    pub flash_attn_mode: String,
    #[serde(default = "default_llama_kv_offload")]
    pub kv_offload: bool,
    #[serde(default = "default_llama_mmap")]
    pub mmap: bool,
    #[serde(default)]
    pub mlock: bool,
    #[serde(default = "default_llama_cache_prompt")]
    pub cache_prompt: bool,
    #[serde(default = "default_llama_cont_batching")]
    pub cont_batching: bool,
    #[serde(default = "default_llama_warmup")]
    pub warmup: bool,
    #[serde(default = "default_llama_context_shift")]
    pub context_shift: bool,
    #[serde(default = "default_llama_jinja")]
    pub jinja: bool,
    #[serde(default)]
    pub rope_scaling: Option<String>,
    #[serde(default)]
    pub rope_scale: Option<f32>,
    #[serde(default)]
    pub rope_freq_base: Option<f32>,
    #[serde(default)]
    pub rope_freq_scale: Option<f32>,
    #[serde(default)]
    pub yarn_orig_ctx: Option<u32>,
    #[serde(default)]
    pub yarn_ext_factor: Option<f32>,
    #[serde(default)]
    pub yarn_attn_factor: Option<f32>,
    #[serde(default)]
    pub yarn_beta_slow: Option<f32>,
    #[serde(default)]
    pub yarn_beta_fast: Option<f32>,
    #[serde(default)]
    pub cache_type_k: Option<String>,
    #[serde(default)]
    pub cache_type_v: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub split_mode: Option<String>,
    #[serde(default)]
    pub tensor_split: Option<String>,
    #[serde(default)]
    pub main_gpu: Option<u32>,
    #[serde(default = "default_llama_fit_mode")]
    pub fit_mode: String,
    #[serde(default)]
    pub fit_target: Option<String>,
    #[serde(default)]
    pub fit_ctx: Option<u32>,
    #[serde(default)]
    pub cpu_moe: bool,
    #[serde(default)]
    pub n_cpu_moe: Option<u32>,
    #[serde(default = "default_llama_mmproj_offload")]
    pub mmproj_offload: bool,
    #[serde(default)]
    pub image_min_tokens: Option<u32>,
    #[serde(default)]
    pub image_max_tokens: Option<u32>,
    #[serde(default = "default_llama_reasoning_mode")]
    pub reasoning_mode: String,
    #[serde(default = "default_llama_reasoning_format")]
    pub reasoning_format: String,
    #[serde(default)]
    pub reasoning_budget: Option<i32>,
    #[serde(default)]
    pub reasoning_budget_message: Option<String>,
    #[serde(default = "default_llama_sampling_temperature")]
    pub sampling_temperature: f32,
    #[serde(default = "default_llama_sampling_top_k")]
    pub sampling_top_k: i32,
    #[serde(default = "default_llama_sampling_top_p")]
    pub sampling_top_p: f32,
    #[serde(default = "default_llama_sampling_min_p")]
    pub sampling_min_p: f32,
    #[serde(default = "default_llama_sampling_typical_p")]
    pub sampling_typical_p: f32,
    #[serde(default = "default_llama_sampling_repeat_penalty")]
    pub sampling_repeat_penalty: f32,
    #[serde(default = "default_llama_sampling_presence_penalty")]
    pub sampling_presence_penalty: f32,
    #[serde(default = "default_llama_sampling_frequency_penalty")]
    pub sampling_frequency_penalty: f32,
    #[serde(default = "default_llama_sampling_mirostat")]
    pub sampling_mirostat: i32,
    #[serde(default = "default_llama_sampling_mirostat_eta")]
    pub sampling_mirostat_eta: f32,
    #[serde(default = "default_llama_sampling_mirostat_tau")]
    pub sampling_mirostat_tau: f32,
    #[serde(default)]
    pub seed: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContinuationRuntimeConfig {
    #[serde(default)]
    pub disk_cache_enabled: bool,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default = "default_continuation_cache_budget_mb")]
    pub cache_budget_mb: u64,
    #[serde(default = "default_continuation_cache_max_entries")]
    pub cache_max_entries: u32,
    #[serde(default = "default_continuation_cache_sensitive_tasks_disabled")]
    pub disable_disk_cache_for_sensitive_tasks: bool,
}

impl Default for ContinuationRuntimeConfig {
    fn default() -> Self {
        Self {
            disk_cache_enabled: false,
            cache_dir: None,
            cache_budget_mb: default_continuation_cache_budget_mb(),
            cache_max_entries: default_continuation_cache_max_entries(),
            disable_disk_cache_for_sensitive_tasks:
                default_continuation_cache_sensitive_tasks_disabled(),
        }
    }
}

impl Default for LlamaCppRuntimeConfig {
    fn default() -> Self {
        Self {
            tuning_mode: default_llama_tuning_mode(),
            performance_profile: default_llama_performance_profile(),
            last_recommendation: None,
            effective_diagnostics: None,
            ctx_size: default_llama_ctx_size(),
            gpu_layers: default_llama_gpu_layers(),
            threads: default_llama_threads(),
            threads_batch: None,
            batch_size: default_llama_batch_size(),
            ubatch_size: default_llama_ubatch_size(),
            parallel_slots: default_llama_parallel_slots(),
            cache_ram: default_llama_cache_ram(),
            ctx_checkpoints: default_llama_ctx_checkpoints(),
            flash_attn_mode: default_llama_flash_attn_mode(),
            kv_offload: default_llama_kv_offload(),
            mmap: default_llama_mmap(),
            mlock: false,
            cache_prompt: default_llama_cache_prompt(),
            cont_batching: default_llama_cont_batching(),
            warmup: default_llama_warmup(),
            context_shift: default_llama_context_shift(),
            jinja: default_llama_jinja(),
            rope_scaling: None,
            rope_scale: None,
            rope_freq_base: None,
            rope_freq_scale: None,
            yarn_orig_ctx: None,
            yarn_ext_factor: None,
            yarn_attn_factor: None,
            yarn_beta_slow: None,
            yarn_beta_fast: None,
            cache_type_k: None,
            cache_type_v: None,
            device: None,
            split_mode: None,
            tensor_split: None,
            main_gpu: None,
            fit_mode: default_llama_fit_mode(),
            fit_target: None,
            fit_ctx: None,
            cpu_moe: false,
            n_cpu_moe: None,
            mmproj_offload: default_llama_mmproj_offload(),
            image_min_tokens: None,
            image_max_tokens: None,
            reasoning_mode: default_llama_reasoning_mode(),
            reasoning_format: default_llama_reasoning_format(),
            reasoning_budget: None,
            reasoning_budget_message: None,
            sampling_temperature: default_llama_sampling_temperature(),
            sampling_top_k: default_llama_sampling_top_k(),
            sampling_top_p: default_llama_sampling_top_p(),
            sampling_min_p: default_llama_sampling_min_p(),
            sampling_typical_p: default_llama_sampling_typical_p(),
            sampling_repeat_penalty: default_llama_sampling_repeat_penalty(),
            sampling_presence_penalty: default_llama_sampling_presence_penalty(),
            sampling_frequency_penalty: default_llama_sampling_frequency_penalty(),
            sampling_mirostat: default_llama_sampling_mirostat(),
            sampling_mirostat_eta: default_llama_sampling_mirostat_eta(),
            sampling_mirostat_tau: default_llama_sampling_mirostat_tau(),
            seed: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowsMlRuntimeConfig {
    #[serde(default = "default_windows_ml_runtime_family")]
    pub runtime_family: String,
    #[serde(default = "default_windows_ml_execution_provider")]
    pub execution_provider_preference: String,
    #[serde(default = "default_windows_ml_device_target")]
    pub device_target: String,
    #[serde(default = "default_windows_ml_cpu_fallback_policy")]
    pub cpu_fallback_policy: String,
    #[serde(default = "default_windows_ml_graph_optimization")]
    pub graph_optimization_level: String,
    #[serde(default)]
    pub intra_threads: Option<u32>,
    #[serde(default)]
    pub inter_threads: Option<u32>,
    #[serde(default)]
    pub text_profile: WindowsMlTextProfile,
    #[serde(default)]
    pub vision_profile: WindowsMlVisionProfile,
    #[serde(default)]
    pub audio_profile: WindowsMlAudioProfile,
    #[serde(default)]
    pub image_profile: WindowsMlImageProfile,
    #[serde(default)]
    pub realtime_profile: WindowsMlRealtimeProfile,
    #[serde(default)]
    pub safety_profile: WindowsMlSafetyProfile,
}

impl Default for WindowsMlRuntimeConfig {
    fn default() -> Self {
        Self {
            runtime_family: default_windows_ml_runtime_family(),
            execution_provider_preference: default_windows_ml_execution_provider(),
            device_target: default_windows_ml_device_target(),
            cpu_fallback_policy: default_windows_ml_cpu_fallback_policy(),
            graph_optimization_level: default_windows_ml_graph_optimization(),
            intra_threads: None,
            inter_threads: None,
            text_profile: WindowsMlTextProfile::default(),
            vision_profile: WindowsMlVisionProfile::default(),
            audio_profile: WindowsMlAudioProfile::default(),
            image_profile: WindowsMlImageProfile::default(),
            realtime_profile: WindowsMlRealtimeProfile::default(),
            safety_profile: WindowsMlSafetyProfile::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowsMlTextProfile {
    #[serde(default = "default_windows_ml_text_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_windows_ml_text_max_sequence_length")]
    pub max_sequence_length: u32,
}

impl Default for WindowsMlTextProfile {
    fn default() -> Self {
        Self {
            batch_size: default_windows_ml_text_batch_size(),
            max_sequence_length: default_windows_ml_text_max_sequence_length(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowsMlVisionProfile {
    #[serde(default = "default_windows_ml_vision_max_image_side")]
    pub max_image_side: u32,
    #[serde(default = "default_windows_ml_vision_resize_policy")]
    pub resize_policy: String,
}

impl Default for WindowsMlVisionProfile {
    fn default() -> Self {
        Self {
            max_image_side: default_windows_ml_vision_max_image_side(),
            resize_policy: default_windows_ml_vision_resize_policy(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowsMlAudioProfile {
    #[serde(default = "default_windows_ml_audio_sample_rate")]
    pub sample_rate_hz: u32,
    #[serde(default = "default_windows_ml_audio_chunk_ms")]
    pub chunk_ms: u32,
}

impl Default for WindowsMlAudioProfile {
    fn default() -> Self {
        Self {
            sample_rate_hz: default_windows_ml_audio_sample_rate(),
            chunk_ms: default_windows_ml_audio_chunk_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowsMlImageProfile {
    #[serde(default = "default_windows_ml_image_width")]
    pub width: u32,
    #[serde(default = "default_windows_ml_image_height")]
    pub height: u32,
    #[serde(default = "default_windows_ml_image_steps")]
    pub steps: u32,
    #[serde(default = "default_windows_ml_image_guidance")]
    pub guidance: f32,
}

impl Default for WindowsMlImageProfile {
    fn default() -> Self {
        Self {
            width: default_windows_ml_image_width(),
            height: default_windows_ml_image_height(),
            steps: default_windows_ml_image_steps(),
            guidance: default_windows_ml_image_guidance(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowsMlRealtimeProfile {
    #[serde(default = "default_windows_ml_realtime_vad_window_ms")]
    pub vad_window_ms: u32,
    #[serde(default = "default_windows_ml_realtime_duplex_frame_ms")]
    pub duplex_frame_ms: u32,
}

impl Default for WindowsMlRealtimeProfile {
    fn default() -> Self {
        Self {
            vad_window_ms: default_windows_ml_realtime_vad_window_ms(),
            duplex_frame_ms: default_windows_ml_realtime_duplex_frame_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowsMlSafetyProfile {
    #[serde(default = "default_windows_ml_safety_threshold")]
    pub threshold: f32,
}

impl Default for WindowsMlSafetyProfile {
    fn default() -> Self {
        Self {
            threshold: default_windows_ml_safety_threshold(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeHostControlConfig {
    #[serde(default)]
    pub main_brain: ManagedRuntimeHostConfig,
    #[serde(default)]
    pub windows_ml: ManagedRuntimeHostConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedRuntimeHostConfig {
    #[serde(default = "default_runtime_host_control_mode")]
    pub control_mode: String,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub restart_command: Vec<String>,
    #[serde(default = "default_runtime_host_control_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for ManagedRuntimeHostConfig {
    fn default() -> Self {
        Self {
            control_mode: default_runtime_host_control_mode(),
            service_name: None,
            restart_command: Vec::new(),
            timeout_secs: default_runtime_host_control_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowsMlBridgeConfig {
    #[serde(default)]
    pub image_bridge_base_url: Option<String>,
    #[serde(default)]
    pub bindings: std::collections::BTreeMap<String, WindowsMlBridgeBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowsMlBridgeBinding {
    pub role: String,
    pub source_model: String,
    pub effective_model: String,
    pub artifact_kind: String,
    pub runtime_target: String,
    pub execution_provider: String,
    pub bridge_mode: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfigOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_model_artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_mmproj_artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_runtime_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_policy: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_consolidation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traits: Option<crate::agent::agent_identity::Traits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backstory: Option<String>,
}

impl AgentConfigOverrides {
    pub fn runtime_only(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            local_model_artifact: self.local_model_artifact.clone(),
            local_mmproj_artifact: self.local_mmproj_artifact.clone(),
            local_runtime_family: self.local_runtime_family.clone(),
            temperature: None,
            tools: None,
            artifact_policy: None,
            auto_consolidation: None,
            traits: None,
            name: None,
            description: None,
            tone: None,
            constraints: None,
            backstory: None,
        }
    }

    pub fn is_runtime_empty(&self) -> bool {
        self.provider.is_none()
            && self.base_url.is_none()
            && self.model.is_none()
            && self.local_model_artifact.is_none()
            && self.local_mmproj_artifact.is_none()
            && self.local_runtime_family.is_none()
    }

    pub fn parse_frontmatter(content: &str) -> (Self, String) {
        if content.starts_with("---\n") || content.starts_with("---\r\n") {
            if let Some(end_idx) = content[4..].find("\n---") {
                let end_full = end_idx + 4;
                let yaml_str = &content[4..end_full];
                if let Ok(config) = serde_yaml_ng::from_str::<Self>(yaml_str) {
                    let mut rest = content[end_full + 4..].to_string();
                    if rest.starts_with('\n') {
                        rest.remove(0);
                    }
                    return (config, rest);
                }
            }
        }
        (Self::default(), content.to_string())
    }

    pub fn to_yaml(&self) -> String {
        serde_yaml_ng::to_string(self).unwrap_or_default()
    }

    pub fn artifact_policy_yaml(&self) -> String {
        self.artifact_policy
            .as_ref()
            .map(artifact_policy::artifact_policy_to_yaml)
            .unwrap_or_default()
    }

    pub fn parse_artifact_policy_yaml(
        yaml: &str,
    ) -> std::result::Result<Option<serde_json::Value>, String> {
        let trimmed = yaml.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        artifact_policy::parse_artifact_policy_yaml(trimmed)
    }

    pub fn to_markdown(&self, body: &str) -> String {
        let yaml = self.to_yaml();
        if yaml.is_empty() || yaml == "{}\n" {
            body.to_string()
        } else {
            format!("---\n{}---\n\n{}", yaml, body.trim_start())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectorsConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub feishu: Option<FeishuConfig>,
    pub dingtalk: Option<DingTalkConfig>,
    pub slack: Option<SlackConfig>,
    pub email: Option<EmailConfig>,
    pub qq: Option<QQConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QQConfig {
    pub app_id: String,
    pub app_secret: String,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmailConfig {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_pass: String,
    pub imap_server: String,
    pub imap_port: u16,
    pub imap_user: String,
    pub imap_pass: String,
    pub from_address: String,
    pub broadcast_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub allowed_chat_ids: Vec<String>, // Whitelist for security
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscordConfig {
    pub bot_token: String,
    pub channel_ids: Vec<String>,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    pub verification_token: String,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DingTalkConfig {
    pub app_key: String,
    pub app_secret: String,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: Option<String>, // For Socket Mode
    pub verification_token: String,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SensoryConfig {
    #[serde(default)]
    pub enable_global_voice: bool,
    #[serde(default)]
    pub enable_local_vision: bool,
    #[serde(default)]
    pub fact_check_enabled: bool,
    pub voice_provider: Option<String>,   // "openai", "local"
    pub stt_model: Option<String>,        // whisper-tiny, whisper-base
    pub tts_model: Option<String>,        // piper-en_US-lessac-medium
    pub vision_model: Option<String>,     // "llava-v1.5-7b", "moondream"
    pub ocr_model: Option<String>,        // "tesseract", local OCR package
    pub image_gen_model: Option<String>, // e.g. "api:openai/gpt-image-1" or "bridge-image:http://host:port/v1|model"
    pub image_edit_model: Option<String>, // local edit / inpaint / multimodal edit package
    pub tactical_model: Option<String>,  // local or cloud tactical model binding
    pub fact_check_model: Option<String>, // local or API validation model
    pub audio_understanding_model: Option<String>, // local audio comprehension package
    pub realtime_vad_model: Option<String>, // local VAD package
    pub duplex_voice_model: Option<String>, // local duplex/realtime voice package
    pub local_classifier_model: Option<String>, // local classifier package
    pub local_router_model: Option<String>, // local routing / selector package
    pub local_safety_model: Option<String>, // local safety / moderation package
    pub vram_budget_mb: Option<u64>,
    pub video_buffer_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsConfig {
    #[serde(default)]
    pub enabled: std::collections::HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeConfig {
    pub enable_vector: bool,
    #[serde(default = "default_ram_limit")]
    pub model_ram_limit_gb: u32,
    #[serde(default = "default_vram_limit")]
    pub model_vram_limit_gb: u32,
    #[serde(default = "default_auto_consolidation")]
    pub auto_consolidation_enabled: bool,
    #[serde(default = "default_embed_model")]
    pub embed_model: String,
    #[serde(default = "default_rerank_model")]
    pub rerank_model: String,
}

fn default_embed_model() -> String {
    String::new()
}

fn default_rerank_model() -> String {
    String::new()
}

fn default_llama_tuning_mode() -> String {
    benshu_inference::runtime::LLAMA_TUNING_AUTO.to_string()
}

fn default_llama_performance_profile() -> String {
    benshu_inference::runtime::PROFILE_BALANCED.to_string()
}

fn default_llama_ctx_size() -> u32 {
    8192
}

fn default_llama_gpu_layers() -> u32 {
    24
}

fn default_llama_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|threads| (threads.get() / 4).clamp(4, 8) as i32)
        .unwrap_or(8)
}

fn default_llama_batch_size() -> u32 {
    2048
}

fn default_llama_ubatch_size() -> u32 {
    512
}

fn default_llama_parallel_slots() -> u32 {
    1
}

fn default_llama_cache_ram() -> Option<u32> {
    Some(256)
}

fn default_llama_ctx_checkpoints() -> Option<u32> {
    Some(0)
}

fn default_llama_flash_attn_mode() -> String {
    "auto".to_string()
}

fn default_llama_kv_offload() -> bool {
    true
}

fn default_llama_mmap() -> bool {
    true
}

fn default_llama_cache_prompt() -> bool {
    false
}

fn default_continuation_cache_budget_mb() -> u64 {
    4096
}

fn default_continuation_cache_max_entries() -> u32 {
    1024
}

fn default_continuation_cache_sensitive_tasks_disabled() -> bool {
    true
}

fn default_llama_cont_batching() -> bool {
    false
}

fn default_llama_warmup() -> bool {
    true
}

fn default_llama_context_shift() -> bool {
    false
}

fn default_llama_jinja() -> bool {
    true
}

fn default_llama_fit_mode() -> String {
    "on".to_string()
}

fn default_llama_mmproj_offload() -> bool {
    true
}

fn default_llama_reasoning_mode() -> String {
    "auto".to_string()
}

fn default_llama_reasoning_format() -> String {
    "auto".to_string()
}

fn default_llama_sampling_temperature() -> f32 {
    0.8
}

fn default_llama_sampling_top_k() -> i32 {
    40
}

fn default_llama_sampling_top_p() -> f32 {
    0.95
}

fn default_llama_sampling_min_p() -> f32 {
    0.05
}

fn default_llama_sampling_typical_p() -> f32 {
    1.0
}

fn default_llama_sampling_repeat_penalty() -> f32 {
    1.0
}

fn default_llama_sampling_presence_penalty() -> f32 {
    0.0
}

fn default_llama_sampling_frequency_penalty() -> f32 {
    0.0
}

fn default_llama_sampling_mirostat() -> i32 {
    0
}

fn default_llama_sampling_mirostat_eta() -> f32 {
    0.1
}

fn default_llama_sampling_mirostat_tau() -> f32 {
    5.0
}

fn default_windows_ml_runtime_family() -> String {
    "windows_ml_onnx_runtime".to_string()
}

fn default_runtime_host_control_mode() -> String {
    "disabled".to_string()
}

fn default_runtime_host_control_timeout_secs() -> u64 {
    60
}

fn default_windows_ml_execution_provider() -> String {
    "directml".to_string()
}

fn default_windows_ml_device_target() -> String {
    "auto".to_string()
}

fn default_windows_ml_cpu_fallback_policy() -> String {
    "allow".to_string()
}

fn default_windows_ml_graph_optimization() -> String {
    "all".to_string()
}

fn default_windows_ml_text_batch_size() -> u32 {
    8
}

fn default_windows_ml_text_max_sequence_length() -> u32 {
    1024
}

fn default_windows_ml_vision_max_image_side() -> u32 {
    1024
}

fn default_windows_ml_vision_resize_policy() -> String {
    "fit".to_string()
}

fn default_windows_ml_audio_sample_rate() -> u32 {
    16_000
}

fn default_windows_ml_audio_chunk_ms() -> u32 {
    30_000
}

fn default_windows_ml_image_width() -> u32 {
    1024
}

fn default_windows_ml_image_height() -> u32 {
    1024
}

fn default_windows_ml_image_steps() -> u32 {
    20
}

fn default_windows_ml_image_guidance() -> f32 {
    7.5
}

fn default_windows_ml_realtime_vad_window_ms() -> u32 {
    30
}

fn default_windows_ml_realtime_duplex_frame_ms() -> u32 {
    20
}

fn default_windows_ml_safety_threshold() -> f32 {
    0.5
}

fn default_auto_consolidation() -> bool {
    true
}

fn default_ram_limit() -> u32 {
    4
}
fn default_vram_limit() -> u32 {
    0
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            enable_vector: true,
            model_ram_limit_gb: 4,
            model_vram_limit_gb: 0,
            auto_consolidation_enabled: true,
            embed_model: default_embed_model(),
            rerank_model: default_rerank_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    pub active_provider: Option<String>,
    #[serde(default)]
    pub resilient_enabled: bool,
    pub fallback_provider: Option<String>,
    pub request_timeout_secs: Option<u64>,
    pub reset_timeout_secs: Option<u64>,
    pub failure_threshold: Option<u32>,
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub deepseek_api_key: Option<String>,
    pub minimax_api_key: Option<String>,
    pub openrouter_api_key: Option<String>,
    pub moonshot_api_key: Option<String>,
    pub zhipu_api_key: Option<String>,
    pub qwen_api_key: Option<String>,
    pub baidu_api_key: Option<String>,
    pub baidu_secret_key: Option<String>,
    pub xunfei_api_key: Option<String>,
    pub doubao_api_key: Option<String>,
    pub siliconflow_api_key: Option<String>,
    #[serde(default)]
    pub custom_providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            host: "0.0.0.0".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    pub data_dir: Option<PathBuf>,
}

impl AppConfig {
    pub fn sanitize_agent_runtime_overrides(&mut self) {
        self.agents = self
            .agents
            .iter()
            .map(|(role, overrides)| (role.clone(), overrides.runtime_only()))
            .filter(|(_, overrides)| !overrides.is_runtime_empty())
            .collect();
    }

    pub fn apply_hidden_agent_overrides(
        &self,
        role: &str,
        mut file_overrides: AgentConfigOverrides,
    ) -> AgentConfigOverrides {
        if let Some(hidden) = self.agents.get(role).cloned() {
            if hidden.provider.is_some() {
                file_overrides.provider = hidden.provider;
            }
            if hidden.base_url.is_some() {
                file_overrides.base_url = hidden.base_url;
            }
            if hidden.model.is_some() {
                file_overrides.model = hidden.model;
            }
            if hidden.local_model_artifact.is_some() {
                file_overrides.local_model_artifact = hidden.local_model_artifact;
            }
            if hidden.local_mmproj_artifact.is_some() {
                file_overrides.local_mmproj_artifact = hidden.local_mmproj_artifact;
            }
            if hidden.local_runtime_family.is_some() {
                file_overrides.local_runtime_family = hidden.local_runtime_family;
            }
        }

        file_overrides
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_agent_overrides_from_file(
        &self,
        config_path: &std::path::Path,
        role: &str,
    ) -> Option<AgentConfigOverrides> {
        let base_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
        let base_agent_path = self
            .agent_path
            .clone()
            .unwrap_or_else(|| base_dir.join("agents"));
        let agent_file = base_agent_path.join(role).join("AGENT.md");
        let content = std::fs::read_to_string(agent_file).ok()?;
        let (file_overrides, _) = AgentConfigOverrides::parse_frontmatter(&content);
        Some(self.apply_hidden_agent_overrides(role, file_overrides))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn migrate_agent_runtime_overrides_from_frontmatter(
        &mut self,
        config_path: &std::path::Path,
    ) -> Result<bool> {
        let base_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
        let base_agent_path = self
            .agent_path
            .clone()
            .unwrap_or_else(|| base_dir.join("agents"));
        let mut changed = false;

        if !base_agent_path.exists() {
            return Ok(false);
        }

        for entry in std::fs::read_dir(base_agent_path)? {
            let entry = entry?;
            if !entry.path().is_dir() {
                continue;
            }

            let role = entry.file_name().to_string_lossy().to_string();
            let agent_file = entry.path().join("AGENT.md");
            let Ok(content) = std::fs::read_to_string(&agent_file) else {
                continue;
            };
            let (file_overrides, _) = AgentConfigOverrides::parse_frontmatter(&content);
            if file_overrides.provider.is_none()
                && file_overrides.base_url.is_none()
                && file_overrides.model.is_none()
                && file_overrides.local_model_artifact.is_none()
                && file_overrides.local_mmproj_artifact.is_none()
                && file_overrides.local_runtime_family.is_none()
            {
                continue;
            }

            let target = self.agents.entry(role).or_default();
            if target.provider.is_none() && file_overrides.provider.is_some() {
                target.provider = file_overrides.provider;
                changed = true;
            }
            if target.base_url.is_none() && file_overrides.base_url.is_some() {
                target.base_url = file_overrides.base_url;
                changed = true;
            }
            if target.model.is_none() && file_overrides.model.is_some() {
                target.model = file_overrides.model;
                changed = true;
            }
            if target.local_model_artifact.is_none()
                && file_overrides.local_model_artifact.is_some()
            {
                target.local_model_artifact = file_overrides.local_model_artifact;
                changed = true;
            }
            if target.local_mmproj_artifact.is_none()
                && file_overrides.local_mmproj_artifact.is_some()
            {
                target.local_mmproj_artifact = file_overrides.local_mmproj_artifact;
                changed = true;
            }
            if target.local_runtime_family.is_none()
                && file_overrides.local_runtime_family.is_some()
            {
                target.local_runtime_family = file_overrides.local_runtime_family;
                changed = true;
            }
        }

        Ok(changed)
    }

    pub fn effective_global_model_binding(
        &self,
        role: &str,
        configured_model: impl AsRef<str>,
    ) -> String {
        self.windows_ml_bridge
            .bindings
            .get(role)
            .map(|binding| binding.effective_model.clone())
            .filter(|effective| !effective.trim().is_empty())
            .unwrap_or_else(|| configured_model.as_ref().to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from_file(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut content = std::fs::read_to_string(path)?;

        // Phase 1: Basic environment variable expansion for ${VAR} pattern
        // This handles the primitive case mentioned in the example.
        content = expand_env_vars(&content);

        let mut config: Self = serde_yaml_ng::from_str(&content)
            .map_err(|e| crate::error::Error::Internal(format!("Failed to parse config: {}", e)))?;

        // Phase 2: Resolve vault:// references using system keychain/env
        let vault = vault::CompositeVault::default_system();
        config.resolve_secrets(&vault)?;
        config.sanitize_agent_runtime_overrides();

        Ok(config)
    }

    pub fn resolve_secrets(&mut self, vault: &dyn vault::SecretVault) -> Result<()> {
        // Resolve provider keys
        self.providers.openai_api_key = resolve_one(self.providers.openai_api_key.take(), vault)?;
        self.providers.anthropic_api_key =
            resolve_one(self.providers.anthropic_api_key.take(), vault)?;
        self.providers.gemini_api_key = resolve_one(self.providers.gemini_api_key.take(), vault)?;
        self.providers.deepseek_api_key =
            resolve_one(self.providers.deepseek_api_key.take(), vault)?;
        self.providers.minimax_api_key = resolve_one(self.providers.minimax_api_key.take(), vault)?;
        self.providers.openrouter_api_key =
            resolve_one(self.providers.openrouter_api_key.take(), vault)?;
        self.providers.moonshot_api_key =
            resolve_one(self.providers.moonshot_api_key.take(), vault)?;
        self.providers.zhipu_api_key = resolve_one(self.providers.zhipu_api_key.take(), vault)?;
        self.providers.qwen_api_key = resolve_one(self.providers.qwen_api_key.take(), vault)?;
        self.providers.baidu_api_key = resolve_one(self.providers.baidu_api_key.take(), vault)?;
        self.providers.baidu_secret_key =
            resolve_one(self.providers.baidu_secret_key.take(), vault)?;
        self.providers.xunfei_api_key = resolve_one(self.providers.xunfei_api_key.take(), vault)?;
        self.providers.doubao_api_key = resolve_one(self.providers.doubao_api_key.take(), vault)?;
        self.providers.siliconflow_api_key =
            resolve_one(self.providers.siliconflow_api_key.take(), vault)?;

        // Resolve connector tokens
        if let Some(tg) = &mut self.connectors.telegram {
            tg.bot_token = resolve_one(Some(tg.bot_token.clone()), vault)?.unwrap_or_default();
        }
        if let Some(ds) = &mut self.connectors.discord {
            ds.bot_token = resolve_one(Some(ds.bot_token.clone()), vault)?.unwrap_or_default();
        }
        if let Some(sl) = &mut self.connectors.slack {
            sl.bot_token = resolve_one(Some(sl.bot_token.clone()), vault)?.unwrap_or_default();
            sl.app_token = resolve_one(sl.app_token.take(), vault)?;
        }
        if let Some(em) = &mut self.connectors.email {
            em.smtp_pass = resolve_one(Some(em.smtp_pass.clone()), vault)?.unwrap_or_default();
            em.imap_pass = resolve_one(Some(em.imap_pass.clone()), vault)?.unwrap_or_default();
        }
        if let Some(qq) = &mut self.connectors.qq {
            qq.app_secret = resolve_one(Some(qq.app_secret.clone()), vault)?.unwrap_or_default();
        }

        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_to_file(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut sanitized = self.clone();
        sanitized.sanitize_agent_runtime_overrides();
        let content = serde_yaml_ng::to_string(&sanitized).map_err(|e| {
            crate::error::Error::Internal(format!("Failed to serialize config: {}", e))
        })?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::vault::SecretVault;
    use benshu_infra::error::Result as InfraResult;

    struct MockVault;
    impl SecretVault for MockVault {
        fn get(&self, key: &str) -> InfraResult<Option<String>> {
            if key == "SECRET_API_KEY" {
                Ok(Some("mocked-secret-key".to_string()))
            } else {
                Ok(None)
            }
        }
    }

    #[test]
    fn test_expand_env_vars() {
        std::env::set_var("TEST_VAR", "hello-world");
        let content = "api_key = \"${TEST_VAR}\"";
        let expanded = expand_env_vars(content);
        assert_eq!(expanded, "api_key = \"hello-world\"");
    }

    #[test]
    fn test_resolve_secrets() {
        let mut config = AppConfig::default();
        config.providers.openai_api_key = Some("vault://SECRET_API_KEY".to_string());

        config.resolve_secrets(&MockVault).unwrap();

        assert_eq!(
            config.providers.openai_api_key.unwrap(),
            "mocked-secret-key"
        );
    }

    #[test]
    fn artifact_policy_yaml_accepts_wrapped_policy_documents() {
        let policy = AgentConfigOverrides::parse_artifact_policy_yaml(
            "artifact_policy:\n  handles:\n    - artifact: report\n",
        )
        .expect("policy yaml parses")
        .expect("policy should be present");

        assert!(policy.get("artifact_policy").is_none());
        assert_eq!(policy["handles"][0]["artifact"], "report");
    }
}

fn resolve_one(value: Option<String>, vault: &dyn vault::SecretVault) -> Result<Option<String>> {
    match value {
        Some(s) if s.starts_with("vault://") => {
            let key = &s[8..];
            match vault.get(key)? {
                Some(secret) => Ok(Some(secret)),
                None => {
                    tracing::warn!("Vault key '{}' not found, using literal reference", key);
                    Ok(Some(s))
                }
            }
        }
        _ => Ok(value),
    }
}

fn expand_env_vars(content: &str) -> String {
    let re = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();
    re.replace_all(content, |caps: &regex::Captures| {
        let key = &caps[1];
        std::env::var(key).unwrap_or_else(|_| format!("${{{}}}", key))
    })
    .to_string()
}
