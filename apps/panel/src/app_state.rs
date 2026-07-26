//! Application state shared across all egui panels.

use crate::i18n::Language;

use crate::api::{
    ActiveSandboxContext, ApprovalDecisionReceipt, ArtifactCleanupPolicy, ArtifactCleanupReport,
    ArtifactQuery, ArtifactRecord, ChatResponse, ConfigUpdateResult, CronJob, GatewayClient,
    InstallSkillResponse, MemoryRestoreDeleteReport, MemoryRestoreDryRunReport,
    MemoryRestorePointManifest, MemoryRestorePolicyBasis, MemoryRestoreReceipt,
    OpenArtifactTargetResponse, RuntimeMode, SessionInfo, SessionTaskInfo, SkillInfo,
    TaskOutputInfo, TaskWaitInfo,
};
use eframe::egui;
use poll_promise::Promise;
use std::path::Path;
use tokio::runtime::Handle;

pub(crate) fn default_llama_runtime_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|threads| (threads.get() / 4).clamp(4, 8) as i32)
        .unwrap_or(8)
}

#[derive(Debug, Clone)]
pub struct ChatAttachmentDraft {
    pub path: String,
    pub media_type: String,
    pub display_name: String,
}

impl ChatAttachmentDraft {
    pub fn from_path(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let media_type = classify_chat_attachment(path).to_string();
        let display_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        Self {
            path: path.to_string_lossy().to_string(),
            media_type,
            display_name,
        }
    }

    pub fn to_api_media(&self) -> crate::api::ChatMediaAttachment {
        crate::api::ChatMediaAttachment {
            media_type: self.media_type.clone(),
            url: self.path.clone(),
            caption: Some(self.display_name.clone()),
        }
    }
}

fn classify_chat_attachment(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("gif") => "image/gif",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("m4a") => "audio/mp4",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("opus") => "audio/opus",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("pdf") => "application/pdf",
        Some("json") => "application/json",
        Some("md" | "markdown") => "text/markdown",
        Some("csv") => "text/csv",
        Some("html") => "text/html",
        Some("css") => "text/css",
        Some("xml") => "application/xml",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "text/plain",
    }
}

/// Which tab is currently active.
#[derive(Debug, Clone, PartialEq)]
pub enum ActiveTab {
    Skills,
    Models,
    Logs,
    Agent,
    Connection,
    Dashboard,
    System,
    Channels,
}

impl Default for ActiveTab {
    fn default() -> Self {
        Self::Skills
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkillsSubTab {
    Installed,
    Manual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentSubTab {
    Editor,
    Chat,
    Tasks,
    A2A,
    Metrics,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentTaskSubTab {
    Scheduled,
    HighRisk,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SystemSubTab {
    General,
    Artifacts,
    Doctor,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApiSubTab {
    Cloud,
    Local,
}

/// A vault entry being entered by the user.
#[derive(Default, Clone)]
pub struct VaultEntry {
    pub key: String,
    pub value: String,
    pub saved: bool,
    pub error: Option<String>,
}

/// The full panel application state.
pub struct AppState {
    /// Current active tab
    pub tab: ActiveTab,
    pub skills_subtab: SkillsSubTab,
    pub agent_subtab: AgentSubTab,
    pub agent_task_subtab: AgentTaskSubTab,
    pub api_subtab: ApiSubTab,
    pub system_subtab: SystemSubTab,

    /// Global sensory toggles
    pub enable_global_voice: bool,
    pub enable_local_vision: bool,
    pub local_vision_status: String, // "Not Configured", "Downloading", "Ready"

    /// The gateway endpoint URL (editable in Connection tab)
    pub gateway_url: String,

    /// API client (recreated when URL changes)
    pub client: GatewayClient,

    /// The session token provided by main.rs (P11 Handshake)
    pub session_token: Option<String>,

    /// Async load of skills
    pub skills_promise: Option<Promise<Result<Vec<SkillInfo>, String>>>,

    /// Cached skills list (after successful load)
    pub skills: Vec<SkillInfo>,

    /// Vault entries being edited
    pub vault_entries: Vec<VaultEntry>,
    pub new_vault_key: String,
    pub new_vault_value: String,
    pub vault_show_value: bool,

    /// Connection status
    pub connected: Option<bool>,
    pub gateway_version: Option<String>,

    /// Log buffer (filled via periodic polling)
    pub log_lines: Vec<String>,

    /// Currently expanded skill (for detail popup)
    pub expanded_skill: Option<String>,

    /// Timer tracking (egui time = seconds since startup, works on WASM too)
    pub last_log_poll_time: f64,
    pub last_skill_refresh_time: f64,
    /// Whether auto-refresh is enabled for logs
    pub auto_log_poll: bool,
    /// Pending one-shot log fetch promise
    pub pending_log_promise: Option<Promise<Vec<String>>>,
    pub pending_runtime_config_promise: Option<Promise<Result<ConfigUpdateResult, String>>>,

    /// Last error/status message (displayed in footer)
    pub status_msg: Option<(String, bool)>, // (message, is_error)

    // ── Cron state ───────────────────────────────────────────────────────────
    pub cron_jobs: Vec<CronJob>,
    pub cron_loading: bool,
    pub cron_error: Option<String>,
    pub last_cron_refresh_time: f64,
    pub pending_cron_promise: Option<Promise<Result<Vec<CronJob>, String>>>,
    pub pending_cron_action_promise: Option<Promise<Result<String, String>>>,

    // New job form
    pub cron_form_name: String,
    pub cron_form_schedule: String, // "every" | "cron" | "at"
    pub cron_form_interval: String, // seconds for "every"
    pub cron_form_expr: String,     // cron expr for "cron"
    pub cron_form_prompt: String,
    pub cron_form_role: String, // Selected agent role for the job

    // Visual Builder Helpers (Phase 7.1)
    pub cron_visual_mode: bool,
    pub cron_visual_freq: String, // "hourly", "daily", "weekly"
    pub cron_visual_hour: u32,
    pub cron_visual_minute: u32,
    pub cron_visual_weekday: String,

    // ── Sessions state ───────────────────────────────────────────────────────
    pub sessions: Vec<SessionInfo>,
    pub sessions_loading: bool,
    pub sessions_error: Option<String>,
    pub last_sessions_refresh_time: f64,
    pub pending_sessions_promise: Option<Promise<Result<Vec<SessionInfo>, String>>>,
    pub session_runtime_tasks: Vec<SessionTaskInfo>,
    pub session_runtime_tasks_loading: bool,
    pub session_runtime_tasks_error: Option<String>,
    pub session_runtime_tasks_session_id: Option<String>,
    pub last_session_runtime_tasks_refresh_time: f64,
    pub pending_session_runtime_tasks_promise:
        Option<Promise<Result<Vec<SessionTaskInfo>, String>>>,
    pub selected_task_output: Option<TaskOutputInfo>,
    pub selected_task_output_task_id: Option<String>,
    pub selected_task_output_loading: bool,
    pub selected_task_output_error: Option<String>,
    pub pending_task_output_promise: Option<Promise<Result<TaskOutputInfo, String>>>,
    pub selected_task_wait_notice: Option<String>,
    pub selected_task_wait_task_id: Option<String>,
    pub selected_task_wait_loading: bool,
    pub selected_task_wait_error: Option<String>,
    pub pending_task_wait_promise: Option<Promise<Result<TaskWaitInfo, String>>>,
    pub pending_task_cancel_promise: Option<Promise<Result<String, String>>>,
    pub selected_session_delegation_trace: Option<crate::api::SessionDelegationTrace>,
    pub selected_session_delegation_session_id: Option<String>,
    pub selected_session_delegation_loading: bool,
    pub selected_session_delegation_error: Option<String>,
    pub pending_session_delegation_promise:
        Option<Promise<Result<crate::api::SessionDelegationTrace, String>>>,
    pub selected_run_trace: Option<benshu_telemetry::RunTrace>,
    pub selected_run_trace_id: Option<String>,
    pub selected_run_trace_loading: bool,
    pub selected_run_trace_error: Option<String>,
    pub pending_run_trace_promise: Option<Promise<Result<benshu_telemetry::RunTrace, String>>>,
    pub selected_run_replay: Option<benshu_telemetry::RunReplay>,
    pub selected_run_replay_loading: bool,
    pub selected_run_replay_error: Option<String>,
    pub pending_run_replay_promise: Option<Promise<Result<benshu_telemetry::RunReplay, String>>>,
    pub selected_profiler_artifact: Option<benshu_telemetry::ProfilerArtifact>,
    pub selected_profiler_loading: bool,
    pub selected_profiler_error: Option<String>,
    pub pending_profiler_promise:
        Option<Promise<Result<benshu_telemetry::ProfilerArtifact, String>>>,
    pub selected_profiler_query_results: Vec<benshu_telemetry::ProfilerArtifact>,
    pub selected_profiler_query_loading: bool,
    pub selected_profiler_query_error: Option<String>,
    pub pending_profiler_query_promise:
        Option<Promise<Result<Vec<benshu_telemetry::ProfilerArtifact>, String>>>,
    pub selected_profiler_export: Option<benshu_telemetry::ProfilerExport>,
    pub selected_profiler_export_loading: bool,
    pub selected_profiler_export_error: Option<String>,
    pub pending_profiler_export_promise:
        Option<Promise<Result<benshu_telemetry::ProfilerExport, String>>>,
    pub selected_witness_summary: Option<benshu_telemetry::WitnessSummary>,
    pub selected_witness_id: Option<String>,
    pub selected_witness_loading: bool,
    pub selected_witness_error: Option<String>,
    pub pending_witness_promise: Option<Promise<Result<benshu_telemetry::WitnessSummary, String>>>,
    pub selected_witness_bundle: Option<benshu_telemetry::WitnessBundle>,
    pub selected_witness_bundle_loading: bool,
    pub selected_witness_bundle_error: Option<String>,
    pub pending_witness_bundle_promise:
        Option<Promise<Result<benshu_telemetry::WitnessBundle, String>>>,
    pub selected_witness_log: Option<benshu_telemetry::WitnessLogEntry>,
    pub selected_witness_log_loading: bool,
    pub selected_witness_log_error: Option<String>,
    pub pending_witness_log_promise:
        Option<Promise<Result<benshu_telemetry::WitnessLogEntry, String>>>,
    pub selected_witness_query_results: Vec<benshu_telemetry::WitnessLogEntry>,
    pub selected_witness_query_loading: bool,
    pub selected_witness_query_error: Option<String>,
    pub pending_witness_query_promise:
        Option<Promise<Result<Vec<benshu_telemetry::WitnessLogEntry>, String>>>,
    pub selected_scorecard_query_results: Vec<benshu_telemetry::Scorecard>,
    pub selected_scorecard_query_loading: bool,
    pub selected_scorecard_query_error: Option<String>,
    pub pending_scorecard_query_promise:
        Option<Promise<Result<Vec<benshu_telemetry::Scorecard>, String>>>,

    // ── Artifact registry state ─────────────────────────────────────────────
    pub artifacts: Vec<ArtifactRecord>,
    pub artifacts_loading: bool,
    pub artifacts_error: Option<String>,
    pub artifacts_query: ArtifactQuery,
    pub selected_artifact_id: Option<String>,
    pub pending_artifacts_promise: Option<Promise<Result<Vec<ArtifactRecord>, String>>>,
    pub artifact_cleanup_policy: ArtifactCleanupPolicy,
    pub artifact_cleanup_loading: bool,
    pub artifact_cleanup_error: Option<String>,
    pub last_artifact_cleanup_report: Option<ArtifactCleanupReport>,
    pub pending_artifact_cleanup_promise: Option<Promise<Result<ArtifactCleanupReport, String>>>,
    pub open_target_promise: Option<Promise<Result<OpenArtifactTargetResponse, String>>>,
    pub runtime_mode_loading: bool,
    pub runtime_mode_error: Option<String>,
    pub pending_runtime_mode_promise: Option<Promise<Result<RuntimeMode, String>>>,
    pub local_model_stack: Option<crate::api::LocalModelStack>,
    pub local_model_stack_loading: bool,
    pub local_model_stack_error: Option<String>,
    pub pending_local_model_stack_promise:
        Option<Promise<Result<crate::api::LocalModelStack, String>>>,
    pub local_model_artifacts: Option<crate::api::LocalModelArtifactCatalog>,
    pub local_model_artifacts_loading: bool,
    pub local_model_artifacts_error: Option<String>,
    pub pending_local_model_artifacts_promise:
        Option<Promise<Result<crate::api::LocalModelArtifactCatalog, String>>>,
    pub knowledge_import_collection: String,
    pub knowledge_import_loading: bool,
    pub knowledge_import_error: Option<String>,
    pub last_knowledge_import_report: Option<crate::api::KnowledgeImportReport>,
    pub pending_knowledge_import_promise:
        Option<Promise<Result<crate::api::KnowledgeImportReport, String>>>,
    pub knowledge_documents: Vec<crate::api::KnowledgeDocumentInfo>,
    pub knowledge_documents_loading: bool,
    pub knowledge_documents_error: Option<String>,
    pub pending_knowledge_documents_promise:
        Option<Promise<Result<crate::api::KnowledgeDocumentsReport, String>>>,
    pub pending_knowledge_delete_promise:
        Option<Promise<Result<crate::api::KnowledgeDeleteReport, String>>>,
    pub novel_projects_root: String,
    pub novel_projects: Vec<crate::api::NovelProjectInfo>,
    pub novel_projects_loading: bool,
    pub novel_projects_error: Option<String>,
    pub pending_novel_projects_promise:
        Option<Promise<Result<crate::api::NovelProjectsReport, String>>>,
    pub selected_novel_project_path: Option<String>,
    pub novel_export_format: String,
    pub novel_export_approved_only: bool,
    pub novel_export_loading: bool,
    pub novel_export_error: Option<String>,
    pub last_novel_export: Option<crate::api::NovelExportReport>,
    pub pending_novel_export_promise:
        Option<Promise<Result<crate::api::NovelExportReport, String>>>,

    // ── Snapshot / Overview state ─────────────────────────────────────────
    /// Keys the user has explicitly deleted this session — prevents snapshot from re-adding them
    pub deleted_vault_keys: std::collections::HashSet<String>,
    pub fact_check_enabled: bool,
    pub image_gen_model: String,
    pub image_gen_status: String,
    pub tactical_model: String,
    pub windows_ml_runtime_family: String,
    pub windows_ml_execution_provider_preference: String,
    pub windows_ml_device_target: String,
    pub windows_ml_cpu_fallback_policy: String,
    pub windows_ml_graph_optimization_level: String,
    pub windows_ml_intra_threads: String,
    pub windows_ml_inter_threads: String,
    pub windows_ml_text_batch_size: u32,
    pub windows_ml_text_max_sequence_length: u32,
    pub windows_ml_vision_max_image_side: u32,
    pub windows_ml_vision_resize_policy: String,
    pub windows_ml_audio_sample_rate_hz: u32,
    pub windows_ml_audio_chunk_ms: u32,
    pub windows_ml_image_width: u32,
    pub windows_ml_image_height: u32,
    pub windows_ml_image_steps: u32,
    pub windows_ml_image_guidance: String,
    pub windows_ml_realtime_vad_window_ms: u32,
    pub windows_ml_duplex_frame_ms: u32,
    pub windows_ml_safety_threshold: String,
    pub llama_tuning_mode: String,
    pub llama_performance_profile: String,
    pub llama_runtime_diagnostics: String,
    pub llama_ctx_size: u32,
    pub llama_gpu_layers: u32,
    pub llama_threads: i32,
    pub llama_threads_batch: String,
    pub llama_batch_size: u32,
    pub llama_ubatch_size: u32,
    pub llama_parallel_slots: u32,
    pub llama_cache_ram: String,
    pub llama_ctx_checkpoints: String,
    pub llama_flash_attn_mode: String,
    pub llama_kv_offload: bool,
    pub llama_mmap: bool,
    pub llama_mlock: bool,
    pub llama_cache_prompt: bool,
    pub llama_cont_batching: bool,
    pub llama_warmup: bool,
    pub llama_context_shift: bool,
    pub llama_jinja: bool,
    pub llama_rope_scaling: String,
    pub llama_rope_scale: String,
    pub llama_rope_freq_base: String,
    pub llama_rope_freq_scale: String,
    pub llama_yarn_orig_ctx: String,
    pub llama_yarn_ext_factor: String,
    pub llama_yarn_attn_factor: String,
    pub llama_yarn_beta_slow: String,
    pub llama_yarn_beta_fast: String,
    pub llama_cache_type_k: String,
    pub llama_cache_type_v: String,
    pub llama_device: String,
    pub llama_split_mode: String,
    pub llama_tensor_split: String,
    pub llama_main_gpu: String,
    pub llama_fit_mode: String,
    pub llama_fit_target: String,
    pub llama_fit_ctx: String,
    pub llama_cpu_moe: bool,
    pub llama_n_cpu_moe: String,
    pub llama_mmproj_offload: bool,
    pub llama_image_min_tokens: String,
    pub llama_image_max_tokens: String,
    pub llama_reasoning_mode: String,
    pub llama_reasoning_format: String,
    pub llama_reasoning_budget: String,
    pub llama_reasoning_budget_message: String,
    pub llama_sampling_temperature: String,
    pub llama_sampling_top_k: String,
    pub llama_sampling_top_p: String,
    pub llama_sampling_min_p: String,
    pub llama_sampling_typical_p: String,
    pub llama_sampling_repeat_penalty: String,
    pub llama_sampling_presence_penalty: String,
    pub llama_sampling_frequency_penalty: String,
    pub llama_sampling_mirostat: String,
    pub llama_sampling_mirostat_eta: String,
    pub llama_sampling_mirostat_tau: String,
    pub llama_seed: String,
    pub voice_tts_model: String,
    pub voice_tts_voice: String,
    pub model_vram_limit_gb: u32,
    pub model_ram_limit_gb: u32,
    pub auto_consolidation_enabled: bool,

    // ── Store tab (Browse & Install) ──────────────────────────────────────
    pub store_install_url: String,
    pub store_installing: bool,
    pub store_install_error: Option<String>,
    pub store_install_success: Option<String>,
    pub pending_install_promise: Option<Promise<Result<InstallSkillResponse, String>>>,

    pub channels: Vec<crate::api::ChannelMetadata>,
    pub channels_loading: bool,
    pub channels_error: Option<String>,
    pub pending_channels_promise:
        Option<Promise<Result<crate::api::ChannelSchemaResponse, String>>>,

    // ── LLM Providers state ───────────────────────────────────────────────
    pub provider_metadata: Vec<crate::api::ProviderMetadata>,
    pub provider_loading: bool,
    pub provider_error: Option<String>,
    pub pending_provider_promise:
        Option<Promise<Result<crate::api::ProviderSchemaResponse, String>>>,

    // ── Agent Identity & Life state ──────────────────────────────────────
    pub agent_save_promise: Option<Promise<Result<(), String>>>,
    pub agent_export_promise: Option<Promise<Result<String, String>>>,
    pub agent_export_json: Option<String>,
    pub agent_export_save_path: Option<std::path::PathBuf>,
    pub agent_import_json: String,
    pub agent_show_import_window: bool,
    pub agent_import_promise: Option<Promise<Result<(), String>>>,
    pub agent_show_export_window: bool,
    pub agent_export_loading: bool,

    pub agent_role_selected: String,
    pub is_adding_agent: bool,
    pub is_editing_identity: bool,
    pub custom_added_agents: std::collections::BTreeSet<String>,
    pub agent_role_content: String,
    pub agent_role_dirty: bool,
    pub agent_role_artifact_policy_dirty: bool,
    pub agent_role_promise: Option<Promise<Result<crate::api::FileDto, String>>>,
    pub agent_role_loaded: bool,
    pub agent_role_provider: String,
    pub agent_role_base_url: String,
    pub agent_role_model: String,
    pub agent_role_local_model_artifact: String,
    pub agent_role_local_mmproj_artifact: String,
    pub agent_role_local_runtime_family: String,
    pub agent_role_temperature: String,
    pub agent_role_auto_consolidation: bool,
    pub agent_role_tools: Vec<String>,
    pub agent_role_pending_tool: String,
    pub agent_role_artifact_policy_yaml: String,
    pub agent_role_artifact_policy_error: Option<String>,
    pub agent_ocean_openness: f32,
    pub agent_ocean_conscientiousness: f32,
    pub agent_ocean_extraversion: f32,
    pub agent_ocean_agreeableness: f32,
    pub agent_ocean_neuroticism: f32,
    pub agent_list_promise: Option<Promise<Result<Vec<crate::api::AgentSummary>, String>>>,
    pub agent_list: Vec<crate::api::AgentSummary>,
    pub agent_templates: Vec<crate::api::AgentTemplate>,
    pub agent_templates_promise: Option<Promise<Result<Vec<crate::api::AgentTemplate>, String>>>,
    pub last_agent_list_refresh_time: f64,
    pub agent_role_name: String,
    pub agent_role_description: String,
    pub agent_role_tone: String,
    pub agent_role_constraints: Vec<String>,
    pub agent_role_backstory: String,

    // ── Chat state ──────────────────────────────────────────────────────────
    pub chat_histories: std::collections::HashMap<String, Vec<ChatMessage>>,
    pub chat_sessions: std::collections::HashMap<String, Vec<String>>,
    pub active_chat_session: std::collections::HashMap<String, String>,
    pub chat_input: String,
    pub chat_attachments: Vec<ChatAttachmentDraft>,
    pub chat_selected_role: String,
    pub chat_loading: bool,
    pub chat_promise: Option<Promise<Result<crate::api::ChatResponse, String>>>,
    pub chat_history_promise: Option<Promise<Result<Vec<ChatMessage>, String>>>,
    pub pending_chat_task_output_promise: Option<Promise<Result<TaskOutputInfo, String>>>,
    pub pending_chat_task_output_task_id: Option<String>,
    pub pending_chat_task_output_session_id: Option<String>,
    pub chat_task_output_appended: std::collections::HashSet<String>,
    // ── Task Cancellation state (Phase 11-B) ───────────────────────
    pub cancel_promise: Option<Promise<Result<(), String>>>,
    pub pending_delete_agent: Option<String>,
    pub pending_delete_session: Option<(String, usize)>,

    // ── Diagnostic / Doctor state ───────────────────────────────────────────
    pub doctor_loading: bool,
    pub doctor_error: Option<String>,
    pub doctor_results: Option<Vec<crate::api::DoctorCheckResult>>,
    pub pending_doctor_promise: Option<Promise<Result<Vec<crate::api::DoctorCheckResult>, String>>>,
    pub repair_loading: bool,
    pub pending_repair_promise: Option<Promise<Result<String, String>>>,

    // ── Exit Dialog state ───────────────────────────────────────────────────
    pub show_exit_dialog: bool,
    pub exit_in_progress: bool,

    // ── Metrics Display state ───────────────────────────────────────────────
    pub metrics_loading: bool,
    pub metrics_error: Option<String>,
    pub last_metrics: Option<crate::api::Metrics>,
    pub pending_metrics_promise: Option<Promise<Result<crate::api::Metrics, String>>>,
    pub last_metrics_refresh_time: f64,
    pub metrics_history: Vec<MetricsSnapshot>,

    // ── Channels state ──────────────────────────────────────────────────────
    pub channel_metadata: Vec<crate::api::ChannelMetadata>,
    pub running_channels: Vec<String>,
    pub channel_observability: std::collections::HashMap<String, crate::api::ChannelObservability>,
    pub channel_metadata_promise:
        Option<Promise<Result<crate::api::ChannelSchemaResponse, String>>>,

    // ── Sandboxes state ─────────────────────────────────────────────────────
    pub sandboxes: Vec<crate::api::ActiveSandboxContext>,
    pub sandboxes_promise: Option<Promise<Result<Vec<crate::api::ActiveSandboxContext>, String>>>,
    pub last_sandboxes_refresh_time: f64,

    // ── Local Model Resource Management ─────────────────────────────────────

    // ── Approvals state (Roadmap Phase 6.1) ──────────────────────────
    pub approvals: Vec<crate::api::ApprovalInfo>,
    pub approval_receipts: Vec<ApprovalDecisionReceipt>,
    pub approval_error: Option<String>,
    pub pending_approval_promise: Option<Promise<Result<Vec<crate::api::ApprovalInfo>, String>>>,
    pub pending_approval_receipts_promise:
        Option<Promise<Result<Vec<ApprovalDecisionReceipt>, String>>>,
    pub last_approval_refresh_time: f64,
    pub approval_resolve_promise: Option<Promise<Result<(), String>>>,
    pub approval_receipt_error: Option<String>,

    pub restore_points: Vec<MemoryRestorePointManifest>,
    pub selected_restore_backup_id: Option<String>,
    pub selected_restore_dry_run: Option<MemoryRestoreDryRunReport>,
    pub selected_restore_policy_basis: Option<MemoryRestorePolicyBasis>,
    pub selected_restore_receipts: Vec<MemoryRestoreReceipt>,
    pub selected_restore_delete_report: Option<MemoryRestoreDeleteReport>,
    pub restore_points_error: Option<String>,
    pub restore_points_loading: bool,
    pub pending_restore_create_promise: Option<Promise<Result<MemoryRestorePointManifest, String>>>,
    pub pending_restore_points_promise:
        Option<Promise<Result<Vec<MemoryRestorePointManifest>, String>>>,
    pub pending_restore_dry_run_promise: Option<Promise<Result<MemoryRestoreDryRunReport, String>>>,
    pub pending_restore_policy_promise: Option<Promise<Result<MemoryRestorePolicyBasis, String>>>,
    pub pending_restore_receipts_promise:
        Option<Promise<Result<Vec<MemoryRestoreReceipt>, String>>>,
    pub pending_restore_execute_promise: Option<Promise<Result<MemoryRestoreReceipt, String>>>,
    pub pending_restore_delete_promise: Option<Promise<Result<MemoryRestoreDeleteReport, String>>>,

    pub night_mode: bool,

    // ── Reranker & Embedder state (Phase 7.2) ──────────────────────────────
    // ── Unified Organ Model Selection (Phase 21.4) ──────────────────────────────
    pub organ_stt_model: String,
    pub organ_tts_model: String,
    pub organ_embed_model: String,
    pub organ_rerank_model: String,
    pub organ_ocr_model: String,
    pub organ_vision_model: String,
    pub organ_fact_check_model: String,
    pub organ_image_edit_model: String,
    pub organ_audio_understanding_model: String,
    pub organ_realtime_vad_model: String,
    pub organ_duplex_voice_model: String,
    pub organ_local_classifier_model: String,
    pub organ_local_router_model: String,
    pub organ_local_safety_model: String,
    pub use_local_ocr: Option<bool>,

    /// UI Language state
    pub language: Language,

    /// Tracked UI scale factor (based on window width). Updated every frame.
    /// Used to avoid re-setting text styles when scale hasn't changed.
    pub last_ui_scale: f32,

    /// ── A2A diagnostics state ───────────────────────────────────────────
    pub a2a_agents: Vec<String>,
    pub a2a_board: std::collections::HashMap<String, String>,
    pub a2a_loading: bool,
    pub a2a_error: Option<String>,
    pub a2a_throttle_tenant: String,
    pub a2a_throttle_role: String,
    pub a2a_throttle_limit: u32,
    pub a2a_throttle_promise: Option<Promise<Result<(), String>>>,
    pub last_a2a_refresh_time: f64,
    pub pending_a2a_promise: Option<poll_promise::Promise<Result<crate::api::A2aSummary, String>>>,

    /// Whether we have performed the initial 50% screen-size resize.
    pub initial_resize_done: bool,

    // ── Update state ────────────────────────────────────────────────────────
    pub update_in_progress: bool,
    pub update_status: Option<String>,
    pub update_promise: Option<Promise<Result<String, String>>>,

    // ── Workspace / Sandbox state (Phase 7.1) ──────────────────────────────
    pub trusted_workspaces: Vec<String>,
    pub workspace_loading: bool,
    pub last_workspace_refresh_time: f64,
    pub pending_workspace_promise: Option<Promise<Result<Vec<String>, String>>>,
    pub workspace_form_path: String,

    // ── Rollback state (Phase 15.3) ────────────────────────────────────────
    pub rollback_promise: Option<Promise<Result<(), String>>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String, // "user" or "agent"
    pub content: String,
    pub agent_name: Option<String>,
    pub reasoning: Option<String>, // "thought"
    #[serde(default)]
    pub tool_calls: Vec<ToolCallTrace>,
    #[serde(default)]
    pub artifacts: Vec<crate::api::ChatArtifactRef>,
    #[serde(default)]
    pub chat_route: Option<String>,
    #[serde(default)]
    pub tool_surface_mode: Option<String>,
    #[serde(default)]
    pub runtime_persistence_status: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub trace_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCallTrace {
    pub name: String,
    pub args: String,
    pub result: Option<String>,
    pub backup: Option<crate::api::BackupInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetricsSnapshot {
    pub time: f64,
    pub total_calls: u64,
    pub cpu_usage: f32,
    pub ram_usage: f32,
    pub vram_usage: f32,
}

impl AppState {
    fn parse_optional_string(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn local_provider_ids() -> &'static [&'static str] {
        &[
            "native",
            "local",
            "llama_cpp",
            "ollama",
            "localai",
            "lmstudio",
            "x-engram",
            "koboldcpp",
        ]
    }

    fn model_looks_like_local_path(model: &str) -> bool {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            return false;
        }

        if Path::new(trimmed).is_absolute() {
            return true;
        }

        trimmed.starts_with("\\\\")
            || trimmed.starts_with("./")
            || trimmed.starts_with("../")
            || trimmed.contains("/models/")
            || trimmed.ends_with(".gguf")
            || trimmed.ends_with(".onnx")
            || trimmed.ends_with(".safetensors")
    }

    fn base_url_looks_local_service(base_url: &str) -> bool {
        let trimmed = base_url.trim().to_lowercase();
        if trimmed.is_empty() {
            return false;
        }

        trimmed.contains("127.0.0.1")
            || trimmed.contains("localhost")
            || trimmed.contains("0.0.0.0")
            || trimmed.contains("172.18.")
            || trimmed.contains("wsl.localhost")
    }

    pub fn agent_role_prefers_local_execution(&self) -> bool {
        Self::model_looks_like_local_path(&self.agent_role_model)
            || !self.agent_role_local_model_artifact.trim().is_empty()
            || Self::base_url_looks_local_service(&self.agent_role_base_url)
            || Self::local_provider_ids()
                .contains(&self.agent_role_provider.trim().to_lowercase().as_str())
    }

    fn selected_local_main_brain_artifact(&self) -> Option<&crate::api::LocalModelArtifact> {
        let selected = self.agent_role_model.trim();
        let local_source = self.agent_role_local_model_artifact.trim();
        self.local_model_artifacts.as_ref().and_then(|catalog| {
            catalog.artifacts.iter().find(|artifact| {
                artifact.selectable_as_main_brain
                    && (!selected.is_empty() && artifact.path == selected
                        || !local_source.is_empty() && artifact.path == local_source)
            })
        })
    }

    pub fn resolved_agent_provider_and_base_url(&self) -> (String, Option<String>) {
        let model = self.agent_role_model.trim();
        let base_url = self.agent_role_base_url.trim();
        let provider = self.agent_role_provider.trim().to_lowercase();

        if Self::model_looks_like_local_path(model) {
            return ("native".to_string(), None);
        }

        if !base_url.is_empty() {
            return ("openai".to_string(), Some(base_url.to_string()));
        }

        if provider.is_empty() {
            ("".to_string(), None)
        } else {
            (provider, None)
        }
    }

    pub fn resolved_agent_runtime_config(&self) -> Option<crate::api::AgentRuntimeConfigDto> {
        let provider = self.agent_role_provider.trim().to_lowercase();
        let base_url = self.agent_role_base_url.trim();
        let model = self.agent_role_model.trim();
        let selected_local = self.selected_local_main_brain_artifact();
        let local_model_artifact = selected_local
            .map(|artifact| artifact.path.clone())
            .or_else(|| {
                let local = self.agent_role_local_model_artifact.trim();
                (!local.is_empty()).then_some(local.to_string())
            });
        let local_mmproj_artifact = selected_local
            .and_then(|artifact| artifact.resolved_mmproj_path.clone())
            .or_else(|| {
                let mmproj = self.agent_role_local_mmproj_artifact.trim();
                (!mmproj.is_empty()).then_some(mmproj.to_string())
            });
        let local_runtime_family = if local_model_artifact.is_some() {
            Some("llama_cpp".to_string())
        } else {
            let family = self.agent_role_local_runtime_family.trim();
            (!family.is_empty()).then_some(family.to_string())
        };

        let runtime = crate::api::AgentRuntimeConfigDto {
            provider: (!provider.is_empty()).then_some(provider),
            base_url: (!base_url.is_empty()).then_some(base_url.to_string()),
            model: (!model.is_empty()).then_some(model.to_string()),
            local_model_artifact,
            local_mmproj_artifact,
            local_runtime_family,
        };

        if runtime.provider.is_none()
            && runtime.base_url.is_none()
            && runtime.model.is_none()
            && runtime.local_model_artifact.is_none()
            && runtime.local_mmproj_artifact.is_none()
            && runtime.local_runtime_family.is_none()
        {
            None
        } else {
            Some(runtime)
        }
    }

    pub fn resolved_agent_execution_summary(&self) -> String {
        let (provider, base_url) = self.resolved_agent_provider_and_base_url();
        if let Some(artifact) = self.selected_local_main_brain_artifact() {
            let mmproj = artifact
                .resolved_mmproj_path
                .as_deref()
                .map_or("none", |path| path);
            format!(
                "Local native model selected. BenShu will persist the discovered GGUF loadout as a local runtime source (model={}, mmproj={}).",
                artifact.relative_path, mmproj
            )
        } else if Self::model_looks_like_local_path(&self.agent_role_model) {
            "Local native model selected. BenShu will load the model directly from disk."
                .to_string()
        } else if let Some(base_url) = base_url {
            format!(
                "Custom endpoint selected. BenShu will use the OpenAI-compatible bridge at {}.",
                base_url
            )
        } else {
            let provider = provider.trim();
            if provider.is_empty() {
                "No runtime provider selected yet. Choose a local model or cloud provider before saving this agent.".to_string()
            } else {
                format!(
                    "Cloud provider selected. BenShu will use provider '{}'.",
                    provider
                )
            }
        }
    }

    pub fn new(token: Option<String>) -> Self {
        let url = load_saved_url().unwrap_or_else(|| "http://127.0.0.1:3000".to_string());
        let saved = load_saved_config();
        let tab = saved.tab;
        let night_mode = saved.night_mode;
        let language = saved.language;
        let mut client = GatewayClient::new(url.clone());
        if let Some(t) = &token {
            client = client.with_token(t.clone());
        }

        // Cache the session token globally so we don't lose it on URL changes
        let session_token = token;

        // Pre-populate vault with common key names
        let vault_entries = vec![
            VaultEntry {
                key: "OPENAI_API_KEY".to_string(),
                ..Default::default()
            },
            VaultEntry {
                key: "ANTHROPIC_API_KEY".to_string(),
                ..Default::default()
            },
            VaultEntry {
                key: "GEMINI_API_KEY".to_string(),
                ..Default::default()
            },
            VaultEntry {
                key: "DEEPSEEK_API_KEY".to_string(),
                ..Default::default()
            },
            VaultEntry {
                key: "MINIMAX_API_KEY".to_string(),
                ..Default::default()
            },
        ];

        Self {
            session_token,
            tab,
            skills_subtab: SkillsSubTab::Installed,
            agent_subtab: AgentSubTab::Editor,
            agent_task_subtab: AgentTaskSubTab::HighRisk,
            api_subtab: ApiSubTab::Cloud,
            system_subtab: SystemSubTab::General,
            enable_global_voice: true,
            enable_local_vision: false,
            local_vision_status: "Off".to_string(),
            gateway_url: url,
            client,
            skills_promise: None,
            skills: vec![],
            vault_entries,
            new_vault_key: String::new(),
            new_vault_value: String::new(),
            vault_show_value: false,
            connected: None,
            gateway_version: None,
            log_lines: vec![],
            expanded_skill: None,
            last_log_poll_time: -999.0, // force immediate poll on first open
            last_skill_refresh_time: -999.0,
            auto_log_poll: true,
            pending_log_promise: None,
            pending_runtime_config_promise: None,
            status_msg: None,
            cron_jobs: vec![],
            cron_loading: false,
            cron_error: None,
            last_cron_refresh_time: -999.0,
            pending_cron_promise: None,
            pending_cron_action_promise: None,
            cron_form_name: String::new(),
            cron_form_schedule: "every".to_string(),
            cron_form_interval: "3600".to_string(),
            cron_form_expr: "0 * * * *".to_string(),
            cron_form_prompt: String::new(),
            cron_form_role: "benshu".to_string(),
            cron_visual_mode: true,
            cron_visual_freq: "daily".to_string(),
            cron_visual_hour: 9,
            cron_visual_minute: 0,
            cron_visual_weekday: "Mon".to_string(),
            sessions: vec![],
            sessions_loading: false,
            sessions_error: None,
            last_sessions_refresh_time: -999.0,
            pending_sessions_promise: None,
            session_runtime_tasks: vec![],
            session_runtime_tasks_loading: false,
            session_runtime_tasks_error: None,
            session_runtime_tasks_session_id: None,
            last_session_runtime_tasks_refresh_time: -999.0,
            pending_session_runtime_tasks_promise: None,
            selected_task_output: None,
            selected_task_output_task_id: None,
            selected_task_output_loading: false,
            selected_task_output_error: None,
            pending_task_output_promise: None,
            selected_task_wait_notice: None,
            selected_task_wait_task_id: None,
            selected_task_wait_loading: false,
            selected_task_wait_error: None,
            pending_task_wait_promise: None,
            pending_task_cancel_promise: None,
            selected_session_delegation_trace: None,
            selected_session_delegation_session_id: None,
            selected_session_delegation_loading: false,
            selected_session_delegation_error: None,
            pending_session_delegation_promise: None,
            selected_run_trace: None,
            selected_run_trace_id: None,
            selected_run_trace_loading: false,
            selected_run_trace_error: None,
            pending_run_trace_promise: None,
            selected_run_replay: None,
            selected_run_replay_loading: false,
            selected_run_replay_error: None,
            pending_run_replay_promise: None,
            selected_profiler_artifact: None,
            selected_profiler_loading: false,
            selected_profiler_error: None,
            pending_profiler_promise: None,
            selected_profiler_query_results: Vec::new(),
            selected_profiler_query_loading: false,
            selected_profiler_query_error: None,
            pending_profiler_query_promise: None,
            selected_profiler_export: None,
            selected_profiler_export_loading: false,
            selected_profiler_export_error: None,
            pending_profiler_export_promise: None,
            selected_witness_summary: None,
            selected_witness_id: None,
            selected_witness_loading: false,
            selected_witness_error: None,
            pending_witness_promise: None,
            selected_witness_bundle: None,
            selected_witness_bundle_loading: false,
            selected_witness_bundle_error: None,
            pending_witness_bundle_promise: None,
            selected_witness_log: None,
            selected_witness_log_loading: false,
            selected_witness_log_error: None,
            pending_witness_log_promise: None,
            selected_witness_query_results: Vec::new(),
            selected_witness_query_loading: false,
            selected_witness_query_error: None,
            pending_witness_query_promise: None,
            selected_scorecard_query_results: Vec::new(),
            selected_scorecard_query_loading: false,
            selected_scorecard_query_error: None,
            pending_scorecard_query_promise: None,
            artifacts: vec![],
            artifacts_loading: false,
            artifacts_error: None,
            artifacts_query: ArtifactQuery {
                limit: Some(50),
                ..ArtifactQuery::default()
            },
            selected_artifact_id: None,
            pending_artifacts_promise: None,
            artifact_cleanup_policy: ArtifactCleanupPolicy {
                dry_run: true,
                ephemeral_max_age_hours: Some(24),
                session_max_age_hours: Some(24 * 7),
                durable_max_age_days: None,
                max_delete: Some(50),
                ..ArtifactCleanupPolicy::default()
            },
            artifact_cleanup_loading: false,
            artifact_cleanup_error: None,
            last_artifact_cleanup_report: None,
            pending_artifact_cleanup_promise: None,
            open_target_promise: None,
            runtime_mode_loading: false,
            runtime_mode_error: None,
            pending_runtime_mode_promise: None,
            local_model_stack: None,
            local_model_stack_loading: false,
            local_model_stack_error: None,
            pending_local_model_stack_promise: None,
            local_model_artifacts: None,
            local_model_artifacts_loading: false,
            local_model_artifacts_error: None,
            pending_local_model_artifacts_promise: None,
            knowledge_import_collection: "knowledge".to_string(),
            knowledge_import_loading: false,
            knowledge_import_error: None,
            last_knowledge_import_report: None,
            pending_knowledge_import_promise: None,
            knowledge_documents: Vec::new(),
            knowledge_documents_loading: false,
            knowledge_documents_error: None,
            pending_knowledge_documents_promise: None,
            pending_knowledge_delete_promise: None,
            novel_projects_root: String::new(),
            novel_projects: Vec::new(),
            novel_projects_loading: false,
            novel_projects_error: None,
            pending_novel_projects_promise: None,
            selected_novel_project_path: None,
            novel_export_format: "txt".to_string(),
            novel_export_approved_only: false,
            novel_export_loading: false,
            novel_export_error: None,
            last_novel_export: None,
            pending_novel_export_promise: None,
            deleted_vault_keys: std::collections::HashSet::new(),
            fact_check_enabled: saved.fact_check_enabled,
            image_gen_model: String::new(),
            image_gen_status: "Unconfigured".to_string(),
            tactical_model: String::new(),
            windows_ml_runtime_family: "windows_ml_onnx_runtime".to_string(),
            windows_ml_execution_provider_preference: "directml".to_string(),
            windows_ml_device_target: "auto".to_string(),
            windows_ml_cpu_fallback_policy: "allow".to_string(),
            windows_ml_graph_optimization_level: "all".to_string(),
            windows_ml_intra_threads: String::new(),
            windows_ml_inter_threads: String::new(),
            windows_ml_text_batch_size: 8,
            windows_ml_text_max_sequence_length: 1024,
            windows_ml_vision_max_image_side: 1024,
            windows_ml_vision_resize_policy: "fit".to_string(),
            windows_ml_audio_sample_rate_hz: 16_000,
            windows_ml_audio_chunk_ms: 30_000,
            windows_ml_image_width: 1024,
            windows_ml_image_height: 1024,
            windows_ml_image_steps: 20,
            windows_ml_image_guidance: "7.5".to_string(),
            windows_ml_realtime_vad_window_ms: 30,
            windows_ml_duplex_frame_ms: 20,
            windows_ml_safety_threshold: "0.5".to_string(),
            llama_tuning_mode: "auto".to_string(),
            llama_performance_profile: "balanced".to_string(),
            llama_runtime_diagnostics: String::new(),
            llama_ctx_size: 8192,
            llama_gpu_layers: 24,
            llama_threads: default_llama_runtime_threads(),
            llama_threads_batch: String::new(),
            llama_batch_size: 2048,
            llama_ubatch_size: 512,
            llama_parallel_slots: 1,
            llama_cache_ram: "256".to_string(),
            llama_ctx_checkpoints: "0".to_string(),
            llama_flash_attn_mode: "auto".to_string(),
            llama_kv_offload: true,
            llama_mmap: true,
            llama_mlock: false,
            llama_cache_prompt: false,
            llama_cont_batching: false,
            llama_warmup: true,
            llama_context_shift: false,
            llama_jinja: true,
            llama_rope_scaling: String::new(),
            llama_rope_scale: String::new(),
            llama_rope_freq_base: String::new(),
            llama_rope_freq_scale: String::new(),
            llama_yarn_orig_ctx: String::new(),
            llama_yarn_ext_factor: String::new(),
            llama_yarn_attn_factor: String::new(),
            llama_yarn_beta_slow: String::new(),
            llama_yarn_beta_fast: String::new(),
            llama_cache_type_k: String::new(),
            llama_cache_type_v: String::new(),
            llama_device: String::new(),
            llama_split_mode: String::new(),
            llama_tensor_split: String::new(),
            llama_main_gpu: String::new(),
            llama_fit_mode: "on".to_string(),
            llama_fit_target: String::new(),
            llama_fit_ctx: String::new(),
            llama_cpu_moe: false,
            llama_n_cpu_moe: String::new(),
            llama_mmproj_offload: true,
            llama_image_min_tokens: String::new(),
            llama_image_max_tokens: String::new(),
            llama_reasoning_mode: "auto".to_string(),
            llama_reasoning_format: "auto".to_string(),
            llama_reasoning_budget: String::new(),
            llama_reasoning_budget_message: String::new(),
            llama_sampling_temperature: "0.8".to_string(),
            llama_sampling_top_k: "40".to_string(),
            llama_sampling_top_p: "0.95".to_string(),
            llama_sampling_min_p: "0.05".to_string(),
            llama_sampling_typical_p: "1.0".to_string(),
            llama_sampling_repeat_penalty: "1.0".to_string(),
            llama_sampling_presence_penalty: "0.0".to_string(),
            llama_sampling_frequency_penalty: "0.0".to_string(),
            llama_sampling_mirostat: "0".to_string(),
            llama_sampling_mirostat_eta: "0.1".to_string(),
            llama_sampling_mirostat_tau: "5.0".to_string(),
            llama_seed: String::new(),
            voice_tts_model: saved.voice_tts_model,
            voice_tts_voice: saved.voice_tts_voice,
            auto_consolidation_enabled: true,
            model_ram_limit_gb: saved.model_ram_limit_gb,
            model_vram_limit_gb: saved.model_vram_limit_gb,
            store_install_url: String::new(),
            store_installing: false,
            store_install_error: None,
            store_install_success: None,
            pending_install_promise: None,
            channels: vec![],
            channels_loading: false,
            channels_error: None,
            pending_channels_promise: None,
            provider_metadata: Vec::new(),
            provider_loading: false,
            provider_error: None,
            pending_provider_promise: None,
            agent_save_promise: None,
            agent_export_promise: None,
            agent_export_json: None,
            agent_export_save_path: None,
            agent_import_json: String::new(),
            agent_show_import_window: false,
            agent_show_export_window: false,
            agent_export_loading: false,
            agent_import_promise: None,
            agent_role_selected: "benshu".to_string(),
            is_adding_agent: false,
            is_editing_identity: false,
            custom_added_agents: std::collections::BTreeSet::new(),
            agent_role_content: String::new(),
            agent_role_dirty: false,
            agent_role_artifact_policy_dirty: false,
            agent_role_promise: None,
            agent_role_loaded: false,
            agent_role_provider: String::new(),
            agent_role_base_url: String::new(),
            agent_role_model: String::new(),
            agent_role_local_model_artifact: String::new(),
            agent_role_local_mmproj_artifact: String::new(),
            agent_role_local_runtime_family: String::new(),
            agent_role_temperature: "0.7".to_string(),
            agent_role_auto_consolidation: true,
            agent_role_tools: vec![],
            agent_role_pending_tool: String::new(),
            agent_role_artifact_policy_yaml: String::new(),
            agent_role_artifact_policy_error: None,
            agent_ocean_openness: 5.0,
            agent_ocean_conscientiousness: 10.0,
            agent_ocean_extraversion: 5.0,
            agent_ocean_agreeableness: 8.0,
            agent_ocean_neuroticism: 2.0,
            agent_list_promise: None,
            agent_list: vec![crate::api::AgentSummary {
                id: "benshu".to_string(),
                name: Some("BenShu".to_string()),
            }],
            agent_templates: vec![],
            agent_templates_promise: None,
            last_agent_list_refresh_time: -999.0,
            agent_role_name: String::new(),
            agent_role_description: String::new(),
            agent_role_tone: String::new(),
            agent_role_constraints: vec![],
            agent_role_backstory: String::new(),
            chat_histories: std::collections::HashMap::new(),
            chat_sessions: saved.chat_sessions,
            active_chat_session: {
                let mut m = std::collections::HashMap::new();
                m.insert("benshu".to_string(), "default".to_string());
                m
            },
            chat_input: String::new(),
            chat_attachments: Vec::new(),
            chat_selected_role: "benshu".to_string(),
            chat_loading: false,
            chat_promise: None,
            chat_history_promise: None,
            pending_chat_task_output_promise: None,
            pending_chat_task_output_task_id: None,
            pending_chat_task_output_session_id: None,
            chat_task_output_appended: std::collections::HashSet::new(),
            cancel_promise: None,
            pending_delete_agent: None,
            pending_delete_session: None,
            doctor_loading: false,
            doctor_error: None,
            doctor_results: None,
            pending_doctor_promise: None,
            repair_loading: false,
            pending_repair_promise: None,
            show_exit_dialog: false,
            exit_in_progress: false,
            metrics_loading: false,
            metrics_error: None,
            last_metrics: None,
            pending_metrics_promise: None,
            last_metrics_refresh_time: 0.0,
            metrics_history: Vec::new(),

            channel_metadata: Vec::new(),
            running_channels: Vec::new(),
            channel_observability: std::collections::HashMap::new(),
            channel_metadata_promise: None,

            sandboxes: Vec::new(),
            sandboxes_promise: None,
            last_sandboxes_refresh_time: 0.0,

            approvals: Vec::new(),
            approval_receipts: Vec::new(),
            approval_error: None,
            pending_approval_promise: None,
            pending_approval_receipts_promise: None,
            last_approval_refresh_time: 0.0,
            approval_resolve_promise: None,
            approval_receipt_error: None,

            restore_points: Vec::new(),
            selected_restore_backup_id: None,
            selected_restore_dry_run: None,
            selected_restore_policy_basis: None,
            selected_restore_receipts: Vec::new(),
            selected_restore_delete_report: None,
            restore_points_error: None,
            restore_points_loading: false,
            pending_restore_create_promise: None,
            pending_restore_points_promise: None,
            pending_restore_dry_run_promise: None,
            pending_restore_policy_promise: None,
            pending_restore_receipts_promise: None,
            pending_restore_execute_promise: None,
            pending_restore_delete_promise: None,

            rollback_promise: None,

            organ_stt_model: saved.organ_stt_model,
            organ_tts_model: saved.organ_tts_model,
            organ_embed_model: saved.organ_embed_model,
            organ_rerank_model: saved.organ_rerank_model,
            organ_ocr_model: saved.organ_ocr_model,
            organ_vision_model: saved.organ_vision_model,
            organ_fact_check_model: saved.organ_fact_check_model,
            organ_image_edit_model: saved.organ_image_edit_model,
            organ_audio_understanding_model: saved.organ_audio_understanding_model,
            organ_realtime_vad_model: saved.organ_realtime_vad_model,
            organ_duplex_voice_model: saved.organ_duplex_voice_model,
            organ_local_classifier_model: saved.organ_local_classifier_model,
            organ_local_router_model: saved.organ_local_router_model,
            organ_local_safety_model: saved.organ_local_safety_model,
            use_local_ocr: Some(true),

            night_mode,
            language,
            last_ui_scale: 0.0,
            a2a_agents: vec![],
            a2a_board: std::collections::HashMap::new(),
            a2a_loading: false,
            a2a_error: None,
            a2a_throttle_tenant: String::new(),
            a2a_throttle_role: String::new(),
            a2a_throttle_limit: 10,
            a2a_throttle_promise: None,
            last_a2a_refresh_time: 0.0,
            pending_a2a_promise: None,

            initial_resize_done: false,

            update_in_progress: false,
            update_status: None,
            update_promise: None,

            trusted_workspaces: vec![],
            workspace_loading: false,
            last_workspace_refresh_time: -999.0,
            pending_workspace_promise: None,
            workspace_form_path: String::new(),
        }
    }

    pub fn set_url(&mut self, url: String) {
        save_url(&url);
        let mut new_client = GatewayClient::new(url.clone());
        if let Some(t) = &self.session_token {
            new_client = new_client.with_token(t.clone());
        }
        self.client = new_client;
        self.gateway_url = url;
        self.connected = None;
        self.skills = vec![];
        self.skills_promise = None;
    }

    pub fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status_msg = Some((msg.into(), is_error));
    }

    // ── Asynchronous Scheduling Logic (Decoupled from UI) ─────────────────────

    pub fn trigger_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let ctx_clone = ctx.clone();

        // Update the timestamp to prevent the background poll from firing immediately after
        self.last_skill_refresh_time = ctx.input(|i| i.time);

        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let result = match client.list_skills().await {
                Ok(skills) => {
                    ctx_clone.request_repaint();
                    Ok(skills)
                }
                Err(e) => {
                    ctx_clone.request_repaint();
                    Err(e.to_string())
                }
            };
            sender.send(result);
        });

        self.skills_promise = Some(promise);

        self.do_cron_refresh(rt, ctx);
        self.do_sessions_refresh(rt, ctx);
        self.do_sandboxes_refresh(rt, ctx);
        self.do_provider_refresh(rt, ctx);
        self.do_agent_templates_refresh(rt, ctx);
        self.do_agent_refresh(rt, ctx);
        self.do_channel_refresh(rt, ctx);
        self.do_artifact_refresh(rt, ctx);
        self.do_runtime_mode_refresh(rt, ctx);
        self.do_local_model_stack_refresh(rt, ctx);
        self.do_local_model_artifacts_refresh(rt, ctx);

        // Health check (Silent)
        let hc_client = self.client.clone();
        let rt_hc = rt.clone();
        rt_hc.spawn(async move {
            let _ = hc_client.health().await;
        });

        self.connected = None;
    }

    pub fn do_runtime_mode_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_runtime_mode_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        self.runtime_mode_loading = true;
        self.runtime_mode_error = None;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.get_runtime_mode().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_runtime_mode_promise = Some(promise);
    }

    pub fn do_local_model_stack_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_local_model_stack_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        self.local_model_stack_loading = true;
        self.local_model_stack_error = None;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_local_model_stack()
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_local_model_stack_promise = Some(promise);
    }

    pub fn do_local_model_artifacts_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_local_model_artifacts_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        self.local_model_artifacts_loading = true;
        self.local_model_artifacts_error = None;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_local_model_artifacts()
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_local_model_artifacts_promise = Some(promise);
    }

    pub fn do_knowledge_import(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        folder: Option<String>,
        files: Vec<String>,
    ) {
        if self.pending_knowledge_import_promise.is_some() {
            return;
        }

        let collection = self.knowledge_import_collection.trim().to_string();
        if collection.is_empty() {
            self.set_status("Knowledge collection cannot be empty.", true);
            return;
        }

        let request = crate::api::KnowledgeImportRequest {
            collection,
            folder,
            files,
        };

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        self.knowledge_import_loading = true;
        self.knowledge_import_error = None;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .import_knowledge(&request)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_knowledge_import_promise = Some(promise);
    }

    pub fn do_knowledge_documents_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_knowledge_documents_promise.is_some() {
            return;
        }

        let collection = self.knowledge_import_collection.trim().to_string();
        if collection.is_empty() {
            self.set_status("Knowledge collection cannot be empty.", true);
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        self.knowledge_documents_loading = true;
        self.knowledge_documents_error = None;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .list_knowledge_documents(&collection)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_knowledge_documents_promise = Some(promise);
    }

    pub fn do_knowledge_document_delete(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        collection: String,
        path: String,
    ) {
        if self.pending_knowledge_delete_promise.is_some() {
            return;
        }
        let request = crate::api::KnowledgeDeleteRequest { collection, path };
        let client = self.client.clone();
        let ctx2 = ctx.clone();

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .delete_knowledge_document(&request)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_knowledge_delete_promise = Some(promise);
    }

    pub fn do_novel_projects_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_novel_projects_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        self.novel_projects_loading = true;
        self.novel_projects_error = None;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .list_novel_projects()
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_novel_projects_promise = Some(promise);
    }

    pub fn do_novel_export(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        project_path: String,
        format: String,
        approved_only: bool,
    ) {
        if self.pending_novel_export_promise.is_some() {
            return;
        }

        let request = crate::api::NovelExportRequest {
            project_path,
            format,
            approved_only,
        };
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        self.novel_export_loading = true;
        self.novel_export_error = None;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .export_novel_project(&request)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_novel_export_promise = Some(promise);
    }

    pub fn poll_install_promise(&mut self, rt: &Handle, ctx: &egui::Context) {
        if let Some(ref p) = self.pending_install_promise {
            if let Some(result) = p.ready() {
                self.store_installing = false;
                match result {
                    Ok(resp) => {
                        self.store_install_success =
                            Some(format!("Installed: {}", resp.skill_name));
                        self.store_install_url.clear();
                        self.trigger_refresh(rt, ctx);
                    }
                    Err(e) => {
                        self.store_install_error = Some(e.clone());
                    }
                }
                self.pending_install_promise = None;
            }
        }
    }

    pub fn poll_agent_import_promise(&mut self, rt: &Handle, ctx: &egui::Context) {
        let mut import_res = None;
        if let Some(ref mut p) = self.agent_import_promise {
            if let Some(res) = p.ready_mut() {
                import_res = Some(res.clone());
            }
        }
        if let Some(res) = import_res {
            self.agent_import_promise = None;
            match res {
                Ok(_) => {
                    self.set_status(crate::i18n::t("agent.import_success", self.language), false);
                    self.agent_show_import_window = false;
                    self.do_agent_refresh(rt, ctx);
                }
                Err(e) => {
                    self.set_status(
                        format!(
                            "{}: {}",
                            crate::i18n::t("agent.import_failed", self.language),
                            e
                        ),
                        true,
                    );
                }
            }
        }
    }

    pub fn poll_agent_export_promise(&mut self) {
        let mut export_res = None;
        if let Some(ref mut p) = self.agent_export_promise {
            if let Some(res) = p.ready_mut() {
                export_res = Some(res.clone());
            }
        }
        if let Some(res) = export_res {
            self.agent_export_promise = None;
            match res {
                Ok(json) => {
                    self.agent_export_json = Some(json);
                    self.set_status(crate::i18n::t("agent.export_success", self.language), false);
                }
                Err(e) => {
                    self.agent_export_loading = false;
                    self.agent_export_promise = None;
                    self.set_status(
                        format!(
                            "{}: {}",
                            crate::i18n::t("agent.export_failed", self.language),
                            e
                        ),
                        true,
                    );
                }
            }
        }
    }

    pub fn poll_agent_promises(&mut self, rt: &Handle, ctx: &egui::Context) {
        let mut agents_res = None;
        if let Some(ref mut p) = self.agent_list_promise {
            if let Some(res) = p.ready_mut() {
                agents_res = Some(res.clone());
            }
        }
        if let Some(res) = agents_res {
            self.agent_list_promise = None;
            match res {
                Ok(agents_ref) => {
                    let mut agents = agents_ref.clone();
                    if !agents.iter().any(|a| a.id == "benshu") {
                        agents.push(crate::api::AgentSummary {
                            id: "benshu".to_string(),
                            name: Some("BenShu".to_string()),
                        });
                    }
                    agents.sort_by(|a, b| {
                        if a.id == "benshu" {
                            return std::cmp::Ordering::Less;
                        }
                        if b.id == "benshu" {
                            return std::cmp::Ordering::Greater;
                        }
                        a.id.cmp(&b.id)
                    });
                    self.agent_list = agents;
                }
                Err(e) => {
                    self.set_status(format!("Failed to list agents: {}", e), true);
                }
            }
        }

        let mut templates_res = None;
        if let Some(ref mut p) = self.agent_templates_promise {
            if let Some(res) = p.ready_mut() {
                templates_res = Some(res.clone());
            }
        }
        if let Some(res) = templates_res {
            self.agent_templates_promise = None;
            match res {
                Ok(templates) => {
                    self.agent_templates = templates;
                    self.do_agent_refresh(rt, ctx);
                }
                Err(e) => {
                    self.set_status(format!("Failed to load agent templates: {}", e), true);
                }
            }
        }

        let mut save_res = None;
        if let Some(ref mut p) = self.agent_save_promise {
            if let Some(res) = p.ready_mut() {
                save_res = Some(res.clone());
            }
        }
        if let Some(res) = save_res {
            self.agent_save_promise = None;
            match res {
                Ok(_) => {
                    self.set_status("Saved successfully.".to_string(), false);
                    self.do_agent_refresh(rt, ctx);
                    // trigger_load_agent logic would go here, maybe handled by UI check
                }
                Err(e) => {
                    self.set_status(format!("Failed to save: {}", e), true);
                }
            }
        }

        self.poll_agent_role_promise(ctx);
        self.poll_agent_import_promise(rt, ctx);
        self.poll_agent_export_promise();
    }

    pub fn poll_a2a_promise(&mut self) {
        if let Some(promise) = self.pending_a2a_promise.as_mut() {
            if let Some(result) = promise.ready() {
                self.a2a_loading = false;
                match result {
                    Ok(summary) => {
                        self.a2a_agents = summary.agents.clone();
                        self.a2a_board = summary.board.clone();
                        self.a2a_error = None;
                    }
                    Err(e) => {
                        self.a2a_error = Some(e.clone());
                    }
                }
                self.pending_a2a_promise = None;
            }
        }
    }

    pub fn do_doctor_run(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let ctx_clone = ctx.clone();
        self.doctor_loading = true;
        self.doctor_error = None;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.doctor_check().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx_clone.request_repaint();
        });
        self.pending_doctor_promise = Some(promise);
    }

    pub fn do_repair(&mut self, rt: &Handle, ctx: &egui::Context, name: &str) {
        let client = self.client.clone();
        let ctx_clone = ctx.clone();
        let name_str = name.to_string();
        self.repair_loading = true;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .repair_system(&name_str)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx_clone.request_repaint();
        });
        self.pending_repair_promise = Some(promise);
    }

    pub fn do_rollback(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        original: String,
        backup: String,
    ) {
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .rollback(original, backup)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.rollback_promise = Some(promise);
        self.set_status("Rolling back changes...", false);
    }

    pub fn do_save_model_budgets(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let vram = self.model_vram_limit_gb;
        let ram = self.model_ram_limit_gb;
        let embed = self.organ_embed_model.clone();
        let rerank = self.organ_rerank_model.clone();
        let ctx2 = ctx.clone();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            if let Ok(mut config) = client.get_config().await {
                if let Some(knowledge) = config.get_mut("knowledge") {
                    knowledge["model_vram_limit_gb"] = serde_json::json!(vram);
                    knowledge["model_ram_limit_gb"] = serde_json::json!(ram);
                    knowledge["embed_model"] = serde_json::json!(embed);
                    knowledge["rerank_model"] = serde_json::json!(rerank);
                    let _ = client.update_config(&config).await;
                    ctx2.request_repaint();
                }
            }
        });
    }

    pub fn do_toggle_consolidation(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let enabled = self.auto_consolidation_enabled;
        let ctx2 = ctx.clone();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            if let Ok(mut config) = client.get_config().await {
                if let Some(knowledge) = config.get_mut("knowledge") {
                    knowledge["auto_consolidation_enabled"] = serde_json::json!(enabled);
                    let _ = client.update_config(&config).await;
                    ctx2.request_repaint();
                }
            }
        });
    }

    pub fn do_save_sensory_settings(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();

        // Capture all relevant state
        let voice = self.enable_global_voice;
        let vision = false;
        let stt = self.organ_stt_model.clone();
        let tts = self.organ_tts_model.clone();
        let ocr = self.organ_ocr_model.clone();
        let vision_model = self.organ_vision_model.clone();
        let image_edit = self.organ_image_edit_model.clone();
        let audio_understanding = self.organ_audio_understanding_model.clone();
        let realtime_vad = self.organ_realtime_vad_model.clone();
        let duplex_voice = self.organ_duplex_voice_model.clone();
        let local_classifier = self.organ_local_classifier_model.clone();
        let local_router = self.organ_local_router_model.clone();
        let local_safety = self.organ_local_safety_model.clone();
        let embed = self.organ_embed_model.clone();
        let rerank = self.organ_rerank_model.clone();
        let fact = self.fact_check_enabled;
        let fact_model = self.organ_fact_check_model.clone();
        let tactical = self.tactical_model.clone();
        let image_gen = self.image_gen_model.clone();
        let auto_con = self.auto_consolidation_enabled;
        let ram_limit = self.model_ram_limit_gb;
        let vram_limit = self.model_vram_limit_gb;

        let ctx2 = ctx.clone();
        let rt_handle = rt.clone();

        rt_handle.spawn(async move {
            if let Ok(mut config) = client.get_config().await {
                // Ensure sensory block exists
                if config.get("sensory").is_none() {
                    config["sensory"] = serde_json::json!({});
                }
                if let Some(sensory) = config.get_mut("sensory") {
                    sensory["enable_global_voice"] = serde_json::json!(voice);
                    sensory["enable_local_vision"] = serde_json::json!(vision);
                    sensory["stt_model"] = serde_json::json!(stt);
                    sensory["tts_model"] = serde_json::json!(tts);
                    sensory["ocr_model"] = serde_json::json!(ocr);
                    sensory["vision_model"] = serde_json::json!(vision_model);
                    sensory["image_edit_model"] = serde_json::json!(image_edit);
                    sensory["audio_understanding_model"] = serde_json::json!(audio_understanding);
                    sensory["realtime_vad_model"] = serde_json::json!(realtime_vad);
                    sensory["duplex_voice_model"] = serde_json::json!(duplex_voice);
                    sensory["local_classifier_model"] = serde_json::json!(local_classifier);
                    sensory["local_router_model"] = serde_json::json!(local_router);
                    sensory["local_safety_model"] = serde_json::json!(local_safety);
                    sensory["tactical_model"] = serde_json::json!(tactical);
                    sensory["image_gen_model"] = serde_json::json!(image_gen);
                    sensory["fact_check_enabled"] = serde_json::json!(fact);
                    sensory["fact_check_model"] = serde_json::json!(fact_model);
                }

                // Ensure knowledge block exists
                if config.get("knowledge").is_none() {
                    config["knowledge"] = serde_json::json!({});
                }
                if let Some(knowledge) = config.get_mut("knowledge") {
                    knowledge["embed_model"] = serde_json::json!(embed);
                    knowledge["rerank_model"] = serde_json::json!(rerank);
                    knowledge["auto_consolidation_enabled"] = serde_json::json!(auto_con);
                    knowledge["model_ram_limit_gb"] = serde_json::json!(ram_limit);
                    knowledge["model_vram_limit_gb"] = serde_json::json!(vram_limit);
                }

                if let Err(e) = client.update_config(&config).await {
                    tracing::error!("Failed to deliver config to gateway: {}", e);
                }
                ctx2.request_repaint();
            }
        });
    }

    pub fn do_save_llama_cpp_runtime_settings(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let tuning_mode = self.llama_tuning_mode.clone();
        let performance_profile = self.llama_performance_profile.clone();
        let ctx_size = self.llama_ctx_size;
        let gpu_layers = self.llama_gpu_layers;
        let threads = self.llama_threads;
        let threads_batch = self.llama_threads_batch.trim().parse::<i32>().ok();
        let batch_size = self.llama_batch_size;
        let ubatch_size = self.llama_ubatch_size;
        let parallel_slots = self.llama_parallel_slots;
        let cache_ram = self.llama_cache_ram.trim().parse::<u32>().ok();
        let ctx_checkpoints = self.llama_ctx_checkpoints.trim().parse::<u32>().ok();
        let flash_attn_mode = self.llama_flash_attn_mode.clone();
        let kv_offload = self.llama_kv_offload;
        let mmap = self.llama_mmap;
        let mlock = self.llama_mlock;
        let cache_prompt = self.llama_cache_prompt;
        let cont_batching = self.llama_cont_batching;
        let warmup = self.llama_warmup;
        let context_shift = self.llama_context_shift;
        let jinja = self.llama_jinja;
        let rope_scaling = parse_optional_string(&self.llama_rope_scaling);
        let rope_scale = self.llama_rope_scale.trim().parse::<f32>().ok();
        let rope_freq_base = self.llama_rope_freq_base.trim().parse::<f32>().ok();
        let rope_freq_scale = self.llama_rope_freq_scale.trim().parse::<f32>().ok();
        let yarn_orig_ctx = self.llama_yarn_orig_ctx.trim().parse::<u32>().ok();
        let yarn_ext_factor = self.llama_yarn_ext_factor.trim().parse::<f32>().ok();
        let yarn_attn_factor = self.llama_yarn_attn_factor.trim().parse::<f32>().ok();
        let yarn_beta_slow = self.llama_yarn_beta_slow.trim().parse::<f32>().ok();
        let yarn_beta_fast = self.llama_yarn_beta_fast.trim().parse::<f32>().ok();
        let cache_type_k = parse_optional_string(&self.llama_cache_type_k);
        let cache_type_v = parse_optional_string(&self.llama_cache_type_v);
        let device = parse_optional_string(&self.llama_device);
        let split_mode = parse_optional_string(&self.llama_split_mode);
        let tensor_split = parse_optional_string(&self.llama_tensor_split);
        let main_gpu = self.llama_main_gpu.trim().parse::<u32>().ok();
        let fit_mode = self.llama_fit_mode.clone();
        let fit_target = parse_optional_string(&self.llama_fit_target);
        let fit_ctx = self.llama_fit_ctx.trim().parse::<u32>().ok();
        let cpu_moe = self.llama_cpu_moe;
        let n_cpu_moe = self.llama_n_cpu_moe.trim().parse::<u32>().ok();
        let mmproj_offload = self.llama_mmproj_offload;
        let image_min_tokens = self.llama_image_min_tokens.trim().parse::<u32>().ok();
        let image_max_tokens = self.llama_image_max_tokens.trim().parse::<u32>().ok();
        let reasoning_mode = self.llama_reasoning_mode.clone();
        let reasoning_format = self.llama_reasoning_format.clone();
        let reasoning_budget = self.llama_reasoning_budget.trim().parse::<i32>().ok();
        let reasoning_budget_message = parse_optional_string(&self.llama_reasoning_budget_message);
        let sampling_temperature = self
            .llama_sampling_temperature
            .trim()
            .parse::<f32>()
            .unwrap_or(0.8);
        let sampling_top_k = self
            .llama_sampling_top_k
            .trim()
            .parse::<i32>()
            .unwrap_or(40);
        let sampling_top_p = self
            .llama_sampling_top_p
            .trim()
            .parse::<f32>()
            .unwrap_or(0.95);
        let sampling_min_p = self
            .llama_sampling_min_p
            .trim()
            .parse::<f32>()
            .unwrap_or(0.05);
        let sampling_typical_p = self
            .llama_sampling_typical_p
            .trim()
            .parse::<f32>()
            .unwrap_or(1.0);
        let sampling_repeat_penalty = self
            .llama_sampling_repeat_penalty
            .trim()
            .parse::<f32>()
            .unwrap_or(1.0);
        let sampling_presence_penalty = self
            .llama_sampling_presence_penalty
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0);
        let sampling_frequency_penalty = self
            .llama_sampling_frequency_penalty
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0);
        let sampling_mirostat = self
            .llama_sampling_mirostat
            .trim()
            .parse::<i32>()
            .unwrap_or(0);
        let sampling_mirostat_eta = self
            .llama_sampling_mirostat_eta
            .trim()
            .parse::<f32>()
            .unwrap_or(0.1);
        let sampling_mirostat_tau = self
            .llama_sampling_mirostat_tau
            .trim()
            .parse::<f32>()
            .unwrap_or(5.0);
        let seed = self.llama_seed.trim().parse::<i64>().ok();

        self.set_status(
            "保存 Llama.cpp Runtime 参数中，随后会自动重启正式运行时。",
            false,
        );
        let (sender, promise) = Promise::new();
        self.pending_runtime_config_promise = Some(promise);
        let ctx2 = ctx.clone();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let result = async {
                let mut config = client.get_config().await.map_err(|e| e.to_string())?;
                let runtime = crate::api::LlamaCppRuntime {
                    tuning_mode,
                    performance_profile,
                    last_recommendation: None,
                    effective_diagnostics: None,
                    ctx_size,
                    gpu_layers,
                    threads,
                    threads_batch,
                    batch_size,
                    ubatch_size,
                    parallel_slots,
                    cache_ram,
                    ctx_checkpoints,
                    flash_attn_mode,
                    kv_offload,
                    mmap,
                    mlock,
                    cache_prompt,
                    cont_batching,
                    warmup,
                    context_shift,
                    jinja,
                    rope_scaling,
                    rope_scale,
                    rope_freq_base,
                    rope_freq_scale,
                    yarn_orig_ctx,
                    yarn_ext_factor,
                    yarn_attn_factor,
                    yarn_beta_slow,
                    yarn_beta_fast,
                    cache_type_k,
                    cache_type_v,
                    device,
                    split_mode,
                    tensor_split,
                    main_gpu,
                    fit_mode,
                    fit_target,
                    fit_ctx,
                    cpu_moe,
                    n_cpu_moe,
                    mmproj_offload,
                    image_min_tokens,
                    image_max_tokens,
                    reasoning_mode,
                    reasoning_format,
                    reasoning_budget,
                    reasoning_budget_message,
                    sampling_temperature,
                    sampling_top_k,
                    sampling_top_p,
                    sampling_min_p,
                    sampling_typical_p,
                    sampling_repeat_penalty,
                    sampling_presence_penalty,
                    sampling_frequency_penalty,
                    sampling_mirostat,
                    sampling_mirostat_eta,
                    sampling_mirostat_tau,
                    seed,
                };
                config["llama_cpp_runtime"] =
                    serde_json::to_value(runtime).map_err(|e| e.to_string())?;
                client
                    .update_config(&config)
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            sender.send(result);
            ctx2.request_repaint();
        });
    }

    pub fn do_save_windows_ml_runtime_settings(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let runtime_family = self.windows_ml_runtime_family.clone();
        let execution_provider_preference = self.windows_ml_execution_provider_preference.clone();
        let device_target = self.windows_ml_device_target.clone();
        let cpu_fallback_policy = self.windows_ml_cpu_fallback_policy.clone();
        let graph_optimization_level = self.windows_ml_graph_optimization_level.clone();
        let intra_threads = self.windows_ml_intra_threads.trim().parse::<u32>().ok();
        let inter_threads = self.windows_ml_inter_threads.trim().parse::<u32>().ok();
        let text_batch_size = self.windows_ml_text_batch_size;
        let text_max_sequence_length = self.windows_ml_text_max_sequence_length;
        let vision_max_image_side = self.windows_ml_vision_max_image_side;
        let vision_resize_policy = self.windows_ml_vision_resize_policy.clone();
        let audio_sample_rate_hz = self.windows_ml_audio_sample_rate_hz;
        let audio_chunk_ms = self.windows_ml_audio_chunk_ms;
        let image_width = self.windows_ml_image_width;
        let image_height = self.windows_ml_image_height;
        let image_steps = self.windows_ml_image_steps;
        let image_guidance = self.windows_ml_image_guidance.trim().parse::<f32>().ok();
        let realtime_vad_window_ms = self.windows_ml_realtime_vad_window_ms;
        let duplex_frame_ms = self.windows_ml_duplex_frame_ms;
        let safety_threshold = self.windows_ml_safety_threshold.trim().parse::<f32>().ok();

        self.set_status(
            "保存 Windows ML / ONNX Runtime 参数中，随后会自动重启正式运行时。",
            false,
        );
        let (sender, promise) = Promise::new();
        self.pending_runtime_config_promise = Some(promise);
        let ctx2 = ctx.clone();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let result = async {
                let mut config = client.get_config().await.map_err(|e| e.to_string())?;
                config["windows_ml_runtime"] = serde_json::json!({
                    "runtime_family": runtime_family,
                    "execution_provider_preference": execution_provider_preference,
                    "device_target": device_target,
                    "cpu_fallback_policy": cpu_fallback_policy,
                    "graph_optimization_level": graph_optimization_level,
                    "intra_threads": intra_threads,
                    "inter_threads": inter_threads,
                    "text_profile": {
                        "batch_size": text_batch_size,
                        "max_sequence_length": text_max_sequence_length
                    },
                    "vision_profile": {
                        "max_image_side": vision_max_image_side,
                        "resize_policy": vision_resize_policy
                    },
                    "audio_profile": {
                        "sample_rate_hz": audio_sample_rate_hz,
                        "chunk_ms": audio_chunk_ms
                    },
                    "image_profile": {
                        "width": image_width,
                        "height": image_height,
                        "steps": image_steps,
                        "guidance": image_guidance.unwrap_or(7.5)
                    },
                    "realtime_profile": {
                        "vad_window_ms": realtime_vad_window_ms,
                        "duplex_frame_ms": duplex_frame_ms
                    },
                    "safety_profile": {
                        "threshold": safety_threshold.unwrap_or(0.5)
                    }
                });

                client
                    .update_config(&config)
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            sender.send(result);
            ctx2.request_repaint();
        });
    }

    pub fn poll_runtime_config_promise(&mut self) {
        let status_update = if let Some(promise) = self.pending_runtime_config_promise.as_mut() {
            promise.ready_mut().map(|result| match result {
                Ok(update) => {
                    let mut restarted = Vec::new();
                    let mut pending_manual = Vec::new();
                    if update.main_brain_restart_requested {
                        restarted.push("Main Brain");
                    } else if update.main_brain_restart_needed {
                        pending_manual.push("Main Brain");
                    }
                    if update.windows_ml_restart_requested {
                        restarted.push("Windows ML");
                    } else if update.windows_ml_restart_needed {
                        pending_manual.push("Windows ML");
                    }
                    if !restarted.is_empty() && pending_manual.is_empty() {
                        (
                            format!(
                                "参数已保存，{} 运行时重启请求已发出，请稍候等待重新就绪。",
                                restarted.join(" + ")
                            ),
                            false,
                        )
                    } else if restarted.is_empty() && !pending_manual.is_empty() {
                        (
                            format!(
                                "参数已保存，但 {} 运行时当前还没有可用的托管重启入口；正式 Windows 原生部署里需要补齐宿主管理后才能自动重启。",
                                pending_manual.join(" + ")
                            ),
                            true,
                        )
                    } else if !restarted.is_empty() && !pending_manual.is_empty() {
                        (
                            format!(
                                "参数已保存。{} 已发出自动重启请求；{} 仍缺少可用的托管重启入口。",
                                restarted.join(" + "),
                                pending_manual.join(" + ")
                            ),
                            true,
                        )
                    } else {
                        (
                            "参数已保存，本次没有需要重启的运行时。".to_string(),
                            false,
                        )
                    }
                }
                Err(error) => (format!("保存失败：{error}"), true),
            })
        } else {
            None
        };

        if let Some((message, is_error)) = status_update {
            self.pending_runtime_config_promise = None;
            self.set_status(message, is_error);
        }
    }

    pub fn poll_chat_history_promise(&mut self) {
        if let Some(ref mut p) = self.chat_history_promise {
            if let Some(res) = p.ready_mut() {
                match res {
                    Ok(history) => {
                        let current_role = self.chat_selected_role.clone();
                        let current_session = self
                            .active_chat_session
                            .get(&current_role)
                            .cloned()
                            .unwrap_or_else(|| "default".to_string());
                        let key = format!("{}:{}", current_session, current_role);
                        self.chat_histories.insert(
                            key,
                            history
                                .iter()
                                .cloned()
                                .map(compact_chat_message_for_panel)
                                .collect(),
                        );
                    }
                    Err(e) => {
                        let msg = format!("History load failed: {}", e);
                        self.set_status(msg, true);
                    }
                }
                self.chat_history_promise = None;
            }
        }
    }

    fn push_chat_history_message(&mut self, session_id: &str, role: &str, message: ChatMessage) {
        let history_key = format!("{}:{}", session_id, role);
        let history = self.chat_histories.entry(history_key).or_default();
        history.push(compact_chat_message_for_panel(message));

        const MAX_CHAT_MESSAGES: usize = 100;
        if history.len() > MAX_CHAT_MESSAGES {
            let overflow = history.len().saturating_sub(MAX_CHAT_MESSAGES);
            history.drain(0..overflow);
        }
    }

    pub fn poll_chat_promise(&mut self, rt: &Handle, ctx: &egui::Context) {
        let Some(mut promise) = self.chat_promise.take() else {
            return;
        };

        let Some(result) = promise.ready_mut() else {
            self.chat_promise = Some(promise);
            return;
        };

        self.chat_loading = false;
        match result {
            Ok(resp) => {
                let current_role = self.chat_selected_role.clone();
                let current_session = self
                    .active_chat_session
                    .get(&current_role)
                    .cloned()
                    .unwrap_or_else(|| "default".to_string());
                self.push_chat_history_message(
                    &current_session,
                    &current_role,
                    ChatMessage {
                        role: "agent".to_string(),
                        content: resp.response.clone(),
                        agent_name: Some(self.chat_selected_role.clone()),
                        reasoning: resp.reasoning.clone(),
                        tool_calls: resp
                            .tool_calls
                            .clone()
                            .unwrap_or_default()
                            .into_iter()
                            .map(|t| ToolCallTrace {
                                name: t.name,
                                args: t.args,
                                result: t.result,
                                backup: t.backup,
                            })
                            .collect(),
                        artifacts: resp.artifacts.clone(),
                        chat_route: resp.chat_route.clone(),
                        tool_surface_mode: resp.tool_surface_mode.clone(),
                        runtime_persistence_status: resp.runtime_persistence_status.clone(),
                        task_id: resp.task_id.clone(),
                        run_id: resp.run_id.clone(),
                        trace_id: resp.trace_id.clone(),
                    },
                );
                self.do_session_runtime_tasks_refresh(rt, ctx, current_session.clone());
                self.do_session_delegation_refresh(rt, ctx, current_session);
            }
            Err(e) => {
                self.status_msg = Some((format!("Chat error: {}", e), true));
            }
        }
        ctx.request_repaint();
    }

    pub fn poll_rollback_promise(&mut self, ctx: &egui::Context) {
        if let Some(p) = self.rollback_promise.take() {
            match p.try_take() {
                Ok(Ok(())) => {
                    self.set_status("Undo successful!", false);
                    ctx.request_repaint();
                }
                Ok(Err(e)) => self.set_status(format!("Undo failed: {}", e), true),
                Err(p) => self.rollback_promise = Some(p),
            }
        }
    }

    pub fn poll_workspace_promise(&mut self) {
        if let Some(promise) = &self.pending_workspace_promise {
            if let Some(res) = promise.ready() {
                self.workspace_loading = false;
                match res {
                    Ok(list) => {
                        self.trusted_workspaces = list.clone();
                    }
                    Err(e) => {
                        tracing::error!("Failed to fetch workspaces: {}", e);
                    }
                }
                self.pending_workspace_promise = None;
            }
        }
    }

    pub fn poll_approval_promise(&mut self, rt: &Handle, ctx: &egui::Context) {
        if let Some(promise) = &self.pending_approval_promise {
            if let Some(res) = promise.ready() {
                match res {
                    Ok(list) => {
                        self.approvals = list.clone();
                        self.approval_error = None;
                    }
                    Err(e) => {
                        tracing::error!("Failed to fetch approvals: {}", e);
                        self.approval_error = Some(e.clone());
                    }
                }
                self.pending_approval_promise = None;
            }
        }

        if let Some(promise) = &self.pending_approval_receipts_promise {
            if let Some(res) = promise.ready() {
                match res {
                    Ok(list) => {
                        self.approval_receipts = list.clone();
                        self.approval_receipt_error = None;
                    }
                    Err(e) => {
                        tracing::error!("Failed to fetch approval receipts: {}", e);
                        self.approval_receipt_error = Some(e.clone());
                    }
                }
                self.pending_approval_receipts_promise = None;
            }
        }

        if let Some(promise) = &self.approval_resolve_promise {
            if let Some(result) = promise.ready() {
                match result {
                    Ok(()) => {
                        self.approval_resolve_promise = None;
                        self.do_approval_refresh(rt, ctx);
                        self.do_approval_receipt_refresh(rt, ctx);
                    }
                    Err(e) => {
                        self.set_status(format!("Approval resolution failed: {}", e), true);
                        self.approval_resolve_promise = None;
                    }
                }
            }
        }
    }

    pub fn do_approval_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_approval_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.list_approvals().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_approval_promise = Some(promise);
        self.last_approval_refresh_time = ctx.input(|i| i.time);
        self.do_approval_receipt_refresh(rt, ctx);
    }

    pub fn do_approval_receipt_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_approval_receipts_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .list_approval_receipts()
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_approval_receipts_promise = Some(promise);
    }

    pub fn do_resolve_approval(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        id: String,
        approved: bool,
    ) {
        if self.approval_resolve_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .resolve_approval(&id, approved)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.approval_resolve_promise = Some(promise);
    }

    pub fn do_restore_points_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_restore_points_promise.is_some() {
            return;
        }

        self.restore_points_loading = true;
        self.restore_points_error = None;
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .list_memory_restore_points()
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_restore_points_promise = Some(promise);
    }

    pub fn do_restore_create(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_restore_create_promise.is_some() {
            return;
        }

        self.restore_points_loading = true;
        self.restore_points_error = None;
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .create_memory_restore_point()
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_restore_create_promise = Some(promise);
    }

    pub fn do_restore_dry_run_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        backup_id: String,
    ) {
        if self.pending_restore_dry_run_promise.is_some() {
            return;
        }

        self.selected_restore_backup_id = Some(backup_id.clone());
        self.restore_points_loading = true;
        self.restore_points_error = None;
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .dry_run_memory_restore_point(&backup_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_restore_dry_run_promise = Some(promise);
    }

    pub fn do_restore_receipts_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        backup_id: String,
    ) {
        if self.pending_restore_receipts_promise.is_some() {
            return;
        }

        self.selected_restore_backup_id = Some(backup_id.clone());
        self.restore_points_loading = true;
        self.restore_points_error = None;
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .list_memory_restore_receipts(&backup_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_restore_receipts_promise = Some(promise);
    }

    pub fn do_restore_execute(&mut self, rt: &Handle, ctx: &egui::Context, backup_id: String) {
        if self.pending_restore_execute_promise.is_some() {
            return;
        }

        self.selected_restore_backup_id = Some(backup_id.clone());
        self.restore_points_loading = true;
        self.restore_points_error = None;
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .restore_memory_restore_point(&backup_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_restore_execute_promise = Some(promise);
    }

    pub fn do_restore_policy_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        backup_id: String,
    ) {
        if self.pending_restore_policy_promise.is_some() {
            return;
        }

        self.selected_restore_backup_id = Some(backup_id.clone());
        self.restore_points_loading = true;
        self.restore_points_error = None;
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .explain_memory_restore_policy(&backup_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_restore_policy_promise = Some(promise);
    }

    pub fn do_restore_delete(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        backup_id: String,
        dry_run: bool,
    ) {
        if self.pending_restore_delete_promise.is_some() {
            return;
        }

        self.selected_restore_backup_id = Some(backup_id.clone());
        self.restore_points_loading = true;
        self.restore_points_error = None;
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .delete_memory_restore_point(&backup_id, dry_run)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_restore_delete_promise = Some(promise);
    }

    pub fn poll_doctor_promise(&mut self) {
        if let Some(promise) = &self.pending_doctor_promise {
            if let Some(res) = promise.ready() {
                match res {
                    Ok(results) => {
                        self.doctor_results = Some(results.clone());
                        self.doctor_loading = false;
                    }
                    Err(e) => {
                        self.doctor_error = Some(e.clone());
                        self.doctor_loading = false;
                    }
                }
                self.pending_doctor_promise = None;
            }
        }
    }

    pub fn poll_repair_promise(&mut self) {
        if let Some(promise) = &self.pending_repair_promise {
            if let Some(res) = promise.ready() {
                match res {
                    Ok(msg) => {
                        self.set_status(format!("Repair: {}", msg), false);
                    }
                    Err(e) => {
                        self.set_status(format!("Repair failed: {}", e), true);
                    }
                }
                self.repair_loading = false;
                self.pending_repair_promise = None;
            }
        }
    }

    pub fn poll_cron_action_promise(&mut self) {
        if let Some(ref p) = self.pending_cron_action_promise {
            if let Some(res) = p.ready() {
                self.cron_loading = false;
                match res {
                    Ok(msg) => {
                        self.set_status(msg.clone(), false);
                        self.cron_form_name.clear();
                        self.cron_form_prompt.clear();
                        self.last_cron_refresh_time = -999.0;
                    }
                    Err(e) => {
                        self.set_status(format!("Error: {}", e), true);
                    }
                }
                self.pending_cron_action_promise = None;
            }
        }
    }

    fn poll_skills_promise(&mut self, _ctx: &egui::Context) {
        let resolved = if let Some(ref promise) = self.skills_promise {
            match promise.ready() {
                Some(Ok(skills)) => Some(Ok(skills.clone())),
                Some(Err(e)) => Some(Err(e.clone())),
                None => None,
            }
        } else {
            None
        };

        if let Some(result) = resolved {
            self.skills_promise = None;
            match result {
                Ok(skills) => {
                    self.skills = skills;
                    self.connected = Some(true);
                    self.set_status(format!("Loaded {} skills", self.skills.len()), false);
                }
                Err(e) => {
                    tracing::error!("Failed to poll skills: {}", e);
                    self.connected = Some(false);
                    if self.skills.is_empty() {
                        self.set_status("Waiting for Gateway engine...".to_string(), false);
                    } else {
                        self.set_status(format!("Connection Error: {}", e), true);
                    }
                }
            }
        }
    }

    pub fn poll_sessions_promise(&mut self) {
        if let Some(ref p) = self.pending_sessions_promise {
            if let Some(result) = p.ready() {
                self.sessions_loading = false;
                match result {
                    Ok(sessions) => {
                        self.sessions = sessions.clone();
                        self.sessions_error = None;
                    }
                    Err(e) => {
                        self.sessions_error = Some(e.clone());
                    }
                }
                self.pending_sessions_promise = None;
            }
        }
    }

    pub fn poll_session_runtime_tasks_promise(&mut self) {
        if let Some(ref p) = self.pending_session_runtime_tasks_promise {
            if let Some(result) = p.ready() {
                self.session_runtime_tasks_loading = false;
                match result {
                    Ok(tasks) => {
                        self.session_runtime_tasks = tasks.clone();
                        self.session_runtime_tasks_error = None;
                        if let Some(selected_trace_id) = self.selected_run_trace_id.as_deref() {
                            let trace_still_present = tasks
                                .iter()
                                .any(|task| task.trace_id.as_deref() == Some(selected_trace_id));
                            if !trace_still_present {
                                self.selected_run_trace = None;
                                self.selected_run_trace_id = None;
                                self.selected_run_trace_error = None;
                                self.selected_run_trace_loading = false;
                                self.pending_run_trace_promise = None;
                                self.selected_run_replay = None;
                                self.selected_run_replay_loading = false;
                                self.selected_run_replay_error = None;
                                self.pending_run_replay_promise = None;
                                self.selected_profiler_artifact = None;
                                self.selected_profiler_loading = false;
                                self.selected_profiler_error = None;
                                self.pending_profiler_promise = None;
                                self.selected_profiler_query_results.clear();
                                self.selected_profiler_query_loading = false;
                                self.selected_profiler_query_error = None;
                                self.pending_profiler_query_promise = None;
                                self.selected_profiler_export = None;
                                self.selected_profiler_export_loading = false;
                                self.selected_profiler_export_error = None;
                                self.pending_profiler_export_promise = None;
                                self.selected_witness_summary = None;
                                self.selected_witness_id = None;
                                self.selected_witness_error = None;
                                self.selected_witness_loading = false;
                                self.pending_witness_promise = None;
                                self.selected_witness_bundle = None;
                                self.selected_witness_bundle_error = None;
                                self.selected_witness_bundle_loading = false;
                                self.pending_witness_bundle_promise = None;
                                self.selected_witness_log = None;
                                self.selected_witness_log_error = None;
                                self.selected_witness_log_loading = false;
                                self.pending_witness_log_promise = None;
                                self.selected_witness_query_results.clear();
                                self.selected_witness_query_loading = false;
                                self.selected_witness_query_error = None;
                                self.pending_witness_query_promise = None;
                                self.selected_scorecard_query_results.clear();
                                self.selected_scorecard_query_loading = false;
                                self.selected_scorecard_query_error = None;
                                self.pending_scorecard_query_promise = None;
                            }
                        }
                    }
                    Err(e) => {
                        self.session_runtime_tasks_error = Some(e.clone());
                    }
                }
                self.pending_session_runtime_tasks_promise = None;
            }
        }
    }

    pub fn poll_task_output_promise(&mut self) {
        if let Some(ref p) = self.pending_task_output_promise {
            if let Some(result) = p.ready() {
                self.selected_task_output_loading = false;
                match result {
                    Ok(output) => {
                        self.selected_task_output_task_id = Some(output.task.id.clone());
                        self.selected_task_output = Some(output.clone());
                        self.selected_task_output_error = None;
                    }
                    Err(e) => {
                        self.selected_task_output_error = Some(e.clone());
                    }
                }
                self.pending_task_output_promise = None;
            }
        }
    }

    pub fn maybe_start_chat_task_output_backfill(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_chat_task_output_promise.is_some() {
            return;
        }
        let Some(session_id) = self.current_runtime_session_id() else {
            return;
        };
        let Some(task) = self
            .session_runtime_tasks
            .iter()
            .filter(|task| task.thread_id.as_deref() == Some(session_id.as_str()))
            .filter(|task| task.name == "foreground_chat_supervisor")
            .filter(|task| Self::chat_task_status_is_terminal(&task.status))
            .filter(|task| !self.chat_task_output_appended.contains(&task.id))
            .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
            .cloned()
        else {
            return;
        };

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let task_id = task.id.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_task_output(&task_id, Some(80))
                .await
                .map_err(|error| error.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_chat_task_output_task_id = Some(task.id);
        self.pending_chat_task_output_session_id = Some(session_id);
        self.pending_chat_task_output_promise = Some(promise);
    }

    pub fn poll_chat_task_output_backfill_promise(&mut self) {
        let Some(promise) = self.pending_chat_task_output_promise.take() else {
            return;
        };
        let Some(result) = promise.ready() else {
            self.pending_chat_task_output_promise = Some(promise);
            return;
        };

        let task_id = self.pending_chat_task_output_task_id.take();
        let session_id = self.pending_chat_task_output_session_id.take();
        match result {
            Ok(output) => {
                let task_id = task_id.unwrap_or_else(|| output.task.id.clone());
                let session_id = session_id
                    .or_else(|| output.task.thread_id.clone())
                    .unwrap_or_else(|| {
                        self.active_chat_session
                            .get(&self.chat_selected_role)
                            .cloned()
                            .unwrap_or_else(|| "default".to_string())
                    });
                let response_text = Self::task_output_response_text(&output);
                if let Some(response_text) = response_text {
                    let role = self.chat_selected_role.clone();
                    if !self.chat_history_contains_task_output(
                        &session_id,
                        &role,
                        &task_id,
                        Some(&response_text),
                    ) {
                        self.push_chat_history_message(
                            &session_id,
                            &role,
                            ChatMessage {
                                role: "agent".to_string(),
                                content: response_text,
                                agent_name: Some(role.clone()),
                                reasoning: None,
                                tool_calls: Vec::new(),
                                artifacts: Vec::new(),
                                chat_route: Some("background_task_output".to_string()),
                                tool_surface_mode: None,
                                runtime_persistence_status: Some(
                                    "task_output_backfilled".to_string(),
                                ),
                                task_id: Some(task_id.clone()),
                                run_id: output.task.run_id.clone(),
                                trace_id: output.task.trace_id.clone(),
                            },
                        );
                    }
                }
                self.chat_task_output_appended.insert(task_id);
            }
            Err(error) => {
                if let Some(task_id) = task_id {
                    self.chat_task_output_appended.insert(task_id);
                }
                self.status_msg = Some((format!("Task output backfill failed: {error}"), true));
            }
        }
    }

    fn chat_history_contains_task_output(
        &self,
        session_id: &str,
        role: &str,
        _task_id: &str,
        content: Option<&str>,
    ) -> bool {
        let Some(content) = content else {
            return false;
        };
        let key = format!("{}:{}", session_id, role);
        self.chat_histories.get(&key).is_some_and(|history| {
            history
                .iter()
                .any(|message| message.role == "agent" && message.content.trim() == content.trim())
        })
    }

    fn chat_task_status_is_terminal(status: &str) -> bool {
        matches!(
            status,
            "completed" | "blocked" | "failed" | "cancelled" | "paused"
        )
    }

    fn task_output_response_text(output: &TaskOutputInfo) -> Option<String> {
        let result = output.result.as_ref()?;
        result
            .get("response_text")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                result
                    .get("creation_contract")
                    .and_then(|value| value.get("text"))
                    .and_then(serde_json::Value::as_str)
            })
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    }

    pub fn poll_task_wait_promise(&mut self) {
        if let Some(ref p) = self.pending_task_wait_promise {
            if let Some(result) = p.ready() {
                self.selected_task_wait_loading = false;
                match result {
                    Ok(wait) => {
                        self.selected_task_wait_task_id = Some(wait.task.id.clone());
                        self.selected_task_wait_notice = Some(wait.reason.clone());
                        self.selected_task_wait_error = None;
                        if let Some(existing) = self
                            .session_runtime_tasks
                            .iter_mut()
                            .find(|task| task.id == wait.task.id)
                        {
                            *existing = wait.task.clone();
                        }
                    }
                    Err(e) => {
                        self.selected_task_wait_error = Some(e.clone());
                    }
                }
                self.pending_task_wait_promise = None;
            }
        }
    }

    pub fn poll_task_cancel_promise(&mut self) {
        if let Some(ref p) = self.pending_task_cancel_promise {
            if let Some(result) = p.ready().cloned() {
                match result {
                    Ok(task_id) => {
                        self.set_status(format!("Task {} cancelled", task_id), false);
                        if let Some(existing) = self
                            .session_runtime_tasks
                            .iter_mut()
                            .find(|task| task.id == *task_id)
                        {
                            existing.status = "cancelled".to_string();
                            existing.status_detail = Some("cancel requested".to_string());
                        }
                    }
                    Err(e) => {
                        self.set_status(format!("Task cancel failed: {}", e), true);
                    }
                }
                self.pending_task_cancel_promise = None;
            }
        }
    }

    pub fn poll_session_delegation_promise(&mut self) {
        if let Some(ref p) = self.pending_session_delegation_promise {
            if let Some(result) = p.ready() {
                self.selected_session_delegation_loading = false;
                match result {
                    Ok(trace) => {
                        self.selected_session_delegation_session_id =
                            Some(trace.session_id.clone());
                        self.selected_session_delegation_trace = Some(trace.clone());
                        self.selected_session_delegation_error = None;
                    }
                    Err(e) => {
                        self.selected_session_delegation_session_id = None;
                        self.selected_session_delegation_trace = None;
                        self.selected_session_delegation_error = Some(e.clone());
                    }
                }
                self.pending_session_delegation_promise = None;
            }
        }
    }

    pub fn poll_run_trace_promise(&mut self) {
        if let Some(ref p) = self.pending_run_trace_promise {
            if let Some(result) = p.ready() {
                self.selected_run_trace_loading = false;
                match result {
                    Ok(trace) => {
                        self.selected_witness_summary = trace.witness.clone();
                        self.selected_witness_id = trace
                            .witness
                            .as_ref()
                            .map(|witness| witness.witness_id.to_string());
                        self.selected_witness_error = None;
                        self.selected_witness_loading = false;
                        self.pending_witness_promise = None;
                        self.selected_run_trace = Some(trace.clone());
                        self.selected_run_trace_error = None;
                        self.selected_run_replay = None;
                        self.selected_run_replay_error = None;
                        self.selected_run_replay_loading = false;
                        self.pending_run_replay_promise = None;
                        self.selected_profiler_artifact = None;
                        self.selected_profiler_error = None;
                        self.selected_profiler_loading = false;
                        self.pending_profiler_promise = None;
                        self.selected_profiler_query_results.clear();
                        self.selected_profiler_query_loading = false;
                        self.selected_profiler_query_error = None;
                        self.pending_profiler_query_promise = None;
                        self.selected_profiler_export = None;
                        self.selected_profiler_export_loading = false;
                        self.selected_profiler_export_error = None;
                        self.pending_profiler_export_promise = None;
                        self.selected_witness_query_results.clear();
                        self.selected_witness_query_loading = false;
                        self.selected_witness_query_error = None;
                        self.pending_witness_query_promise = None;
                        self.selected_scorecard_query_results.clear();
                        self.selected_scorecard_query_loading = false;
                        self.selected_scorecard_query_error = None;
                        self.pending_scorecard_query_promise = None;
                    }
                    Err(e) => {
                        self.selected_run_trace = None;
                        self.selected_run_trace_error = Some(e.clone());
                        self.selected_run_replay = None;
                        self.selected_run_replay_error = None;
                        self.selected_run_replay_loading = false;
                        self.pending_run_replay_promise = None;
                        self.selected_profiler_artifact = None;
                        self.selected_profiler_error = None;
                        self.selected_profiler_loading = false;
                        self.pending_profiler_promise = None;
                        self.selected_profiler_query_results.clear();
                        self.selected_profiler_query_loading = false;
                        self.selected_profiler_query_error = None;
                        self.pending_profiler_query_promise = None;
                        self.selected_profiler_export = None;
                        self.selected_profiler_export_loading = false;
                        self.selected_profiler_export_error = None;
                        self.pending_profiler_export_promise = None;
                        self.selected_witness_query_results.clear();
                        self.selected_witness_query_loading = false;
                        self.selected_witness_query_error = None;
                        self.pending_witness_query_promise = None;
                        self.selected_scorecard_query_results.clear();
                        self.selected_scorecard_query_loading = false;
                        self.selected_scorecard_query_error = None;
                        self.pending_scorecard_query_promise = None;
                        self.selected_witness_summary = None;
                        self.selected_witness_id = None;
                        self.selected_witness_error = None;
                        self.selected_witness_loading = false;
                        self.pending_witness_promise = None;
                        self.selected_witness_bundle = None;
                        self.selected_witness_bundle_error = None;
                        self.selected_witness_bundle_loading = false;
                        self.pending_witness_bundle_promise = None;
                        self.selected_witness_log = None;
                        self.selected_witness_log_error = None;
                        self.selected_witness_log_loading = false;
                        self.pending_witness_log_promise = None;
                    }
                }
                self.pending_run_trace_promise = None;
            }
        }
    }

    pub fn poll_run_replay_promise(&mut self) {
        if let Some(ref p) = self.pending_run_replay_promise {
            if let Some(result) = p.ready() {
                self.selected_run_replay_loading = false;
                match result {
                    Ok(replay) => {
                        self.selected_run_replay = Some(replay.clone());
                        self.selected_run_replay_error = None;
                    }
                    Err(e) => {
                        self.selected_run_replay = None;
                        self.selected_run_replay_error = Some(e.clone());
                    }
                }
                self.pending_run_replay_promise = None;
            }
        }
    }

    pub fn poll_profiler_promise(&mut self) {
        if let Some(ref p) = self.pending_profiler_promise {
            if let Some(result) = p.ready() {
                self.selected_profiler_loading = false;
                match result {
                    Ok(profiler) => {
                        self.selected_profiler_artifact = Some(profiler.clone());
                        self.selected_profiler_error = None;
                    }
                    Err(e) => {
                        self.selected_profiler_artifact = None;
                        self.selected_profiler_error = Some(e.clone());
                    }
                }
                self.pending_profiler_promise = None;
            }
        }
    }

    pub fn poll_profiler_query_promise(&mut self) {
        if let Some(ref p) = self.pending_profiler_query_promise {
            if let Some(result) = p.ready() {
                self.selected_profiler_query_loading = false;
                match result {
                    Ok(artifacts) => {
                        self.selected_profiler_query_results = artifacts.clone();
                        self.selected_profiler_query_error = None;
                    }
                    Err(e) => {
                        self.selected_profiler_query_results.clear();
                        self.selected_profiler_query_error = Some(e.clone());
                    }
                }
                self.pending_profiler_query_promise = None;
            }
        }
    }

    pub fn poll_profiler_export_promise(&mut self) {
        if let Some(ref p) = self.pending_profiler_export_promise {
            if let Some(result) = p.ready() {
                self.selected_profiler_export_loading = false;
                match result {
                    Ok(export) => {
                        self.selected_profiler_export = Some(export.clone());
                        self.selected_profiler_export_error = None;
                    }
                    Err(e) => {
                        self.selected_profiler_export = None;
                        self.selected_profiler_export_error = Some(e.clone());
                    }
                }
                self.pending_profiler_export_promise = None;
            }
        }
    }

    pub fn poll_witness_promise(&mut self) {
        if let Some(ref p) = self.pending_witness_promise {
            if let Some(result) = p.ready() {
                self.selected_witness_loading = false;
                match result {
                    Ok(witness) => {
                        self.selected_witness_summary = Some(witness.clone());
                        self.selected_witness_id = Some(witness.witness_id.to_string());
                        self.selected_witness_error = None;
                        self.selected_witness_bundle = None;
                        self.selected_witness_bundle_error = None;
                        self.selected_witness_bundle_loading = false;
                        self.pending_witness_bundle_promise = None;
                        self.selected_witness_log = None;
                        self.selected_witness_log_error = None;
                        self.selected_witness_log_loading = false;
                        self.pending_witness_log_promise = None;
                        self.selected_witness_query_results.clear();
                        self.selected_witness_query_loading = false;
                        self.selected_witness_query_error = None;
                        self.pending_witness_query_promise = None;
                        self.selected_scorecard_query_results.clear();
                        self.selected_scorecard_query_loading = false;
                        self.selected_scorecard_query_error = None;
                        self.pending_scorecard_query_promise = None;
                    }
                    Err(e) => {
                        self.selected_witness_summary = None;
                        self.selected_witness_error = Some(e.clone());
                        self.selected_witness_bundle = None;
                        self.selected_witness_bundle_error = None;
                        self.selected_witness_bundle_loading = false;
                        self.pending_witness_bundle_promise = None;
                        self.selected_witness_log = None;
                        self.selected_witness_log_error = None;
                        self.selected_witness_log_loading = false;
                        self.pending_witness_log_promise = None;
                        self.selected_witness_query_results.clear();
                        self.selected_witness_query_loading = false;
                        self.selected_witness_query_error = None;
                        self.pending_witness_query_promise = None;
                        self.selected_scorecard_query_results.clear();
                        self.selected_scorecard_query_loading = false;
                        self.selected_scorecard_query_error = None;
                        self.pending_scorecard_query_promise = None;
                    }
                }
                self.pending_witness_promise = None;
            }
        }
    }

    pub fn poll_witness_bundle_promise(&mut self) {
        if let Some(ref p) = self.pending_witness_bundle_promise {
            if let Some(result) = p.ready() {
                self.selected_witness_bundle_loading = false;
                match result {
                    Ok(bundle) => {
                        self.selected_witness_bundle = Some(bundle.clone());
                        self.selected_witness_bundle_error = None;
                    }
                    Err(e) => {
                        self.selected_witness_bundle = None;
                        self.selected_witness_bundle_error = Some(e.clone());
                    }
                }
                self.pending_witness_bundle_promise = None;
            }
        }
    }

    pub fn poll_witness_log_promise(&mut self) {
        if let Some(ref p) = self.pending_witness_log_promise {
            if let Some(result) = p.ready() {
                self.selected_witness_log_loading = false;
                match result {
                    Ok(log) => {
                        self.selected_witness_log = Some(log.clone());
                        self.selected_witness_log_error = None;
                    }
                    Err(e) => {
                        self.selected_witness_log = None;
                        self.selected_witness_log_error = Some(e.clone());
                    }
                }
                self.pending_witness_log_promise = None;
            }
        }
    }

    pub fn poll_witness_query_promise(&mut self) {
        if let Some(ref p) = self.pending_witness_query_promise {
            if let Some(result) = p.ready() {
                self.selected_witness_query_loading = false;
                match result {
                    Ok(logs) => {
                        self.selected_witness_query_results = logs.clone();
                        self.selected_witness_query_error = None;
                    }
                    Err(e) => {
                        self.selected_witness_query_results.clear();
                        self.selected_witness_query_error = Some(e.clone());
                    }
                }
                self.pending_witness_query_promise = None;
            }
        }
    }

    pub fn poll_scorecard_query_promise(&mut self) {
        if let Some(ref p) = self.pending_scorecard_query_promise {
            if let Some(result) = p.ready() {
                self.selected_scorecard_query_loading = false;
                match result {
                    Ok(scorecards) => {
                        self.selected_scorecard_query_results = scorecards.clone();
                        self.selected_scorecard_query_error = None;
                    }
                    Err(e) => {
                        self.selected_scorecard_query_results.clear();
                        self.selected_scorecard_query_error = Some(e.clone());
                    }
                }
                self.pending_scorecard_query_promise = None;
            }
        }
    }

    pub fn poll_sandbox_promises(&mut self, rt: &Handle, ctx: &egui::Context) {
        if let Some(ref p) = self.sandboxes_promise {
            if let Some(res) = p.ready() {
                match res {
                    Ok(list) => {
                        self.sandboxes = list.clone();
                    }
                    Err(e) => {
                        self.set_status(format!("Failed to retrieve sandboxes: {}", e), true);
                    }
                }
                self.sandboxes_promise = None;
            }
        }
    }

    pub fn poll_restore_point_promises(&mut self) {
        if let Some(promise) = &self.pending_restore_create_promise {
            if let Some(result) = promise.ready() {
                self.restore_points_loading = false;
                match result {
                    Ok(manifest) => {
                        self.restore_points
                            .retain(|point| point.backup_id != manifest.backup_id);
                        self.restore_points.insert(0, manifest.clone());
                        self.selected_restore_backup_id = Some(manifest.backup_id.clone());
                        self.selected_restore_dry_run = None;
                        self.selected_restore_policy_basis = None;
                        self.selected_restore_receipts.clear();
                        self.selected_restore_delete_report = None;
                        self.restore_points_error = None;
                        self.set_status(
                            format!("Created restore point {}", manifest.backup_id),
                            false,
                        );
                    }
                    Err(error) => {
                        self.restore_points_error = Some(error.clone());
                        self.set_status(format!("Failed to create restore point: {}", error), true);
                    }
                }
                self.pending_restore_create_promise = None;
            }
        }

        if let Some(promise) = &self.pending_restore_points_promise {
            if let Some(result) = promise.ready() {
                self.restore_points_loading = false;
                match result {
                    Ok(points) => {
                        self.restore_points = points.clone();
                        self.restore_points_error = None;
                        if self.selected_restore_backup_id.is_none() {
                            self.selected_restore_backup_id =
                                points.first().map(|point| point.backup_id.clone());
                        }
                    }
                    Err(error) => {
                        self.restore_points_error = Some(error.clone());
                    }
                }
                self.pending_restore_points_promise = None;
            }
        }

        if let Some(promise) = &self.pending_restore_dry_run_promise {
            if let Some(result) = promise.ready() {
                self.restore_points_loading = false;
                match result {
                    Ok(report) => {
                        self.selected_restore_dry_run = Some(report.clone());
                        self.restore_points_error = None;
                    }
                    Err(error) => {
                        self.selected_restore_dry_run = None;
                        self.restore_points_error = Some(error.clone());
                    }
                }
                self.pending_restore_dry_run_promise = None;
            }
        }

        if let Some(promise) = &self.pending_restore_policy_promise {
            if let Some(result) = promise.ready() {
                self.restore_points_loading = false;
                match result {
                    Ok(policy) => {
                        self.selected_restore_policy_basis = Some(policy.clone());
                        self.restore_points_error = None;
                    }
                    Err(error) => {
                        self.selected_restore_policy_basis = None;
                        self.restore_points_error = Some(error.clone());
                    }
                }
                self.pending_restore_policy_promise = None;
            }
        }

        if let Some(promise) = &self.pending_restore_receipts_promise {
            if let Some(result) = promise.ready() {
                self.restore_points_loading = false;
                match result {
                    Ok(receipts) => {
                        self.selected_restore_receipts = receipts.clone();
                        self.restore_points_error = None;
                    }
                    Err(error) => {
                        self.selected_restore_receipts.clear();
                        self.restore_points_error = Some(error.clone());
                    }
                }
                self.pending_restore_receipts_promise = None;
            }
        }

        if let Some(promise) = &self.pending_restore_execute_promise {
            if let Some(result) = promise.ready() {
                self.restore_points_loading = false;
                match result {
                    Ok(receipt) => {
                        self.selected_restore_backup_id = Some(receipt.backup_id.clone());
                        self.selected_restore_receipts
                            .retain(|item| item.receipt_id != receipt.receipt_id);
                        self.selected_restore_receipts.insert(0, receipt.clone());
                        self.selected_restore_delete_report = None;
                        self.restore_points_error = None;
                        self.set_status(
                            format!(
                                "Restore completed for {} ({})",
                                receipt.backup_id, receipt.receipt_id
                            ),
                            false,
                        );
                    }
                    Err(error) => {
                        self.restore_points_error = Some(error.clone());
                        self.set_status(format!("Restore failed: {}", error), true);
                    }
                }
                self.pending_restore_execute_promise = None;
            }
        }

        if let Some(promise) = &self.pending_restore_delete_promise {
            if let Some(result) = promise.ready() {
                self.restore_points_loading = false;
                match result {
                    Ok(report) => {
                        let deleted_backup_id = report.backup_id.clone();
                        let was_dry_run = report.dry_run;
                        self.selected_restore_delete_report = Some(report.clone());
                        self.restore_points_error = None;
                        if !was_dry_run {
                            self.restore_points
                                .retain(|point| point.backup_id != deleted_backup_id);
                            self.selected_restore_receipts
                                .retain(|receipt| receipt.backup_id != deleted_backup_id);
                            if self.selected_restore_backup_id.as_deref()
                                == Some(deleted_backup_id.as_str())
                            {
                                self.selected_restore_backup_id = self
                                    .restore_points
                                    .first()
                                    .map(|point| point.backup_id.clone());
                                self.selected_restore_dry_run = None;
                                self.selected_restore_policy_basis = None;
                            }
                        }
                    }
                    Err(error) => {
                        self.selected_restore_delete_report = None;
                        self.restore_points_error = Some(error.clone());
                    }
                }
                self.pending_restore_delete_promise = None;
            }
        }
    }

    pub fn poll_cron_promise(&mut self) {
        if let Some(ref p) = self.pending_cron_promise {
            if let Some(result) = p.ready() {
                self.cron_loading = false;
                match result {
                    Ok(jobs) => {
                        self.cron_jobs = jobs.clone();
                        self.cron_error = None;
                    }
                    Err(e) => {
                        self.cron_error = Some(e.clone());
                    }
                }
                self.pending_cron_promise = None;
            }
        }
    }

    fn poll_provider_promise(&mut self) {
        if let Some(ref p) = self.pending_provider_promise {
            if let Some(result) = p.ready() {
                self.provider_loading = false;
                match result {
                    Ok(resp) => {
                        self.provider_metadata = resp.providers.clone();
                        self.provider_error = None;

                        for provider in &resp.providers {
                            for field in &provider.fields {
                                if !self.vault_entries.iter().any(|e| e.key == field.key) {
                                    self.vault_entries.push(VaultEntry {
                                        key: field.key.clone(),
                                        saved: false,
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                    Err(e) => {
                        self.provider_error = Some(e.clone());
                    }
                }
                self.pending_provider_promise = None;
            }
        }
    }

    fn poll_agent_role_promise(&mut self, _ctx: &egui::Context) {
        let mut loaded_dto = None;
        if let Some(ref p) = self.agent_role_promise {
            if let Some(res) = p.ready() {
                match res {
                    Ok(dto) => {
                        loaded_dto = Some(dto.clone());
                    }
                    Err(e) => {
                        self.set_status(format!("Failed to load agent: {}", e), true);
                    }
                }
                self.agent_role_promise = None;
            }
        }

        if let Some(dto) = loaded_dto {
            self.agent_role_content = dto.content.clone();
            self.agent_role_loaded = true;
            self.agent_role_dirty = false;
            self.agent_role_artifact_policy_dirty = false;

            self.update_agent_fields_from_content(
                dto.runtime.as_ref(),
                dto.artifact_policy.as_ref(),
            );

            self.agent_role_promise = None;
        }
    }

    pub fn poll_metrics_promise(&mut self, ctx: &egui::Context) {
        if let Some(promise) = &self.pending_metrics_promise {
            if let Some(res) = promise.ready() {
                match res {
                    Ok(metrics) => {
                        let now = ctx.input(|i| i.time);
                        self.last_metrics = Some(metrics.clone());
                        let ram_usage = if let Some(h) = &metrics.host {
                            if h.memory_total_mb > 0 {
                                h.memory_used_mb as f32 / h.memory_total_mb as f32
                            } else {
                                0.0
                            }
                        } else {
                            0.0
                        };
                        let vram_usage = if let Some(h) = &metrics.host {
                            if h.gpu_vram_total_mb > 0 {
                                h.gpu_vram_used_mb as f32 / h.gpu_vram_total_mb as f32
                            } else {
                                0.0
                            }
                        } else {
                            0.0
                        };
                        self.metrics_history.push(MetricsSnapshot {
                            time: now,
                            total_calls: metrics.total_calls.unwrap_or(0),
                            cpu_usage: metrics
                                .host
                                .as_ref()
                                .map(|h| h.cpu_usage_percent)
                                .unwrap_or(0.0),
                            ram_usage,
                            vram_usage,
                        });
                        if self.metrics_history.len() > 60 {
                            self.metrics_history.remove(0);
                        }
                        self.metrics_loading = false;
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        self.metrics_error = Some(e.clone());
                        self.metrics_loading = false;
                    }
                }
                self.pending_metrics_promise = None;
            }
        }
    }

    pub fn do_sessions_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_sessions_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let (sender, promise) = Promise::new();
        let ctx2 = ctx.clone();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.list_sessions().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_sessions_promise = Some(promise);
        self.last_sessions_refresh_time = ctx.input(|i| i.time);
    }

    pub fn current_runtime_session_id(&self) -> Option<String> {
        let role = self.chat_selected_role.clone();
        self.active_chat_session
            .get(&role)
            .cloned()
            .or_else(|| Some("default".to_string()))
    }

    pub fn do_session_runtime_tasks_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        session_id: String,
    ) {
        if self.pending_session_runtime_tasks_promise.is_some()
            && self.session_runtime_tasks_session_id.as_deref() == Some(session_id.as_str())
        {
            return;
        }

        self.session_runtime_tasks_loading = true;
        self.session_runtime_tasks_session_id = Some(session_id.clone());

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .list_session_tasks(&session_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_session_runtime_tasks_promise = Some(promise);
        self.last_session_runtime_tasks_refresh_time = ctx.input(|i| i.time);
    }

    pub fn do_runtime_task_output_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        task_id: String,
    ) {
        if self.pending_task_output_promise.is_some()
            && self.selected_task_output_task_id.as_deref() == Some(task_id.as_str())
        {
            return;
        }

        self.selected_task_output_loading = true;
        self.selected_task_output_task_id = Some(task_id.clone());
        self.selected_task_output_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_task_output(&task_id, Some(120))
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_task_output_promise = Some(promise);
    }

    pub fn do_runtime_task_wait(&mut self, rt: &Handle, ctx: &egui::Context, task_id: String) {
        if self.pending_task_wait_promise.is_some()
            && self.selected_task_wait_task_id.as_deref() == Some(task_id.as_str())
        {
            return;
        }

        self.selected_task_wait_loading = true;
        self.selected_task_wait_task_id = Some(task_id.clone());
        self.selected_task_wait_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .wait_task(&task_id, Some(60), Some(true))
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_task_wait_promise = Some(promise);
    }

    pub fn do_runtime_task_cancel(&mut self, rt: &Handle, ctx: &egui::Context, task_id: String) {
        if self.pending_task_cancel_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let task_id_for_request = task_id.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .cancel_runtime_task(&task_id_for_request)
                .await
                .map(|_| task_id_for_request)
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_task_cancel_promise = Some(promise);
        self.set_status(format!("Cancelling task {}...", task_id), false);
    }

    pub fn do_session_delegation_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        session_id: String,
    ) {
        if self.pending_session_delegation_promise.is_some()
            && self.selected_session_delegation_session_id.as_deref() == Some(session_id.as_str())
        {
            return;
        }

        self.selected_session_delegation_loading = true;
        self.selected_session_delegation_session_id = Some(session_id.clone());
        self.selected_session_delegation_error = None;
        self.selected_session_delegation_trace = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_session_delegation_trace(&session_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_session_delegation_promise = Some(promise);
    }

    pub fn do_run_trace_refresh(&mut self, rt: &Handle, ctx: &egui::Context, trace_id: String) {
        if self.pending_run_trace_promise.is_some()
            && self.selected_run_trace_id.as_deref() == Some(trace_id.as_str())
        {
            return;
        }

        self.selected_run_trace_loading = true;
        self.selected_run_trace_id = Some(trace_id.clone());
        self.selected_run_trace_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_run_trace(&trace_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_run_trace_promise = Some(promise);
    }

    pub fn do_run_replay_refresh(&mut self, rt: &Handle, ctx: &egui::Context, trace_id: String) {
        if self.pending_run_replay_promise.is_some()
            && self.selected_run_trace_id.as_deref() == Some(trace_id.as_str())
        {
            return;
        }

        self.selected_run_replay_loading = true;
        self.selected_run_replay_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_run_replay(&trace_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_run_replay_promise = Some(promise);
    }

    pub fn do_profiler_refresh(&mut self, rt: &Handle, ctx: &egui::Context, trace_id: String) {
        if self.pending_profiler_promise.is_some()
            && self.selected_run_trace_id.as_deref() == Some(trace_id.as_str())
        {
            return;
        }

        self.selected_profiler_loading = true;
        self.selected_profiler_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_run_profiler(&trace_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_profiler_promise = Some(promise);
    }

    pub fn do_profiler_query_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        query: benshu_telemetry::ProfilerArtifactQuery,
    ) {
        if self.pending_profiler_query_promise.is_some() {
            return;
        }

        self.selected_profiler_query_loading = true;
        self.selected_profiler_query_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .query_profiler_artifacts(&query)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_profiler_query_promise = Some(promise);
    }

    pub fn do_profiler_export_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        query: benshu_telemetry::ProfilerArtifactQuery,
    ) {
        if self.pending_profiler_export_promise.is_some() {
            return;
        }

        self.selected_profiler_export_loading = true;
        self.selected_profiler_export_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .export_profiler_artifacts(&query)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_profiler_export_promise = Some(promise);
    }

    pub fn do_witness_refresh(&mut self, rt: &Handle, ctx: &egui::Context, witness_id: String) {
        if self.pending_witness_promise.is_some()
            && self.selected_witness_id.as_deref() == Some(witness_id.as_str())
        {
            return;
        }

        self.selected_witness_loading = true;
        self.selected_witness_id = Some(witness_id.clone());
        self.selected_witness_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_witness_summary(&witness_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_witness_promise = Some(promise);
    }

    pub fn do_witness_bundle_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        witness_id: String,
    ) {
        if self.pending_witness_bundle_promise.is_some()
            && self.selected_witness_id.as_deref() == Some(witness_id.as_str())
        {
            return;
        }

        self.selected_witness_bundle_loading = true;
        self.selected_witness_bundle_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_witness_bundle(&witness_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_witness_bundle_promise = Some(promise);
    }

    pub fn do_witness_log_refresh(&mut self, rt: &Handle, ctx: &egui::Context, witness_id: String) {
        if self.pending_witness_log_promise.is_some()
            && self.selected_witness_id.as_deref() == Some(witness_id.as_str())
        {
            return;
        }

        self.selected_witness_log_loading = true;
        self.selected_witness_log_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_witness_log(&witness_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_witness_log_promise = Some(promise);
    }

    pub fn do_witness_query_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        query: benshu_telemetry::WitnessLogQuery,
    ) {
        if self.pending_witness_query_promise.is_some() {
            return;
        }

        self.selected_witness_query_loading = true;
        self.selected_witness_query_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .query_witness_logs(&query)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_witness_query_promise = Some(promise);
    }

    pub fn do_scorecard_query_refresh(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        query: benshu_telemetry::ScorecardQuery,
    ) {
        if self.pending_scorecard_query_promise.is_some() {
            return;
        }

        self.selected_scorecard_query_loading = true;
        self.selected_scorecard_query_error = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .query_scorecards(&query)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_scorecard_query_promise = Some(promise);
    }

    pub fn do_artifact_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_artifacts_promise.is_some() {
            return;
        }

        self.artifacts_loading = true;
        self.artifacts_error = None;
        let client = self.client.clone();
        let query = self.artifacts_query.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .list_artifacts(&query)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_artifacts_promise = Some(promise);
    }

    pub fn do_artifact_cleanup(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_artifact_cleanup_promise.is_some() {
            return;
        }

        self.artifact_cleanup_loading = true;
        self.artifact_cleanup_error = None;
        let client = self.client.clone();
        let policy = self.artifact_cleanup_policy.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .cleanup_artifacts(&policy)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_artifact_cleanup_promise = Some(promise);
    }

    pub fn do_open_artifact_target(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        artifact_id: Option<String>,
        target: Option<String>,
    ) {
        if self.open_target_promise.is_some() {
            return;
        }
        let has_artifact = artifact_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        let has_target = target
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        if !has_artifact && !has_target {
            self.set_status("Nothing to open.", true);
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .open_artifact_target(artifact_id, target)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.open_target_promise = Some(promise);
    }

    pub fn do_provider_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_provider_promise.is_some() {
            return;
        }
        self.provider_loading = true;
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let result = client
                .get_provider_schema()
                .await
                .map_err(|e| e.to_string());
            sender.send(result);
            ctx2.request_repaint();
        });
        self.pending_provider_promise = Some(promise);
    }

    pub fn do_agent_templates_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_agent_templates()
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.agent_templates_promise = Some(promise);
    }

    pub fn poll_log_promise(&mut self, ctx: &egui::Context) {
        let new_lines = if let Some(ref promise) = self.pending_log_promise {
            match promise.ready() {
                Some(lines) => Some(lines.clone()),
                None => None,
            }
        } else {
            return;
        };

        if let Some(lines) = new_lines {
            self.pending_log_promise = None;
            for line in lines {
                if self.log_lines.last().map(|l| l.as_str()) != Some(line.as_str()) {
                    self.log_lines.push(line);
                }
            }
            const MAX_LOG_LINES: usize = 500;
            if self.log_lines.len() > MAX_LOG_LINES {
                let drain_count = self.log_lines.len() - MAX_LOG_LINES;
                self.log_lines.drain(0..drain_count);
            }
            ctx.request_repaint();
        }
    }

    pub fn poll_artifact_promise(&mut self) {
        let Some(promise) = &self.pending_artifacts_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        self.artifacts_loading = false;
        match result {
            Ok(artifacts) => {
                self.artifacts = artifacts.clone();
                self.artifacts_error = None;
                if self.selected_artifact_id.as_ref().is_some_and(|id| {
                    !self
                        .artifacts
                        .iter()
                        .any(|artifact| &artifact.artifact_id == id)
                }) {
                    self.selected_artifact_id = None;
                }
                if self.selected_artifact_id.is_none() {
                    self.selected_artifact_id = self
                        .artifacts
                        .first()
                        .map(|artifact| artifact.artifact_id.clone());
                }
            }
            Err(error) => {
                self.artifacts_error = Some(error.clone());
            }
        }
        self.pending_artifacts_promise = None;
    }

    pub fn poll_artifact_cleanup_promise(&mut self) {
        let Some(promise) = &self.pending_artifact_cleanup_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        self.artifact_cleanup_loading = false;
        match result {
            Ok(report) => {
                self.last_artifact_cleanup_report = Some(report.clone());
                self.artifact_cleanup_error = None;
                self.set_status(
                    format!(
                        "Artifact cleanup finished: matched {} / deleted {}",
                        report.matched, report.deleted
                    ),
                    false,
                );
            }
            Err(error) => {
                self.artifact_cleanup_error = Some(error.clone());
                self.set_status(format!("Artifact cleanup failed: {}", error), true);
            }
        }
        self.pending_artifact_cleanup_promise = None;
    }

    pub fn poll_open_target_promise(&mut self) {
        let Some(promise) = &self.open_target_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        match result {
            Ok(report) => {
                let status = if report.opened {
                    format!(
                        "{} Opened {} with {}: {}",
                        report.message, report.target_kind, report.opener, report.target
                    )
                } else {
                    format!("{}: {}", report.message, report.target)
                };
                self.set_status(status, false);
            }
            Err(error) => {
                self.set_status(format!("Open failed: {}", error), true);
            }
        }
        self.open_target_promise = None;
    }

    pub fn poll_runtime_mode_promise(&mut self) {
        let Some(promise) = &self.pending_runtime_mode_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        self.runtime_mode_loading = false;
        match result {
            Ok(mode) => {
                self.connected = Some(mode.connected);
                self.gateway_version = Some(mode.gateway_version.clone());
                self.model_ram_limit_gb = mode.model_ram_limit_gb;
                self.model_vram_limit_gb = mode.model_vram_limit_gb;
                self.auto_consolidation_enabled = mode.auto_consolidation_enabled;
                self.enable_global_voice = mode.enable_global_voice;
                self.enable_local_vision = false;
                self.local_vision_status = mode.local_vision_status.clone();
                self.organ_vision_model = mode.vision_model.clone();
                self.organ_image_edit_model = mode.image_edit_model.clone();
                self.organ_audio_understanding_model = mode.audio_understanding_model.clone();
                self.organ_realtime_vad_model = mode.realtime_vad_model.clone();
                self.organ_duplex_voice_model = mode.duplex_voice_model.clone();
                self.organ_local_classifier_model = mode.local_classifier_model.clone();
                self.organ_local_router_model = mode.local_router_model.clone();
                self.organ_local_safety_model = mode.local_safety_model.clone();
                self.image_gen_model = mode.image_gen_model.clone();
                self.image_gen_status = mode.image_gen_status.clone();
                self.windows_ml_runtime_family = mode.windows_ml_runtime.runtime_family.clone();
                self.windows_ml_execution_provider_preference = mode
                    .windows_ml_runtime
                    .execution_provider_preference
                    .clone();
                self.windows_ml_device_target = mode.windows_ml_runtime.device_target.clone();
                self.windows_ml_cpu_fallback_policy =
                    mode.windows_ml_runtime.cpu_fallback_policy.clone();
                self.windows_ml_graph_optimization_level =
                    mode.windows_ml_runtime.graph_optimization_level.clone();
                self.windows_ml_intra_threads = mode
                    .windows_ml_runtime
                    .intra_threads
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.windows_ml_inter_threads = mode
                    .windows_ml_runtime
                    .inter_threads
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.windows_ml_text_batch_size = mode.windows_ml_runtime.text_profile.batch_size;
                self.windows_ml_text_max_sequence_length =
                    mode.windows_ml_runtime.text_profile.max_sequence_length;
                self.windows_ml_vision_max_image_side =
                    mode.windows_ml_runtime.vision_profile.max_image_side;
                self.windows_ml_vision_resize_policy =
                    mode.windows_ml_runtime.vision_profile.resize_policy.clone();
                self.windows_ml_audio_sample_rate_hz =
                    mode.windows_ml_runtime.audio_profile.sample_rate_hz;
                self.windows_ml_audio_chunk_ms = mode.windows_ml_runtime.audio_profile.chunk_ms;
                self.windows_ml_image_width = mode.windows_ml_runtime.image_profile.width;
                self.windows_ml_image_height = mode.windows_ml_runtime.image_profile.height;
                self.windows_ml_image_steps = mode.windows_ml_runtime.image_profile.steps;
                self.windows_ml_image_guidance =
                    mode.windows_ml_runtime.image_profile.guidance.to_string();
                self.windows_ml_realtime_vad_window_ms =
                    mode.windows_ml_runtime.realtime_profile.vad_window_ms;
                self.windows_ml_duplex_frame_ms =
                    mode.windows_ml_runtime.realtime_profile.duplex_frame_ms;
                self.windows_ml_safety_threshold =
                    mode.windows_ml_runtime.safety_profile.threshold.to_string();
                self.llama_tuning_mode = if mode.llama_cpp_runtime.tuning_mode.trim().is_empty() {
                    "auto".to_string()
                } else {
                    mode.llama_cpp_runtime.tuning_mode.clone()
                };
                self.llama_performance_profile =
                    if mode.llama_cpp_runtime.performance_profile.trim().is_empty() {
                        "balanced".to_string()
                    } else {
                        mode.llama_cpp_runtime.performance_profile.clone()
                    };
                self.llama_runtime_diagnostics = mode
                    .llama_cpp_runtime
                    .effective_diagnostics
                    .as_ref()
                    .map(|diag| {
                        let notes = if diag.notes.is_empty() {
                            "notes=none".to_string()
                        } else {
                            format!("notes={}", diag.notes.join("; "))
                        };
                        let resource_estimate = mode
                            .llama_cpp_runtime
                            .last_recommendation
                            .as_ref()
                            .map(|rec| {
                                let recommendation_kv_location =
                                    if rec.recommended_kv_offload { "VRAM" } else { "RAM" };
                                format!(
                                    "recommendation est_vram={}MiB est_ram={}MiB kv_cache={}MiB kv_location={}",
                                    rec.memory_plan.estimated_vram_mb,
                                    rec.memory_plan.estimated_ram_mb,
                                    rec.memory_plan.kv_cache_budget_mb,
                                    recommendation_kv_location
                                )
                            })
                            .unwrap_or_else(|| {
                                format!(
                                    "current ctx_size={} gpu_layers={} kv_offload={}",
                                    mode.llama_cpp_runtime.ctx_size,
                                    mode.llama_cpp_runtime.gpu_layers,
                                    mode.llama_cpp_runtime.kv_offload
                                )
                            });
                        let effective_memory = diag
                            .effective_memory_plan
                            .as_ref()
                            .map(|plan| {
                                let kv_location = if diag.effective_kv_location.trim().is_empty() {
                                    if mode.llama_cpp_runtime.kv_offload {
                                        "VRAM"
                                    } else {
                                        "RAM"
                                    }
                                } else {
                                    diag.effective_kv_location.as_str()
                                };
                                format!(
                                    "effective est_vram={}MiB est_ram_or_commit={}MiB kv_cache={}MiB kv_location={}",
                                    plan.estimated_vram_mb,
                                    plan.estimated_ram_mb,
                                    plan.kv_cache_budget_mb,
                                    kv_location
                                )
                            })
                            .unwrap_or_else(|| diag.effective_memory_summary.clone());
                        format!(
                            "status={} mode={} profile={}\ncurrent: ctx_size={} gpu_layers={} batch={} ubatch={} kv_offload={}\n{}\n{}\neffective: {}\n{}",
                            diag.status,
                            diag.tuning_mode,
                            diag.performance_profile,
                            mode.llama_cpp_runtime.ctx_size,
                            mode.llama_cpp_runtime.gpu_layers,
                            mode.llama_cpp_runtime.batch_size,
                            mode.llama_cpp_runtime.ubatch_size,
                            mode.llama_cpp_runtime.kv_offload,
                            effective_memory,
                            resource_estimate,
                            diag.effective_value_summary,
                            notes
                        )
                    })
                    .or_else(|| {
                        mode.llama_cpp_runtime.last_recommendation.as_ref().map(|rec| {
                            let recommendation_kv_location =
                                if rec.recommended_kv_offload { "VRAM" } else { "RAM" };
                            format!(
                                "capacity estimate current_gpu_layers={} current_ctx_size={} batch={} ubatch={} rec_est_vram={}MiB rec_est_ram={}MiB kv_cache={}MiB kv_location={}",
                                mode.llama_cpp_runtime.gpu_layers,
                                mode.llama_cpp_runtime.ctx_size,
                                rec.recommended_batch_size,
                                rec.recommended_ubatch_size,
                                rec.memory_plan.estimated_vram_mb,
                                rec.memory_plan.estimated_ram_mb,
                                rec.memory_plan.kv_cache_budget_mb,
                                recommendation_kv_location
                            )
                        })
                    })
                    .unwrap_or_default();
                self.llama_ctx_size = mode.llama_cpp_runtime.ctx_size;
                self.llama_gpu_layers = mode.llama_cpp_runtime.gpu_layers;
                self.llama_threads = mode.llama_cpp_runtime.threads;
                self.llama_threads_batch = mode
                    .llama_cpp_runtime
                    .threads_batch
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_batch_size = mode.llama_cpp_runtime.batch_size;
                self.llama_ubatch_size = mode.llama_cpp_runtime.ubatch_size;
                self.llama_parallel_slots = mode.llama_cpp_runtime.parallel_slots;
                self.llama_cache_ram = mode
                    .llama_cpp_runtime
                    .cache_ram
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_ctx_checkpoints = mode
                    .llama_cpp_runtime
                    .ctx_checkpoints
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_flash_attn_mode = mode.llama_cpp_runtime.flash_attn_mode.clone();
                self.llama_kv_offload = mode.llama_cpp_runtime.kv_offload;
                self.llama_mmap = mode.llama_cpp_runtime.mmap;
                self.llama_mlock = mode.llama_cpp_runtime.mlock;
                self.llama_cache_prompt = mode.llama_cpp_runtime.cache_prompt;
                self.llama_cont_batching = mode.llama_cpp_runtime.cont_batching;
                self.llama_warmup = mode.llama_cpp_runtime.warmup;
                self.llama_context_shift = mode.llama_cpp_runtime.context_shift;
                self.llama_jinja = mode.llama_cpp_runtime.jinja;
                self.llama_rope_scaling = mode
                    .llama_cpp_runtime
                    .rope_scaling
                    .clone()
                    .unwrap_or_default();
                self.llama_rope_scale = mode
                    .llama_cpp_runtime
                    .rope_scale
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_rope_freq_base = mode
                    .llama_cpp_runtime
                    .rope_freq_base
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_rope_freq_scale = mode
                    .llama_cpp_runtime
                    .rope_freq_scale
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_yarn_orig_ctx = mode
                    .llama_cpp_runtime
                    .yarn_orig_ctx
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_yarn_ext_factor = mode
                    .llama_cpp_runtime
                    .yarn_ext_factor
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_yarn_attn_factor = mode
                    .llama_cpp_runtime
                    .yarn_attn_factor
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_yarn_beta_slow = mode
                    .llama_cpp_runtime
                    .yarn_beta_slow
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_yarn_beta_fast = mode
                    .llama_cpp_runtime
                    .yarn_beta_fast
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_cache_type_k = mode
                    .llama_cpp_runtime
                    .cache_type_k
                    .clone()
                    .unwrap_or_default();
                self.llama_cache_type_v = mode
                    .llama_cpp_runtime
                    .cache_type_v
                    .clone()
                    .unwrap_or_default();
                self.llama_device = mode.llama_cpp_runtime.device.clone().unwrap_or_default();
                self.llama_split_mode = mode
                    .llama_cpp_runtime
                    .split_mode
                    .clone()
                    .unwrap_or_default();
                self.llama_tensor_split = mode
                    .llama_cpp_runtime
                    .tensor_split
                    .clone()
                    .unwrap_or_default();
                self.llama_main_gpu = mode
                    .llama_cpp_runtime
                    .main_gpu
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_fit_mode = mode.llama_cpp_runtime.fit_mode.clone();
                self.llama_fit_target = mode
                    .llama_cpp_runtime
                    .fit_target
                    .clone()
                    .unwrap_or_default();
                self.llama_fit_ctx = mode
                    .llama_cpp_runtime
                    .fit_ctx
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_cpu_moe = mode.llama_cpp_runtime.cpu_moe;
                self.llama_n_cpu_moe = mode
                    .llama_cpp_runtime
                    .n_cpu_moe
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_mmproj_offload = mode.llama_cpp_runtime.mmproj_offload;
                self.llama_image_min_tokens = mode
                    .llama_cpp_runtime
                    .image_min_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_image_max_tokens = mode
                    .llama_cpp_runtime
                    .image_max_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_reasoning_mode = mode.llama_cpp_runtime.reasoning_mode.clone();
                self.llama_reasoning_format = mode.llama_cpp_runtime.reasoning_format.clone();
                self.llama_reasoning_budget = mode
                    .llama_cpp_runtime
                    .reasoning_budget
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.llama_reasoning_budget_message = mode
                    .llama_cpp_runtime
                    .reasoning_budget_message
                    .clone()
                    .unwrap_or_default();
                self.llama_sampling_temperature =
                    mode.llama_cpp_runtime.sampling_temperature.to_string();
                self.llama_sampling_top_k = mode.llama_cpp_runtime.sampling_top_k.to_string();
                self.llama_sampling_top_p = mode.llama_cpp_runtime.sampling_top_p.to_string();
                self.llama_sampling_min_p = mode.llama_cpp_runtime.sampling_min_p.to_string();
                self.llama_sampling_typical_p =
                    mode.llama_cpp_runtime.sampling_typical_p.to_string();
                self.llama_sampling_repeat_penalty =
                    mode.llama_cpp_runtime.sampling_repeat_penalty.to_string();
                self.llama_sampling_presence_penalty =
                    mode.llama_cpp_runtime.sampling_presence_penalty.to_string();
                self.llama_sampling_frequency_penalty = mode
                    .llama_cpp_runtime
                    .sampling_frequency_penalty
                    .to_string();
                self.llama_sampling_mirostat = mode.llama_cpp_runtime.sampling_mirostat.to_string();
                self.llama_sampling_mirostat_eta =
                    mode.llama_cpp_runtime.sampling_mirostat_eta.to_string();
                self.llama_sampling_mirostat_tau =
                    mode.llama_cpp_runtime.sampling_mirostat_tau.to_string();
                self.llama_seed = mode
                    .llama_cpp_runtime
                    .seed
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                self.runtime_mode_error = None;
            }
            Err(error) => {
                self.runtime_mode_error = Some(error.clone());
            }
        }

        self.pending_runtime_mode_promise = None;
    }

    pub fn poll_local_model_stack_promise(&mut self) {
        let Some(promise) = &self.pending_local_model_stack_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        self.local_model_stack_loading = false;
        match result {
            Ok(stack) => {
                self.local_model_stack = Some(stack.clone());
                self.local_model_stack_error = None;
            }
            Err(error) => {
                self.local_model_stack_error = Some(error.clone());
            }
        }

        self.pending_local_model_stack_promise = None;
    }

    pub fn poll_local_model_artifacts_promise(&mut self) {
        let Some(promise) = &self.pending_local_model_artifacts_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        self.local_model_artifacts_loading = false;
        match result {
            Ok(catalog) => {
                self.local_model_artifacts = Some(catalog.clone());
                self.local_model_artifacts_error = None;
            }
            Err(error) => {
                self.local_model_artifacts_error = Some(error.clone());
            }
        }

        self.pending_local_model_artifacts_promise = None;
    }

    pub fn poll_knowledge_import_promise(&mut self, rt: &Handle, ctx: &egui::Context) {
        let Some(promise) = &self.pending_knowledge_import_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        self.knowledge_import_loading = false;
        match result {
            Ok(report) => {
                self.knowledge_import_error = None;
                self.last_knowledge_import_report = Some(report.clone());
                self.set_status(
                    format!(
                        "Knowledge import finished: {} imported, {} unchanged, {} unsupported, {} too large, {} missing, {} failed.",
                        report.imported_count,
                        report.skipped_unchanged_count,
                        report.skipped_unsupported_count,
                        report.skipped_too_large_count,
                        report.skipped_missing_count,
                        report.failed_count
                    ),
                    report.failed_count > 0,
                );
                self.do_local_model_stack_refresh(rt, ctx);
            }
            Err(error) => {
                self.knowledge_import_error = Some(error.clone());
                self.set_status(format!("Knowledge import failed: {}", error), true);
            }
        }

        self.pending_knowledge_import_promise = None;
    }

    pub fn poll_knowledge_documents_promise(&mut self, _rt: &Handle, _ctx: &egui::Context) {
        let Some(promise) = &self.pending_knowledge_documents_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        self.knowledge_documents_loading = false;
        match result {
            Ok(report) => {
                self.knowledge_documents = report.documents.clone();
                self.knowledge_documents_error = None;
                self.set_status(
                    format!(
                        "Knowledge documents loaded: {} document(s).",
                        self.knowledge_documents.len()
                    ),
                    false,
                );
            }
            Err(error) => {
                self.knowledge_documents_error = Some(error.clone());
                self.set_status(format!("Knowledge document list failed: {}", error), true);
            }
        }

        self.pending_knowledge_documents_promise = None;
    }

    pub fn poll_knowledge_delete_promise(&mut self, rt: &Handle, ctx: &egui::Context) {
        let Some(promise) = &self.pending_knowledge_delete_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        match result {
            Ok(report) => {
                self.knowledge_documents.retain(|doc| {
                    !(doc.collection == report.collection && doc.path == report.path)
                });
                self.set_status(
                    format!(
                        "Knowledge document {}: {}/{}",
                        if report.deleted {
                            "deleted"
                        } else {
                            "not found"
                        },
                        report.collection,
                        report.path
                    ),
                    !report.deleted,
                );
                self.do_knowledge_documents_refresh(rt, ctx);
            }
            Err(error) => {
                self.knowledge_documents_error = Some(error.clone());
                self.set_status(format!("Knowledge delete failed: {}", error), true);
            }
        }

        self.pending_knowledge_delete_promise = None;
    }

    pub fn poll_novel_projects_promise(&mut self) {
        let Some(promise) = &self.pending_novel_projects_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        self.novel_projects_loading = false;
        match result {
            Ok(report) => {
                self.novel_projects_root = report.root.clone();
                self.novel_projects = report.projects.clone();
                self.novel_projects_error = None;
                if self
                    .selected_novel_project_path
                    .as_ref()
                    .map(|selected| {
                        !self
                            .novel_projects
                            .iter()
                            .any(|project| &project.path == selected)
                    })
                    .unwrap_or(true)
                {
                    self.selected_novel_project_path = self
                        .novel_projects
                        .first()
                        .map(|project| project.path.clone());
                }
            }
            Err(error) => {
                self.novel_projects_error = Some(error.clone());
                self.set_status(format!("Novel project list failed: {}", error), true);
            }
        }

        self.pending_novel_projects_promise = None;
    }

    pub fn poll_novel_export_promise(&mut self, rt: &Handle, ctx: &egui::Context) {
        let Some(promise) = &self.pending_novel_export_promise else {
            return;
        };
        let Some(result) = promise.ready() else {
            return;
        };

        self.novel_export_loading = false;
        match result {
            Ok(report) => {
                self.novel_export_error = if report.exported {
                    None
                } else {
                    Some(report.message.clone())
                };
                self.last_novel_export = Some(report.clone());
                self.set_status(
                    if report.exported {
                        format!(
                            "Novel exported: {}",
                            report.output_path.clone().unwrap_or_default()
                        )
                    } else {
                        format!("Novel export did not complete: {}", report.message)
                    },
                    !report.exported,
                );
                self.do_novel_projects_refresh(rt, ctx);
            }
            Err(error) => {
                self.novel_export_error = Some(error.clone());
                self.set_status(format!("Novel export failed: {}", error), true);
            }
        }

        self.pending_novel_export_promise = None;
    }

    pub fn poll_update_promise(&mut self) {
        if let Some(ref promise) = self.update_promise {
            if let Some(result) = promise.ready() {
                self.update_in_progress = false;
                match result {
                    Ok(msg) => {
                        self.update_status = Some(msg.clone());
                        self.set_status("Update command executed".to_string(), false);
                    }
                    Err(e) => {
                        self.update_status = Some(format!("Error: {}", e));
                        self.set_status(format!("Update failed: {}", e), true);
                    }
                }
                self.update_promise = None;
            }
        }
    }

    pub fn do_agent_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.list_agents().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.agent_list_promise = Some(promise);
    }

    pub fn do_log_poll(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::<Vec<String>>::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let lines = client.poll_logs().await.unwrap_or_default();
            sender.send(lines);
            ctx2.request_repaint();
        });
        self.pending_log_promise = Some(promise);
    }

    pub fn do_system_update(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.update_in_progress {
            return;
        }

        self.update_in_progress = true;
        self.update_status = Some("Starting update...".to_string());

        let client = self.client.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.system_update().await.map_err(|e| e.to_string());
            sender.send(res);
        });

        self.update_promise = Some(promise);
    }

    pub fn auto_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        self.poll_all_promises(rt, ctx);
        let now = ctx.input(|i| i.time);

        // 1. Skill list & Snapshot: Every 30s (connected) or 2s (restarting/offline)
        let skill_refresh_interval = if self.connected == Some(true) {
            30.0
        } else {
            2.0
        };
        if now - self.last_skill_refresh_time > skill_refresh_interval
            && self.skills_promise.is_none()
        {
            self.last_skill_refresh_time = now;
            self.trigger_refresh(rt, ctx);
        }

        // 2. Logs: Every 2s if auto_log_poll is on and we are in Logs tab
        const LOG_POLL_INTERVAL: f64 = 2.0;
        if self.auto_log_poll && self.tab == ActiveTab::Logs {
            if now - self.last_log_poll_time > LOG_POLL_INTERVAL {
                self.last_log_poll_time = now;
                self.do_log_poll(rt, ctx);
            }
        }

        // 3. Sandboxes: Every 10s
        const SANDBOX_REFRESH_INTERVAL: f64 = 10.0;
        if now - self.last_sandboxes_refresh_time > SANDBOX_REFRESH_INTERVAL
            && self.sandboxes_promise.is_none()
        {
            self.last_sandboxes_refresh_time = now;
            self.do_sandboxes_refresh(rt, ctx);
        }

        // 4. Approval Queue: Every 5s
        const APPROVAL_REFRESH_INTERVAL: f64 = 5.0;
        if now - self.last_approval_refresh_time > APPROVAL_REFRESH_INTERVAL
            && self.pending_approval_promise.is_none()
        {
            self.last_approval_refresh_time = now;
            self.do_approval_refresh(rt, ctx);
        }

        // 5. Trusted Workspaces: Every 30s
        const WORKSPACE_INTERVAL: f64 = 30.0;
        if now - self.last_workspace_refresh_time > WORKSPACE_INTERVAL
            && self.pending_workspace_promise.is_none()
        {
            self.last_workspace_refresh_time = now;
            self.do_workspace_refresh(rt, ctx);
        }

        // 6. Cron jobs: Every 60s if in Scheduled tab
        const CRON_INTERVAL: f64 = 60.0;
        if self.tab == ActiveTab::Agent
            && self.agent_subtab == AgentSubTab::Tasks
            && self.agent_task_subtab == AgentTaskSubTab::Scheduled
            && now - self.last_cron_refresh_time > CRON_INTERVAL
        {
            self.last_cron_refresh_time = now;
            self.do_cron_refresh(rt, ctx);
        }

        // 8. Metrics: Every 10s
        const METRICS_INTERVAL: f64 = 10.0;
        if now - self.last_metrics_refresh_time > METRICS_INTERVAL {
            self.last_metrics_refresh_time = now;
            self.do_metrics_refresh(rt, ctx);
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    pub fn do_workspace_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_workspace_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.list_workspaces().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.pending_workspace_promise = Some(promise);
    }

    pub fn do_install_skill(&mut self, rt: &Handle, ctx: &egui::Context) {
        let url = self.store_install_url.trim().to_string();
        if url.is_empty() {
            return;
        }

        self.store_installing = true;
        self.store_install_error = None;
        self.store_install_success = None;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let result = match client.install_skill(&url).await {
                Ok(res) => Ok(res),
                Err(e) => Err(e.to_string()),
            };
            sender.send(result);
            ctx2.request_repaint();
        });

        self.pending_install_promise = Some(promise);
    }

    pub fn submit_cron_job(&mut self, rt: &Handle, ctx: &egui::Context) {
        use crate::api::CreateCronJobRequest;

        let name = self.cron_form_name.trim().to_string();
        let prompt = self.cron_form_prompt.trim().to_string();

        if name.is_empty() || prompt.is_empty() {
            self.set_status(
                "Name and Prompt are required to create a job.".to_string(),
                true,
            );
            return;
        }

        let req = CreateCronJobRequest {
            name,
            schedule_kind: self.cron_form_schedule.clone(),
            interval_secs: if self.cron_form_schedule == "every" {
                self.cron_form_interval.parse().ok()
            } else {
                None
            },
            cron_expr: if self.cron_form_schedule == "cron" {
                Some(self.cron_form_expr.clone())
            } else {
                None
            },
            at: None,
            prompt: Some(prompt),
            role: Some(self.cron_form_role.clone()),
        };

        let client = self.client.clone();
        let ctx2 = ctx.clone();

        self.cron_loading = true;
        let (sender, promise) = Promise::new();
        self.pending_cron_action_promise = Some(promise);

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            match client.create_cron_job(req).await {
                Ok(_) => sender.send(Ok("Cron job added successfully.".into())),
                Err(e) => sender.send(Err(e.to_string())),
            }
            ctx2.request_repaint();
        });
    }

    pub fn do_chat_send(&mut self, rt: &Handle, ctx: &egui::Context, session_id: String) {
        let typed_message = self.chat_input.trim().to_string();
        let attachments = self.chat_attachments.clone();
        if typed_message.is_empty() && attachments.is_empty() {
            return;
        }

        let msg = if typed_message.is_empty() {
            "请解析我上传的附件，并根据附件内容回答。".to_string()
        } else {
            typed_message
        };
        let media = attachments
            .iter()
            .map(ChatAttachmentDraft::to_api_media)
            .collect::<Vec<_>>();

        self.push_chat_history_message(
            &session_id,
            "benshu",
            ChatMessage {
                role: "user".to_string(),
                content: msg.clone(),
                agent_name: None,
                reasoning: None,
                tool_calls: Vec::new(),
                artifacts: Vec::new(),
                chat_route: None,
                tool_surface_mode: None,
                runtime_persistence_status: None,
                task_id: None,
                run_id: None,
                trace_id: None,
            },
        );
        self.chat_input.clear();
        self.chat_attachments.clear();
        self.chat_loading = true;
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .chat(msg, None, Some(session_id), media)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.chat_promise = Some(promise);
    }

    pub fn do_load_session_history(
        &mut self,
        rt: &Handle,
        ctx: &egui::Context,
        session_id: String,
    ) {
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        self.chat_loading = true;

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_session_history(&session_id)
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| {
                    serde_json::from_value::<Vec<ChatMessage>>(serde_json::Value::Array(v))
                        .map_err(|e| e.to_string())
                });
            sender.send(res);
            ctx2.request_repaint();
        });

        self.chat_history_promise = Some(promise);
    }

    pub fn do_load_agent(&mut self, rt: &Handle, ctx: &egui::Context) {
        let role = self.agent_role_selected.clone();
        if role.is_empty() {
            return;
        }

        self.agent_role_loaded = false;
        self.agent_role_name = "Loading...".to_string();
        self.agent_role_description = "Loading...".to_string();
        self.agent_role_content = String::new();
        self.agent_role_dirty = false;
        self.agent_role_artifact_policy_dirty = false;
        self.agent_role_provider = String::new();
        self.agent_role_base_url = String::new();
        self.agent_role_model = String::new();
        self.agent_role_local_model_artifact = String::new();
        self.agent_role_local_mmproj_artifact = String::new();
        self.agent_role_local_runtime_family = String::new();
        self.agent_role_temperature = "0.7".to_string();
        self.agent_role_auto_consolidation = false;
        self.agent_role_tools = vec![];
        self.agent_role_pending_tool = String::new();
        self.agent_role_artifact_policy_yaml = String::new();
        self.agent_role_artifact_policy_error = None;
        self.agent_role_artifact_policy_dirty = false;
        self.agent_role_tone = String::new();
        self.agent_role_constraints = vec![];
        self.agent_role_backstory = String::new();
        self.agent_ocean_openness = 0.5;
        self.agent_ocean_conscientiousness = 0.5;
        self.agent_ocean_extraversion = 0.5;
        self.agent_ocean_agreeableness = 0.5;
        self.agent_ocean_neuroticism = 0.5;

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.get_agent(&role).await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.agent_role_promise = Some(promise);
    }

    pub fn update_agent_fields_from_content(
        &mut self,
        runtime: Option<&crate::api::AgentRuntimeConfigDto>,
        artifact_policy: Option<&serde_json::Value>,
    ) {
        // RESET to safe defaults first to avoid 'state bleeding' between agents
        self.agent_role_auto_consolidation = true;
        self.agent_role_temperature = "0.7".to_string();
        self.agent_role_provider = String::new();
        self.agent_role_base_url = String::new();
        self.agent_role_model = String::new();
        self.agent_role_local_model_artifact = String::new();
        self.agent_role_local_mmproj_artifact = String::new();
        self.agent_role_local_runtime_family = String::new();
        self.agent_role_artifact_policy_yaml = String::new();
        self.agent_role_artifact_policy_error = None;
        self.agent_role_artifact_policy_dirty = false;

        let (ovr, body) =
            benshu_brain::config::AgentConfigOverrides::parse_frontmatter(&self.agent_role_content);

        let mut name = ovr.name.clone().unwrap_or_default();
        let mut description = ovr.description.clone().unwrap_or_default();

        if name.is_empty() || description.is_empty() {
            // Fallback to legacy comment parsing
            let lines: Vec<&str> = body.lines().collect();
            for line in lines {
                if line.starts_with("# Name:") && name.is_empty() {
                    name = line.replace("# Name:", "").trim().to_string();
                }
                if line.starts_with("# Description:") && description.is_empty() {
                    description = line.replace("# Description:", "").trim().to_string();
                }
            }
        }

        self.agent_role_name = name;
        self.agent_role_description = description;

        // Sync Core Fields from content
        let (ovr, _) =
            benshu_brain::config::AgentConfigOverrides::parse_frontmatter(&self.agent_role_content);
        let artifact_policy_yaml = artifact_policy
            .cloned()
            .map(|value| {
                benshu_brain::config::AgentConfigOverrides {
                    artifact_policy: Some(value),
                    ..Default::default()
                }
                .artifact_policy_yaml()
            })
            .filter(|yaml| !yaml.trim().is_empty())
            .unwrap_or_else(|| ovr.artifact_policy_yaml());

        self.agent_role_provider = runtime
            .and_then(|cfg| cfg.provider.clone())
            .or(ovr.provider)
            .unwrap_or_default();
        if let Some(base) = runtime
            .and_then(|cfg| cfg.base_url.clone())
            .or(ovr.base_url)
        {
            self.agent_role_base_url = base;
        }
        if let Some(m) = runtime.and_then(|cfg| cfg.model.clone()).or(ovr.model) {
            self.agent_role_model = m;
        }
        if let Some(local_model) = runtime
            .and_then(|cfg| cfg.local_model_artifact.clone())
            .or(ovr.local_model_artifact)
        {
            self.agent_role_local_model_artifact = local_model.clone();
            if self.agent_role_model.trim().is_empty() {
                self.agent_role_model = local_model;
            }
        }
        if let Some(local_mmproj) = runtime
            .and_then(|cfg| cfg.local_mmproj_artifact.clone())
            .or(ovr.local_mmproj_artifact)
        {
            self.agent_role_local_mmproj_artifact = local_mmproj;
        }
        if let Some(local_runtime_family) = runtime
            .and_then(|cfg| cfg.local_runtime_family.clone())
            .or(ovr.local_runtime_family)
        {
            self.agent_role_local_runtime_family = local_runtime_family;
        }
        if let Some(t) = ovr.temperature {
            self.agent_role_temperature = format!("{:.1}", t);
        }
        if let Some(ac) = ovr.auto_consolidation {
            self.agent_role_auto_consolidation = ac;
        }
        if let Some(tools) = ovr.tools {
            self.agent_role_tools = tools;
        }
        self.agent_role_pending_tool = self.agent_role_tools.first().cloned().unwrap_or_default();
        self.agent_role_artifact_policy_yaml = artifact_policy_yaml;
        if let Some(tone) = ovr.tone {
            self.agent_role_tone = tone;
        }
        if let Some(constraints) = ovr.constraints {
            self.agent_role_constraints = constraints;
        }
        if let Some(backstory) = ovr.backstory {
            self.agent_role_backstory = backstory;
        }

        if let Some(traits) = &ovr.traits {
            self.agent_ocean_openness = traits.openness;
            self.agent_ocean_conscientiousness = traits.conscientiousness;
            self.agent_ocean_extraversion = traits.extraversion;
            self.agent_ocean_agreeableness = traits.agreeableness;
            self.agent_ocean_neuroticism = traits.neuroticism;
        }
    }

    pub fn confirm_agent_primary_tool(&mut self) {
        let selected = self.agent_role_pending_tool.trim();
        self.agent_role_tools = if selected.is_empty() {
            Vec::new()
        } else {
            vec![selected.to_string()]
        };
        self.agent_role_dirty = true;
    }

    pub fn update_agent_content_from_fields(&mut self) {
        let (_, body) =
            benshu_brain::config::AgentConfigOverrides::parse_frontmatter(&self.agent_role_content);

        // Ensure body is clean (no duplicate frontmatter if we had any)

        let final_name = if self.agent_role_selected == "benshu" {
            "BenShu".to_string()
        } else {
            self.agent_role_name.clone()
        };
        let is_primary_agent = self.agent_role_selected == "benshu";

        let ovr = benshu_brain::config::AgentConfigOverrides {
            provider: None,
            model: None,
            local_model_artifact: None,
            local_mmproj_artifact: None,
            local_runtime_family: None,
            name: Some(final_name),
            description: Some(self.agent_role_description.clone()),
            temperature: Some(self.agent_role_temperature.parse::<f32>().unwrap_or(0.7)),
            auto_consolidation: if is_primary_agent {
                Some(self.agent_role_auto_consolidation)
            } else {
                None
            },
            base_url: None,
            tools: if self.agent_role_tools.is_empty() {
                None
            } else {
                Some(self.agent_role_tools.clone())
            },
            artifact_policy: None,
            tone: if !is_primary_agent || self.agent_role_tone.is_empty() {
                None
            } else {
                Some(self.agent_role_tone.clone())
            },
            constraints: if !is_primary_agent || self.agent_role_constraints.is_empty() {
                None
            } else {
                Some(self.agent_role_constraints.clone())
            },
            backstory: if !is_primary_agent || self.agent_role_backstory.is_empty() {
                None
            } else {
                Some(self.agent_role_backstory.clone())
            },
            traits: if is_primary_agent {
                Some(benshu_brain::agent::agent_identity::Traits {
                    openness: self.agent_ocean_openness,
                    conscientiousness: self.agent_ocean_conscientiousness,
                    extraversion: self.agent_ocean_extraversion,
                    agreeableness: self.agent_ocean_agreeableness,
                    neuroticism: self.agent_ocean_neuroticism,
                })
            } else {
                None
            },
        };

        self.agent_role_content = ovr.to_markdown(&body);
        self.agent_role_dirty = true;
    }

    pub fn current_agent_artifact_policy(&mut self) -> Option<serde_json::Value> {
        match benshu_brain::config::AgentConfigOverrides::parse_artifact_policy_yaml(
            &self.agent_role_artifact_policy_yaml,
        ) {
            Ok(policy) => {
                self.agent_role_artifact_policy_error = None;
                policy
            }
            Err(err) => {
                self.agent_role_artifact_policy_error = Some(err);
                None
            }
        }
    }

    pub fn do_channel_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.channel_metadata_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx_clone = ctx.clone();
        self.channels_loading = true;
        self.channels_error = None;

        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.get_channel_schema().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx_clone.request_repaint();
        });
        self.channel_metadata_promise = Some(promise);
    }

    pub fn poll_channel_promise(&mut self) {
        if let Some(ref p) = self.channel_metadata_promise {
            if let Some(result) = p.ready() {
                self.channels_loading = false;
                match result {
                    Ok(resp) => {
                        self.channel_metadata = resp.channels.clone();
                        self.running_channels = resp.running.clone();
                        self.channel_observability = resp
                            .observability
                            .iter()
                            .cloned()
                            .map(|entry| (entry.channel_id.clone(), entry))
                            .collect();
                        self.channels = resp.channels.clone();
                        self.channels_error = None;
                    }
                    Err(e) => {
                        self.channel_metadata.clear();
                        self.running_channels.clear();
                        self.channel_observability.clear();
                        self.channels_error = Some(e.clone());
                    }
                }
                self.channel_metadata_promise = None;
                self.pending_channels_promise = None;
            }
        }
    }

    pub fn do_sandboxes_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.sandboxes_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .get_active_sandboxes()
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });
        self.sandboxes_promise = Some(promise);
    }

    pub fn poll_cancel_promise(&mut self, _ctx: &egui::Context) {
        if let Some(promise) = &self.cancel_promise {
            if promise.ready().is_some() {
                self.cancel_promise = None;
                self.set_status("Cancellation command sent".to_string(), false);
            }
        }
    }

    pub fn do_cancel_task(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.cancel_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.cancel_task().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.cancel_promise = Some(promise);
    }

    pub fn do_cancel_chat_task(&mut self, rt: &Handle, ctx: &egui::Context, session_id: String) {
        if self.cancel_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .cancel_session_task(&session_id)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.cancel_promise = Some(promise);
    }

    pub fn do_delete_agent(&mut self, rt: &Handle, ctx: &egui::Context, role: String) {
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let role_for_api = role.clone();
        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client
                .delete_agent(&role_for_api)
                .await
                .map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.chat_histories.remove(&role);
        self.chat_sessions.remove(&role);
        self.agent_save_promise = Some(promise);
        self.do_agent_refresh(rt, ctx);
    }

    pub fn do_a2a_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.get_a2a_summary().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_a2a_promise = Some(promise);
        self.last_a2a_refresh_time = ctx.input(|i| i.time);
    }

    pub fn do_cron_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        if self.pending_cron_promise.is_some() {
            return;
        }

        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.list_cron_jobs().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_cron_promise = Some(promise);
        self.last_cron_refresh_time = ctx.input(|i| i.time);
    }

    pub fn do_metrics_refresh(&mut self, rt: &Handle, ctx: &egui::Context) {
        let client = self.client.clone();
        let ctx2 = ctx.clone();
        let (sender, promise) = Promise::new();

        let rt_handle = rt.clone();
        rt_handle.spawn(async move {
            let res = client.metrics().await.map_err(|e| e.to_string());
            sender.send(res);
            ctx2.request_repaint();
        });

        self.pending_metrics_promise = Some(promise);
    }

    pub fn poll_all_promises(&mut self, rt: &Handle, ctx: &egui::Context) {
        self.poll_approval_promise(rt, ctx);
        self.poll_doctor_promise();
        self.poll_repair_promise();
        self.poll_cron_action_promise();
        self.poll_skills_promise(ctx);
        self.poll_sessions_promise();
        self.poll_session_runtime_tasks_promise();
        self.maybe_start_chat_task_output_backfill(rt, ctx);
        self.poll_task_output_promise();
        self.poll_chat_task_output_backfill_promise();
        self.poll_task_wait_promise();
        self.poll_task_cancel_promise();
        self.poll_session_delegation_promise();
        self.poll_run_trace_promise();
        self.poll_run_replay_promise();
        self.poll_profiler_promise();
        self.poll_profiler_query_promise();
        self.poll_profiler_export_promise();
        self.poll_witness_promise();
        self.poll_witness_bundle_promise();
        self.poll_witness_log_promise();
        self.poll_witness_query_promise();
        self.poll_scorecard_query_promise();
        self.poll_artifact_promise();
        self.poll_artifact_cleanup_promise();
        self.poll_open_target_promise();
        self.poll_runtime_mode_promise();
        self.poll_local_model_stack_promise();
        self.poll_local_model_artifacts_promise();
        self.poll_knowledge_import_promise(rt, ctx);
        self.poll_knowledge_documents_promise(rt, ctx);
        self.poll_knowledge_delete_promise(rt, ctx);
        self.poll_novel_projects_promise();
        self.poll_novel_export_promise(rt, ctx);
        self.poll_runtime_config_promise();
        self.poll_sandbox_promises(rt, ctx);
        self.poll_restore_point_promises();
        self.poll_cron_promise();
        self.poll_provider_promise();
        self.poll_agent_promises(rt, ctx);
        self.poll_chat_promise(rt, ctx);
        self.poll_chat_history_promise();
        self.poll_update_promise();
        self.poll_rollback_promise(ctx);
        self.poll_workspace_promise();
        self.poll_metrics_promise(ctx);
        self.poll_log_promise(ctx);
        self.poll_channel_promise();
        self.poll_a2a_promise();
        self.poll_install_promise(rt, ctx);
        self.poll_cancel_promise(ctx);
        self.poll_agent_import_promise(rt, ctx);
        self.poll_agent_export_promise();
        self.poll_agent_promises(rt, ctx);
    }
}

fn compact_chat_message_for_panel(mut message: ChatMessage) -> ChatMessage {
    message.content = compact_chat_panel_text(&message.content);
    if let Some(reasoning) = message.reasoning.take() {
        message.reasoning = Some(compact_chat_panel_text(&reasoning));
    }
    for tool in &mut message.tool_calls {
        tool.args = compact_chat_panel_text(&tool.args);
        if let Some(result) = tool.result.take() {
            tool.result = Some(compact_chat_panel_text(&result));
        }
    }
    message
}

fn compact_chat_panel_text(text: &str) -> String {
    const PANEL_TEXT_LIMIT: usize = 12_000;
    let count = text.chars().count();
    if count <= PANEL_TEXT_LIMIT {
        return text.to_string();
    }
    let head: String = text.chars().take(8_000).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(2_000)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!(
        "{head}\n\n[Panel display truncated: original message was {count} chars. Long bodies should live in artifacts; use the file path or task artifact preview instead of keeping full text in chat.]\n\n{tail}"
    )
}

fn parse_optional_string(raw: &str) -> Option<String> {
    AppState::parse_optional_string(raw)
}

#[cfg(not(target_arch = "wasm32"))]
fn config_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("benshu-panel")
}

#[cfg(not(target_arch = "wasm32"))]
fn url_config_path() -> std::path::PathBuf {
    config_dir().join("gateway_url.txt")
}

#[cfg(not(target_arch = "wasm32"))]
fn load_saved_url() -> Option<String> {
    std::fs::read_to_string(url_config_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(not(target_arch = "wasm32"))]
fn save_url(url: &str) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(url_config_path(), url);
}

#[cfg(target_arch = "wasm32")]
fn load_saved_url() -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item("benshu_gateway_url").ok()?
}

#[cfg(target_arch = "wasm32")]
fn save_url(url: &str) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            let _ = storage.set_item("benshu_gateway_url", url);
        }
    }
}

pub struct SavedConfig {
    pub tab: ActiveTab,
    pub night_mode: bool,
    pub language: Language,
    pub voice_tts_model: String,
    pub voice_tts_voice: String,
    pub model_ram_limit_gb: u32,
    pub model_vram_limit_gb: u32,
    pub chat_sessions: std::collections::HashMap<String, Vec<String>>,
    pub organ_stt_model: String,
    pub organ_tts_model: String,
    pub organ_embed_model: String,
    pub organ_rerank_model: String,
    pub organ_ocr_model: String,
    pub organ_vision_model: String,
    pub organ_fact_check_model: String,
    pub organ_image_edit_model: String,
    pub organ_audio_understanding_model: String,
    pub organ_realtime_vad_model: String,
    pub organ_duplex_voice_model: String,
    pub organ_local_classifier_model: String,
    pub organ_local_router_model: String,
    pub organ_local_safety_model: String,
    pub fact_check_enabled: bool,
}

pub fn load_saved_config() -> SavedConfig {
    let mut config = SavedConfig {
        tab: ActiveTab::Skills,
        night_mode: true,
        language: Language::En,
        voice_tts_model: "tts-1".to_string(),
        voice_tts_voice: "alloy".to_string(),
        model_ram_limit_gb: 4,
        model_vram_limit_gb: 0,
        chat_sessions: {
            let mut m = std::collections::HashMap::new();
            m.insert("benshu".to_string(), vec!["default".to_string()]);
            m
        },
        organ_stt_model: String::new(),
        organ_tts_model: String::new(),
        organ_embed_model: String::new(),
        organ_rerank_model: String::new(),
        organ_ocr_model: String::new(),
        organ_vision_model: String::new(),
        organ_fact_check_model: String::new(),
        organ_image_edit_model: String::new(),
        organ_audio_understanding_model: String::new(),
        organ_realtime_vad_model: String::new(),
        organ_duplex_voice_model: String::new(),
        organ_local_classifier_model: String::new(),
        organ_local_router_model: String::new(),
        organ_local_safety_model: String::new(),
        fact_check_enabled: true,
    };

    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                if let Ok(Some(tab_str)) = storage.get_item("benshu_active_tab") {
                    config.tab = match tab_str.as_str() {
                        "Skills" => ActiveTab::Skills,
                        "Api" | "Vault" | "Models" => ActiveTab::Models,
                        "Logs" => ActiveTab::Logs,
                        "Agent" => ActiveTab::Agent,
                        "Connection" => ActiveTab::Connection,
                        "Dashboard" => ActiveTab::Dashboard,
                        "System" => ActiveTab::System,
                        "Channels" => ActiveTab::Models,
                        _ => ActiveTab::Skills,
                    };
                }
                if let Ok(Some(mode_str)) = storage.get_item("benshu_night_mode") {
                    config.night_mode = mode_str == "true";
                }
                if let Ok(Some(lang_str)) = storage.get_item("benshu_language") {
                    config.language = if lang_str == "Zh" {
                        Language::Zh
                    } else {
                        Language::En
                    };
                }
                if let Ok(Some(v)) = storage.get_item("benshu_chat_sessions") {
                    if let Ok(sessions) =
                        serde_json::from_str::<std::collections::HashMap<String, Vec<String>>>(&v)
                    {
                        config.chat_sessions = sessions;
                    } else if let Ok(list) = serde_json::from_str::<Vec<String>>(&v) {
                        // Upgrade legacy flat list to 'benshu' default
                        config.chat_sessions.insert("benshu".to_string(), list);
                    }
                }
                if let Ok(Some(v)) = storage.get_item("benshu_fact_check_enabled") {
                    config.fact_check_enabled = v == "true";
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_stt_model") {
                    config.organ_stt_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_tts_model") {
                    config.organ_tts_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_embed_model") {
                    config.organ_embed_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_rerank_model") {
                    config.organ_rerank_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_ocr_model") {
                    config.organ_ocr_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_fact_check_model") {
                    config.organ_fact_check_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_vision_model") {
                    config.organ_vision_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_image_edit_model") {
                    config.organ_image_edit_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_audio_understanding_model") {
                    config.organ_audio_understanding_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_realtime_vad_model") {
                    config.organ_realtime_vad_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_duplex_voice_model") {
                    config.organ_duplex_voice_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_local_classifier_model") {
                    config.organ_local_classifier_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_local_router_model") {
                    config.organ_local_router_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_organ_local_safety_model") {
                    config.organ_local_safety_model = v;
                }
                if let Ok(Some(v)) = storage.get_item("benshu_model_ram_limit_gb") {
                    config.model_ram_limit_gb = v.parse().unwrap_or(4);
                }
                if let Ok(Some(v)) = storage.get_item("benshu_model_vram_limit_gb") {
                    config.model_vram_limit_gb = v.parse().unwrap_or(0);
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = config_dir().join("settings.json");
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(t_str) = val.get("tab").and_then(|t| t.as_str()) {
                    config.tab = match t_str {
                        "Skills" => ActiveTab::Skills,
                        "Api" | "Vault" | "Models" => ActiveTab::Models,
                        "Logs" => ActiveTab::Logs,
                        "Agent" => ActiveTab::Agent,
                        "Connection" => ActiveTab::Connection,
                        "Dashboard" => ActiveTab::Dashboard,
                        "System" => ActiveTab::System,
                        "Channels" => ActiveTab::Models,
                        _ => ActiveTab::Skills,
                    };
                }
                if let Some(m) = val.get("night_mode").and_then(|m| m.as_bool()) {
                    config.night_mode = m;
                }
                if let Some(l) = val.get("language").and_then(|l| l.as_str()) {
                    config.language = if l == "Zh" {
                        Language::Zh
                    } else {
                        Language::En
                    };
                }
                if let Some(v) = val.get("fact_check_enabled").and_then(|v| v.as_bool()) {
                    config.fact_check_enabled = v;
                }
                if let Some(v) = val.get("organ_stt_model").and_then(|v| v.as_str()) {
                    config.organ_stt_model = v.to_string();
                }
                if let Some(v) = val.get("organ_tts_model").and_then(|v| v.as_str()) {
                    config.organ_tts_model = v.to_string();
                }
                if let Some(v) = val.get("organ_embed_model").and_then(|v| v.as_str()) {
                    config.organ_embed_model = v.to_string();
                }
                if let Some(v) = val.get("organ_rerank_model").and_then(|v| v.as_str()) {
                    config.organ_rerank_model = v.to_string();
                }
                if let Some(v) = val.get("organ_ocr_model").and_then(|v| v.as_str()) {
                    config.organ_ocr_model = v.to_string();
                }
                if let Some(v) = val.get("organ_fact_check_model").and_then(|v| v.as_str()) {
                    config.organ_fact_check_model = v.to_string();
                }
                if let Some(v) = val.get("organ_vision_model").and_then(|v| v.as_str()) {
                    config.organ_vision_model = v.to_string();
                }
                if let Some(v) = val.get("organ_image_edit_model").and_then(|v| v.as_str()) {
                    config.organ_image_edit_model = v.to_string();
                }
                if let Some(v) = val
                    .get("organ_audio_understanding_model")
                    .and_then(|v| v.as_str())
                {
                    config.organ_audio_understanding_model = v.to_string();
                }
                if let Some(v) = val.get("organ_realtime_vad_model").and_then(|v| v.as_str()) {
                    config.organ_realtime_vad_model = v.to_string();
                }
                if let Some(v) = val.get("organ_duplex_voice_model").and_then(|v| v.as_str()) {
                    config.organ_duplex_voice_model = v.to_string();
                }
                if let Some(v) = val
                    .get("organ_local_classifier_model")
                    .and_then(|v| v.as_str())
                {
                    config.organ_local_classifier_model = v.to_string();
                }
                if let Some(v) = val.get("organ_local_router_model").and_then(|v| v.as_str()) {
                    config.organ_local_router_model = v.to_string();
                }
                if let Some(v) = val.get("organ_local_safety_model").and_then(|v| v.as_str()) {
                    config.organ_local_safety_model = v.to_string();
                }
                if let Some(v) = val.get("model_ram_limit_gb").and_then(|v| v.as_u64()) {
                    config.model_ram_limit_gb = v as u32;
                }
                if let Some(v) = val.get("model_vram_limit_gb").and_then(|v| v.as_u64()) {
                    config.model_vram_limit_gb = v as u32;
                }
                if let Some(sessions) = val.get("chat_sessions") {
                    if let Ok(m) = serde_json::from_value::<
                        std::collections::HashMap<String, Vec<String>>,
                    >(sessions.clone())
                    {
                        config.chat_sessions = m;
                    } else if let Ok(list) = serde_json::from_value::<Vec<String>>(sessions.clone())
                    {
                        config.chat_sessions.insert("benshu".to_string(), list);
                    }
                }
            }
        }
    }

    config
}

pub fn save_config(state: &AppState) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.set_item("benshu_active_tab", &format!("{:?}", state.tab));
                let _ = storage.set_item(
                    "benshu_night_mode",
                    if state.night_mode { "true" } else { "false" },
                );
                let _ = storage.set_item(
                    "benshu_language",
                    if state.language == Language::Zh {
                        "Zh"
                    } else {
                        "En"
                    },
                );
                let _ = storage.set_item(
                    "benshu_fact_check_enabled",
                    if state.fact_check_enabled {
                        "true"
                    } else {
                        "false"
                    },
                );
                let _ = storage.set_item("benshu_organ_stt_model", &state.organ_stt_model);
                let _ = storage.set_item("benshu_organ_tts_model", &state.organ_tts_model);
                let _ = storage.set_item("benshu_organ_embed_model", &state.organ_embed_model);
                let _ = storage.set_item("benshu_organ_rerank_model", &state.organ_rerank_model);
                let _ = storage.set_item("benshu_organ_ocr_model", &state.organ_ocr_model);
                let _ = storage.set_item(
                    "benshu_organ_fact_check_model",
                    &state.organ_fact_check_model,
                );
                let _ = storage.set_item("benshu_organ_vision_model", &state.organ_vision_model);
                let _ = storage.set_item(
                    "benshu_organ_image_edit_model",
                    &state.organ_image_edit_model,
                );
                let _ = storage.set_item(
                    "benshu_organ_audio_understanding_model",
                    &state.organ_audio_understanding_model,
                );
                let _ = storage.set_item(
                    "benshu_organ_realtime_vad_model",
                    &state.organ_realtime_vad_model,
                );
                let _ = storage.set_item(
                    "benshu_organ_duplex_voice_model",
                    &state.organ_duplex_voice_model,
                );
                let _ = storage.set_item(
                    "benshu_organ_local_classifier_model",
                    &state.organ_local_classifier_model,
                );
                let _ = storage.set_item(
                    "benshu_organ_local_router_model",
                    &state.organ_local_router_model,
                );
                let _ = storage.set_item(
                    "benshu_organ_local_safety_model",
                    &state.organ_local_safety_model,
                );
                let _ = storage.set_item(
                    "benshu_model_ram_limit_gb",
                    &state.model_ram_limit_gb.to_string(),
                );
                let _ = storage.set_item(
                    "benshu_model_vram_limit_gb",
                    &state.model_vram_limit_gb.to_string(),
                );
                if let Ok(json) = serde_json::to_string(&state.chat_sessions) {
                    let _ = storage.set_item("benshu_chat_sessions", &json);
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = config_dir().join("settings.json");
        let dict = serde_json::json!({
            "tab": format!("{:?}", state.tab),
            "night_mode": state.night_mode,
            "language": if state.language == Language::Zh { "Zh" } else { "En" },
            "fact_check_enabled": state.fact_check_enabled,
            "organ_stt_model": state.organ_stt_model,
            "organ_tts_model": state.organ_tts_model,
            "organ_embed_model": state.organ_embed_model,
            "organ_rerank_model": state.organ_rerank_model,
            "organ_ocr_model": state.organ_ocr_model,
            "organ_fact_check_model": state.organ_fact_check_model,
            "organ_vision_model": state.organ_vision_model,
            "organ_image_edit_model": state.organ_image_edit_model,
            "organ_audio_understanding_model": state.organ_audio_understanding_model,
            "organ_realtime_vad_model": state.organ_realtime_vad_model,
            "organ_duplex_voice_model": state.organ_duplex_voice_model,
            "organ_local_classifier_model": state.organ_local_classifier_model,
            "organ_local_router_model": state.organ_local_router_model,
            "organ_local_safety_model": state.organ_local_safety_model,
            "model_ram_limit_gb": state.model_ram_limit_gb,
            "model_vram_limit_gb": state.model_vram_limit_gb,
            "chat_sessions": state.chat_sessions,
        });
        if let Ok(content) = serde_json::to_string_pretty(&dict) {
            let _ = std::fs::create_dir_all(config_dir());
            let _ = std::fs::write(path, content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::api::SessionTaskInfo;
    use poll_promise::Promise;
    use std::collections::HashMap;

    fn ready_promise<T>(value: T) -> Promise<T>
    where
        T: Send + 'static,
    {
        let (sender, promise) = Promise::new();
        sender.send(value);
        promise
    }

    fn sample_trace(witness_id: &str) -> benshu_telemetry::RunTrace {
        serde_json::from_value(serde_json::json!({
            "run_id": "11111111-1111-1111-1111-111111111111",
            "session_id": "22222222-2222-2222-2222-222222222222",
            "agent_id": "benshu",
            "status": "Succeeded",
            "started_at": "2026-03-25T12:00:00Z",
            "finished_at": "2026-03-25T12:00:01Z",
            "task_id": "33333333-3333-3333-3333-333333333333",
            "thread_id": "panel-stage-a-thread",
            "provider": "local",
            "model": "qwen2.5-0.5b-instruct-q4_k_m.gguf",
            "prompt_tokens": 24,
            "completion_tokens": 8,
            "stages": [{
                "stage": "ingress",
                "status": "Succeeded",
                "started_at": "2026-03-25T12:00:00Z",
                "finished_at": "2026-03-25T12:00:01Z",
                "detail": "panel runtime trace",
                "metadata": {}
            }],
            "tools": [],
            "artifacts": [],
            "degradation_notes": [],
            "witness": {
                "witness_id": witness_id,
                "run_id": "11111111-1111-1111-1111-111111111111",
                "verdict": "pass",
                "scorecard": null,
                "replayable": true,
                "benchmark_fingerprint": "panel-stage-a",
                "notes": ["panel witness"]
            },
            "metadata": {}
        }))
        .expect("sample trace json should deserialize")
    }

    fn session_task(trace_id: Option<String>) -> SessionTaskInfo {
        SessionTaskInfo {
            id: "44444444-4444-4444-4444-444444444444".to_string(),
            name: "foreground_chat".to_string(),
            description: "panel stage a task".to_string(),
            status: "completed".to_string(),
            status_detail: None,
            updated_at: "2026-03-25T12:00:02Z".to_string(),
            thread_id: Some("panel-stage-a-thread".to_string()),
            run_id: trace_id.clone(),
            trace_id,
            witness_id: Some("55555555-5555-5555-5555-555555555555".to_string()),
            parent_task_id: None,
            root_task_id: None,
            delegation_request_id: None,
            delegation_state: None,
            delegated_by: None,
            delegated_to: None,
            delegation_return_mode: None,
            artifacts: vec![],
            checkpoints: vec![],
        }
    }

    #[test]
    fn poll_run_trace_promise_populates_witness_projection() {
        let witness_id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let trace = sample_trace(witness_id);
        let mut state = AppState::new(None);
        state.pending_run_trace_promise = Some(ready_promise(Ok(trace.clone())));
        state.selected_run_trace_loading = true;

        state.poll_run_trace_promise();

        assert_eq!(state.selected_run_trace.as_ref(), Some(&trace));
        assert_eq!(state.selected_witness_id.as_deref(), Some(witness_id));
        assert_eq!(
            state.selected_witness_summary.as_ref(),
            trace.witness.as_ref()
        );
        assert!(!state.selected_witness_loading);
        assert!(state.pending_witness_promise.is_none());
    }

    #[test]
    fn poll_session_runtime_tasks_promise_clears_stale_trace_and_witness_selection() {
        let witness_id = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
        let trace = sample_trace(witness_id);
        let mut state = AppState::new(None);
        state.selected_run_trace = Some(trace.clone());
        state.selected_run_trace_id = Some(trace.run_id.to_string());
        state.selected_witness_id = Some(witness_id.to_string());
        state.selected_witness_summary = trace.witness.clone();
        state.selected_run_trace_loading = true;
        state.selected_witness_loading = true;
        state.pending_session_runtime_tasks_promise = Some(ready_promise(Ok(vec![session_task(
            Some("cccccccc-cccc-cccc-cccc-cccccccccccc".to_string()),
        )])));
        state.session_runtime_tasks_loading = true;

        state.poll_session_runtime_tasks_promise();

        assert!(state.selected_run_trace.is_none());
        assert!(state.selected_run_trace_id.is_none());
        assert!(state.selected_witness_summary.is_none());
        assert!(state.selected_witness_id.is_none());
        assert!(!state.selected_run_trace_loading);
        assert!(!state.selected_witness_loading);
        assert!(state.pending_run_trace_promise.is_none());
        assert!(state.pending_witness_promise.is_none());
    }

    #[test]
    fn poll_session_runtime_tasks_promise_preserves_visible_trace_selection() {
        let witness_id = "cccccccc-cccc-cccc-cccc-cccccccccccc";
        let trace = sample_trace(witness_id);
        let trace_id = trace.run_id.to_string();
        let mut state = AppState::new(None);
        state.selected_run_trace = Some(trace.clone());
        state.selected_run_trace_id = Some(trace_id.clone());
        state.selected_witness_id = Some(witness_id.to_string());
        state.selected_witness_summary = trace.witness.clone();
        state.pending_session_runtime_tasks_promise = Some(ready_promise(Ok(vec![session_task(
            Some(trace_id.clone()),
        )])));
        state.session_runtime_tasks_loading = true;

        state.poll_session_runtime_tasks_promise();

        assert_eq!(state.selected_run_trace.as_ref(), Some(&trace));
        assert_eq!(
            state.selected_run_trace_id.as_deref(),
            Some(trace_id.as_str())
        );
        assert_eq!(state.selected_witness_id.as_deref(), Some(witness_id));
        assert_eq!(state.selected_witness_summary, trace.witness);
        assert!(!state.session_runtime_tasks_loading);
        assert!(state.pending_session_runtime_tasks_promise.is_none());
    }

    #[test]
    fn poll_cancel_promise_clears_pending_and_sets_status() {
        let mut state = AppState::new(None);
        state.cancel_promise = Some(ready_promise(Ok(())));

        state.poll_cancel_promise(&egui::Context::default());

        assert!(state.cancel_promise.is_none());
        assert_eq!(
            state.status_msg,
            Some(("Cancellation command sent".to_string(), false))
        );
    }
}
