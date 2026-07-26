//! HTTP client for communicating with a running benshu-gw instance.
//! Supports both local (localhost:3000) and remote (Tailscale) endpoints.

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// A single skill as returned by /api/skills
#[derive(Debug, Clone, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub runtime: Option<String>,
    pub homepage: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub dependencies: Vec<String>,
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub fields: Vec<ChannelField>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelObservability {
    pub channel_id: String,
    pub inbound_total: u64,
    pub outbound_total: u64,
    pub last_inbound_session_key: Option<String>,
    pub last_chat_id: Option<String>,
    pub last_thread_id: Option<String>,
    pub last_failure_kind: Option<String>,
    pub last_failure_detail: Option<String>,
    pub last_observed_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DelegationInboxEntry {
    pub message_id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub request_id: Option<String>,
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub task_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub root_task_id: Option<String>,
    pub summary: String,
    pub visible_owner: Option<String>,
    pub memory_owner: Option<String>,
    pub approval_owner: Option<String>,
    pub delegated_by: Option<String>,
    pub delegated_to: Option<String>,
    pub final_response_owner: Option<String>,
    pub return_mode: Option<String>,
    pub delegation_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionDelegationTrace {
    pub session_id: String,
    pub active_role: String,
    pub runtime_profile: Option<String>,
    pub owner_rollup: Option<serde_json::Value>,
    pub inbox: Vec<DelegationInboxEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub description: String,
    pub required: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderCapabilityView {
    pub context_window_tokens: Option<usize>,
    pub supports_vision: bool,
    pub supports_tools: bool,
    pub supports_streaming: bool,
    pub locality: String,
    pub has_fallback: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub fields: Vec<ProviderField>,
    pub capabilities: Vec<String>,
    pub preferred_models: Vec<String>,
    pub capability_view: ProviderCapabilityView,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderSchemaResponse {
    pub providers: Vec<ProviderMetadata>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMediaAttachment {
    pub media_type: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactScope {
    Uploads,
    Workspace,
    Outputs,
    Artifacts,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactLifecycle {
    Ephemeral,
    Session,
    Durable,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    pub scope: ArtifactScope,
    pub lifecycle: ArtifactLifecycle,
    pub created_at: String,
    pub updated_at: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: Option<String>,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub tool_name: Option<String>,
    pub media_type: Option<String>,
    pub virtual_path: Option<String>,
    pub source_kind: String,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ArtifactQuery {
    pub thread_id: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: Option<String>,
    pub scope: Option<String>,
    pub lifecycle: Option<String>,
    pub source_kind: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ArtifactCleanupPolicy {
    pub dry_run: bool,
    pub scope: Option<String>,
    pub source_kind: Option<String>,
    pub ephemeral_max_age_hours: Option<i64>,
    pub session_max_age_hours: Option<i64>,
    pub durable_max_age_days: Option<i64>,
    pub max_delete: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactCleanupReport {
    pub dry_run: bool,
    pub scanned: usize,
    pub matched: usize,
    pub deleted: usize,
    pub kept: usize,
    pub skipped_durable_without_policy: usize,
    pub deleted_artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenArtifactTargetRequest {
    pub artifact_id: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenArtifactTargetResponse {
    pub opened: bool,
    pub target: String,
    pub target_kind: String,
    pub opener: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelSchemaResponse {
    pub channels: Vec<ChannelMetadata>,
    pub running: Vec<String>,
    pub observability: Vec<ChannelObservability>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActiveSandboxContext {
    pub pid: u32,
    pub tool_name: String,
    pub interpreter: String,
    pub started_at: std::time::SystemTime,
    pub sandbox_engine: String,
    pub isolation_state: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityDecisionKind {
    Permit,
    Defer,
    Deny,
}

/// Health check response
#[derive(Debug, Clone, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub agent_count: Option<usize>,
    #[serde(default)]
    pub session_agent_mapping_count: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigUpdateResult {
    pub saved: bool,
    pub main_brain_restart_needed: bool,
    pub windows_ml_restart_needed: bool,
    pub main_brain_restart_requested: bool,
    pub windows_ml_restart_requested: bool,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct ContinuationCacheCleanupRequest {
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryRestorePointFileEntry {
    pub label: String,
    pub relative_path: String,
    pub payload_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRuntimeConfigDto {
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileDto {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<AgentRuntimeConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_policy: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentArtifactPolicyDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_policy: Option<serde_json::Value>,
    pub yaml: String,
    pub source: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryRestorePointManifest {
    pub backup_id: String,
    pub product: String,
    pub contract_version: String,
    pub created_at: String,
    pub storage_root_hint: String,
    pub encryption_key_fingerprint: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub files: Vec<MemoryRestorePointFileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryRestoreReceipt {
    pub receipt_id: String,
    pub backup_id: String,
    pub restored_at: String,
    pub contract_version: String,
    pub encryption_key_fingerprint: String,
    pub restored_files: usize,
    pub restored_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryRestoreDryRunReport {
    pub backup_id: String,
    pub checked_at: String,
    pub contract_version: String,
    pub encryption_key_fingerprint: String,
    pub valid: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub restorable_files: usize,
    pub missing_payloads: Vec<String>,
    pub integrity_mismatches: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryRestorePolicyBasis {
    pub backup_id: String,
    pub decision_kind: String,
    pub policy_basis: String,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryRestoreDeleteReport {
    pub backup_id: String,
    pub deleted_at: String,
    pub dry_run: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub receipt_count: usize,
}

/// Vault secret write request
#[derive(Debug, Serialize)]
pub struct VaultSecretRequest {
    pub key: String,
    pub value: String,
}

/// Metrics response
#[derive(Debug, Clone, Deserialize)]
pub struct Metrics {
    pub total_calls: Option<u64>,
    pub success_rate: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    pub total_tokens: Option<u64>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub host: Option<HostMetrics>,
    pub engram: Option<HybridSearchStats>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HybridSearchStats {
    pub total_documents: u64,
    pub total_vectors: usize,
    pub total_collections: usize,
    pub database_path: String,
    pub fp32_count: usize,
    pub warm_count: usize,
    pub cold_count: usize,
    pub background_count: usize,
    pub last_latency_ms: f32,
    pub acceleration_target: String,
    pub windows_native_embed_outcome: Option<String>,
    pub windows_native_embed_class: Option<String>,
    pub windows_native_embed_provider: Option<String>,
    pub windows_native_embed_device_target: Option<String>,
    pub windows_native_embed_fallback_mode: Option<String>,
    pub windows_native_embed_strategy: Option<String>,
    pub windows_native_embed_note: Option<String>,
    pub windows_native_rerank_outcome: Option<String>,
    pub windows_native_rerank_class: Option<String>,
    pub windows_native_rerank_provider: Option<String>,
    pub windows_native_rerank_device_target: Option<String>,
    pub windows_native_rerank_fallback_mode: Option<String>,
    pub windows_native_rerank_strategy: Option<String>,
    pub windows_native_rerank_note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HostMetrics {
    pub cpu_usage_percent: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub active_agent_processes: usize,
    pub os_name: String,
    pub uptime_secs: u64,
    pub disk_usage_percent: f32,
    pub net_rx_kbps: f32,
    pub net_tx_kbps: f32,
    pub gpu_vram_used_mb: u32,
    pub gpu_vram_total_mb: u32,
    pub gpu_utilization_percent: f32,
    pub suggested_quantization: String,
}

/// Result of a single doctor check
#[derive(Debug, Clone, Deserialize)]
pub struct DoctorCheckResult {
    pub name: String,
    pub success: bool,
    pub message: String,
    pub recommendation: Option<String>,
    pub can_repair: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct A2aSummary {
    pub agents: Vec<String>,
    pub board: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentTemplate {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub temperature: f32,
    pub tools: Vec<String>,
    pub body: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalInfo {
    pub id: String,
    pub tool_name: String,
    pub arguments: String,
    pub challenge_code: String,
    pub decision_kind: SecurityDecisionKind,
    pub policy_basis: String,
    pub escalation_reason: Option<String>,
    pub created_at: String,
    pub trace_id: Option<String>,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalDecisionReceipt {
    pub receipt_id: String,
    pub approval_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub decision_kind: SecurityDecisionKind,
    pub policy_basis: String,
    pub escalation_reason: Option<String>,
    pub policy_reason: Option<String>,
    pub challenge_code: Option<String>,
    pub trace_id: Option<String>,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeMode {
    pub gateway_version: String,
    pub connected: bool,
    pub model_ram_limit_gb: u32,
    pub model_vram_limit_gb: u32,
    pub auto_consolidation_enabled: bool,
    pub enable_global_voice: bool,
    pub enable_local_vision: bool,
    pub local_vision_status: String,
    pub vision_model: String,
    pub image_edit_model: String,
    pub audio_understanding_model: String,
    pub realtime_vad_model: String,
    pub duplex_voice_model: String,
    pub local_classifier_model: String,
    pub local_router_model: String,
    pub local_safety_model: String,
    pub nlu_status: String,
    pub fact_check_status: String,
    pub image_gen_model: String,
    pub image_gen_status: String,
    pub llama_cpp_runtime: LlamaCppRuntime,
    pub windows_ml_runtime: WindowsMlRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlamaCppRuntime {
    #[serde(default = "default_llama_tuning_mode")]
    pub tuning_mode: String,
    #[serde(default = "default_llama_performance_profile")]
    pub performance_profile: String,
    #[serde(default)]
    pub last_recommendation: Option<benshu_inference::runtime::LlamaCppRuntimeRecommendation>,
    #[serde(default)]
    pub effective_diagnostics: Option<benshu_inference::runtime::LlamaCppEffectiveDiagnostics>,
    pub ctx_size: u32,
    pub gpu_layers: u32,
    pub threads: i32,
    pub threads_batch: Option<i32>,
    pub batch_size: u32,
    pub ubatch_size: u32,
    pub parallel_slots: u32,
    #[serde(default)]
    pub cache_ram: Option<u32>,
    #[serde(default)]
    pub ctx_checkpoints: Option<u32>,
    pub flash_attn_mode: String,
    pub kv_offload: bool,
    pub mmap: bool,
    pub mlock: bool,
    pub cache_prompt: bool,
    pub cont_batching: bool,
    pub warmup: bool,
    pub context_shift: bool,
    pub jinja: bool,
    pub rope_scaling: Option<String>,
    pub rope_scale: Option<f32>,
    pub rope_freq_base: Option<f32>,
    pub rope_freq_scale: Option<f32>,
    pub yarn_orig_ctx: Option<u32>,
    pub yarn_ext_factor: Option<f32>,
    pub yarn_attn_factor: Option<f32>,
    pub yarn_beta_slow: Option<f32>,
    pub yarn_beta_fast: Option<f32>,
    pub cache_type_k: Option<String>,
    pub cache_type_v: Option<String>,
    pub device: Option<String>,
    pub split_mode: Option<String>,
    pub tensor_split: Option<String>,
    pub main_gpu: Option<u32>,
    pub fit_mode: String,
    pub fit_target: Option<String>,
    pub fit_ctx: Option<u32>,
    pub cpu_moe: bool,
    pub n_cpu_moe: Option<u32>,
    pub mmproj_offload: bool,
    pub image_min_tokens: Option<u32>,
    pub image_max_tokens: Option<u32>,
    pub reasoning_mode: String,
    pub reasoning_format: String,
    pub reasoning_budget: Option<i32>,
    pub reasoning_budget_message: Option<String>,
    pub sampling_temperature: f32,
    pub sampling_top_k: i32,
    pub sampling_top_p: f32,
    pub sampling_min_p: f32,
    pub sampling_typical_p: f32,
    pub sampling_repeat_penalty: f32,
    pub sampling_presence_penalty: f32,
    pub sampling_frequency_penalty: f32,
    pub sampling_mirostat: i32,
    pub sampling_mirostat_eta: f32,
    pub sampling_mirostat_tau: f32,
    pub seed: Option<i64>,
}

fn default_llama_tuning_mode() -> String {
    benshu_inference::runtime::LLAMA_TUNING_AUTO.to_string()
}

fn default_llama_performance_profile() -> String {
    benshu_inference::runtime::PROFILE_BALANCED.to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct WindowsMlRuntime {
    pub runtime_family: String,
    pub execution_provider_preference: String,
    pub device_target: String,
    pub cpu_fallback_policy: String,
    pub graph_optimization_level: String,
    pub intra_threads: Option<u32>,
    pub inter_threads: Option<u32>,
    pub text_profile: WindowsMlTextProfile,
    pub vision_profile: WindowsMlVisionProfile,
    pub audio_profile: WindowsMlAudioProfile,
    pub image_profile: WindowsMlImageProfile,
    pub realtime_profile: WindowsMlRealtimeProfile,
    pub safety_profile: WindowsMlSafetyProfile,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WindowsMlTextProfile {
    pub batch_size: u32,
    pub max_sequence_length: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WindowsMlVisionProfile {
    pub max_image_side: u32,
    pub resize_policy: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WindowsMlAudioProfile {
    pub sample_rate_hz: u32,
    pub chunk_ms: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WindowsMlImageProfile {
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub guidance: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WindowsMlRealtimeProfile {
    pub vad_window_ms: u32,
    pub duplex_frame_ms: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WindowsMlSafetyProfile {
    pub threshold: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlamaCppCompatibility {
    pub compatibility: String,
    pub note: String,
    pub role_support: Vec<String>,
    pub mmproj_status: String,
    pub current_host_status: String,
    pub current_host_note: String,
    pub server_path: Option<String>,
    pub server_build: Option<u32>,
    pub minimum_supported_build: u32,
    pub server_supported: bool,
    pub server_note: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalModelArtifact {
    pub label: String,
    pub path: String,
    pub relative_path: String,
    pub artifact_kind: String,
    pub size_bytes: u64,
    pub selectable_as_main_brain: bool,
    pub source: String,
    pub factory_id: Option<String>,
    pub declared_roles: Vec<String>,
    pub resolved_mmproj_path: Option<String>,
    pub llama_cpp: LlamaCppCompatibility,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalModelArtifactCatalog {
    pub root: String,
    pub artifacts: Vec<LocalModelArtifact>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalModelRoleBinding {
    pub role: String,
    pub configured_model: String,
    pub source: String,
    pub factory_id: Option<String>,
    pub declared_roles: Vec<String>,
    pub readiness: String,
    pub runtime_profile: String,
    pub product_track: String,
    pub preferred_backend: String,
    pub current_backend: String,
    pub execution_provider: String,
    pub artifact_kind: String,
    pub effective_runtime_state: String,
    pub effective_runtime_reason: String,
    pub effective_runtime_outcome: String,
    pub effective_runtime_class: String,
    pub effective_runtime_failure_reason: Option<String>,
    pub effective_runtime_strategy: String,
    pub windows_native_plan_status: String,
    pub windows_native_plan_note: String,
    pub target_readiness: String,
    pub target_reason: String,
    pub host_validation_status: String,
    pub host_validation_note: String,
    pub fallback_hint: Option<String>,
    pub llama_cpp: LlamaCppCompatibility,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MediaRuntimeSurface {
    pub global_voice_enabled: bool,
    pub local_vision_enabled: bool,
    pub local_vision_status: String,
    pub source_contracts: Vec<String>,
    pub followup_contracts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalModelStack {
    pub host_runtime: String,
    pub deployment_lane: String,
    pub deployment_strategy: String,
    pub deployment_note: String,
    pub product_mainline: String,
    pub validation_tracks: Vec<String>,
    pub windows_native_priority: bool,
    pub small_model_runtime_target: String,
    pub small_model_execution_linked: bool,
    pub small_model_execution_provider: String,
    pub small_model_device_target: String,
    pub small_model_fallback_mode: String,
    pub small_model_runtime_outcome: String,
    pub small_model_runtime_strategy: String,
    pub small_model_runtime_readiness: String,
    pub small_model_runtime_reason: String,
    pub main_brain_runtime_target: String,
    pub model_pool_loaded_count: usize,
    pub model_pool_loaded_models: Vec<String>,
    pub entries: Vec<LocalModelRoleBinding>,
    pub media_runtime: MediaRuntimeSurface,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalModelPoolReport {
    pub action: String,
    pub available: bool,
    pub requested_model_id: Option<String>,
    pub unloaded: bool,
    pub unloaded_count: usize,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeHostRestartReport {
    pub role: String,
    pub control_mode: String,
    pub started: bool,
    pub stdout: String,
    pub stderr: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeImportRequest {
    pub collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KnowledgeImportReport {
    pub collection: String,
    pub imported_count: usize,
    pub skipped_unchanged_count: usize,
    pub skipped_unsupported_count: usize,
    pub skipped_too_large_count: usize,
    pub skipped_missing_count: usize,
    pub failed_count: usize,
    pub imported_paths: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KnowledgeDocumentInfo {
    pub collection: String,
    pub path: String,
    pub title: String,
    pub docid: String,
    pub updated_at_ms: i64,
    pub unverified: bool,
    pub source_url: Option<String>,
    pub import_source: Option<String>,
    pub lifecycle_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KnowledgeDocumentsReport {
    pub collection: Option<String>,
    pub documents: Vec<KnowledgeDocumentInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeDeleteRequest {
    pub collection: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KnowledgeDeleteReport {
    pub collection: String,
    pub path: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NovelProjectInfo {
    pub id: String,
    pub title: String,
    pub path: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub language: String,
    pub genre: String,
    pub target_units: Option<u64>,
    pub chapter_unit_target: Option<u64>,
    pub chapter_count: usize,
    pub approved_chapters: usize,
    pub drafted_chapters: usize,
    pub needs_revision_chapters: usize,
    pub total_units: u64,
    pub latest_export_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NovelProjectsReport {
    pub root: String,
    pub projects: Vec<NovelProjectInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NovelExportRequest {
    pub project_path: String,
    pub format: String,
    pub approved_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NovelExportReport {
    pub exported: bool,
    pub project_path: String,
    pub output_path: Option<String>,
    pub format: String,
    pub chapter_count: usize,
    pub total_units: u64,
    pub message: String,
}

/// The gateway API client.
#[derive(Clone)]
pub struct GatewayClient {
    pub base_url: String,
    pub token: Option<String>,
    client: Client,
}

impl GatewayClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let builder = Client::builder();

        let client = builder.build().expect("Failed to create HTTP client");

        Self {
            base_url: base_url.into(),
            token: None,
            client,
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    fn prepare_request(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(t) = &self.token {
            rb.header("X-API-Key", t)
        } else {
            rb
        }
    }

    async fn handle_response_json<T: serde::de::DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<T> {
        let status = resp.status();
        let url = resp.url().clone();
        let body_text = resp
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;

        if !status.is_success() {
            anyhow::bail!("Request to {} failed (HTTP {}): {}", url, status, body_text);
        }

        serde_json::from_str::<T>(&body_text).map_err(|e| {
            anyhow::anyhow!(
                "JSON decode failed for {}: {}. Raw body: {}",
                url,
                e,
                body_text
            )
        })
    }

    pub async fn list_approvals(&self) -> Result<Vec<ApprovalInfo>> {
        let url = format!("{}/api/approvals/pending", self.base_url);
        let res = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(res).await
    }

    pub async fn list_approval_receipts(&self) -> Result<Vec<ApprovalDecisionReceipt>> {
        let url = format!("{}/api/approvals/receipts", self.base_url);
        let res = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(res).await
    }

    pub async fn list_approval_policy_basis(
        &self,
        approval_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let url = format!(
            "{}/api/approvals/{}/policy-basis",
            self.base_url,
            url_encode(approval_id)
        );
        let res = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(res).await
    }

    pub async fn get_runtime_mode(&self) -> Result<RuntimeMode> {
        let url = format!("{}/api/system/runtime-mode", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn get_local_model_stack(&self) -> Result<LocalModelStack> {
        let url = format!("{}/api/system/local-model-stack", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn get_local_model_artifacts(&self) -> Result<LocalModelArtifactCatalog> {
        let url = format!("{}/api/system/local-model-artifacts", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn unload_local_model_pool_model(
        &self,
        model_id: &str,
    ) -> Result<LocalModelPoolReport> {
        let url = format!("{}/api/system/local-model-pool/unload", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&serde_json::json!({ "model_id": model_id }))
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    pub async fn prune_local_model_pool(&self, idle_seconds: u64) -> Result<LocalModelPoolReport> {
        let url = format!("{}/api/system/local-model-pool/prune", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&serde_json::json!({ "idle_seconds": idle_seconds }))
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    pub async fn clear_local_model_pool(&self) -> Result<LocalModelPoolReport> {
        let url = format!("{}/api/system/local-model-pool/clear", self.base_url);
        let resp = self.prepare_request(self.client.post(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn restart_runtime_host(&self, role: &str) -> Result<RuntimeHostRestartReport> {
        let url = format!(
            "{}/api/system/runtime-hosts/{}/restart",
            self.base_url,
            url_encode(role)
        );
        let resp = self.prepare_request(self.client.post(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn import_knowledge(
        &self,
        request: &KnowledgeImportRequest,
    ) -> Result<KnowledgeImportReport> {
        let url = format!("{}/api/system/knowledge/import", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(request)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    pub async fn list_knowledge_documents(
        &self,
        collection: &str,
    ) -> Result<KnowledgeDocumentsReport> {
        let url = format!(
            "{}/api/system/knowledge/documents?collection={}",
            self.base_url,
            url_encode(collection)
        );
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn delete_knowledge_document(
        &self,
        request: &KnowledgeDeleteRequest,
    ) -> Result<KnowledgeDeleteReport> {
        let url = format!("{}/api/system/knowledge/document/delete", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(request)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    pub async fn list_novel_projects(&self) -> Result<NovelProjectsReport> {
        let url = format!("{}/api/system/writing/novels", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn export_novel_project(
        &self,
        request: &NovelExportRequest,
    ) -> Result<NovelExportReport> {
        let url = format!("{}/api/system/writing/novels/export", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(request)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    pub async fn resolve_approval(&self, id: &str, approved: bool) -> Result<()> {
        let url = format!("{}/api/approvals/{}/decide", self.base_url, id);
        let res = self
            .prepare_request(self.client.post(&url))
            .json(&serde_json::json!({ "approved": approved }))
            .send()
            .await?;
        if res.status().is_success() {
            Ok(())
        } else {
            let body = res.text().await.unwrap_or_default();
            Err(anyhow::anyhow!("Resolution failed: {}", body))
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Check if the gateway is reachable.
    pub async fn health(&self) -> Result<HealthStatus> {
        let url = format!("{}/health", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        Ok(resp.json::<HealthStatus>().await?)
    }

    /// Fetch channel metadata schema
    pub async fn get_channel_schema(&self) -> Result<ChannelSchemaResponse> {
        let url = format!("{}/api/channels/schema", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch LLM provider metadata schema
    pub async fn get_provider_schema(&self) -> Result<ProviderSchemaResponse> {
        let url = format!("{}/api/providers/schema", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        Ok(resp.json::<ProviderSchemaResponse>().await?)
    }

    /// List all skills.
    pub async fn list_skills(&self) -> Result<Vec<SkillInfo>> {
        let url = format!("{}/api/skills", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Toggle a skill on/off.
    pub async fn toggle_skill(&self, name: &str) -> Result<()> {
        let url = format!("{}/api/skills/{}/toggle", self.base_url, name);
        self.prepare_request(self.client.post(&url)).send().await?;
        Ok(())
    }

    /// Uninstall a skill.
    pub async fn uninstall_skill(&self, name: &str) -> Result<()> {
        let url = format!("{}/api/skills/{}", self.base_url, name);
        self.prepare_request(self.client.delete(&url))
            .send()
            .await?;
        Ok(())
    }

    /// List all trusted workspaces.
    pub async fn list_workspaces(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/system/workspaces", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Add a trusted workspace.
    pub async fn add_workspace(&self, path: &str) -> Result<()> {
        let url = format!("{}/api/system/workspaces", self.base_url);
        self.prepare_request(self.client.post(&url))
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await?;
        Ok(())
    }

    /// Remove a trusted workspace.
    pub async fn remove_workspace(&self, path: &str) -> Result<()> {
        let url = format!("{}/api/system/workspaces/remove", self.base_url);
        self.prepare_request(self.client.post(&url))
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await?;
        Ok(())
    }

    /// Save a secret to the vault.
    pub async fn save_vault_secret(&self, key: &str, value: &str) -> Result<()> {
        let url = format!("{}/api/config/vault", self.base_url);
        self.prepare_request(self.client.post(&url))
            .json(&serde_json::json!({ "key": key, "value": value }))
            .send()
            .await?;
        Ok(())
    }

    pub async fn save_channel_config(
        &self,
        channel_id: &str,
        values: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        let url = format!("{}/api/channels/config", self.base_url);
        self.prepare_request(self.client.post(&url))
            .json(&serde_json::json!({ "channel_id": channel_id, "values": values }))
            .send()
            .await?;
        Ok(())
    }

    /// Delete a secret from the vault.
    pub async fn delete_vault_secret(&self, key: &str) -> Result<()> {
        let url = format!("{}/api/config/vault/{}", self.base_url, key);
        self.prepare_request(self.client.delete(&url))
            .send()
            .await?;
        Ok(())
    }

    /// Fetch current metrics.
    pub async fn metrics(&self) -> Result<Metrics> {
        let url = format!("{}/api/metrics", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch recent log lines via SSE (one-shot poll, not streaming).
    /// Returns up to `limit` recent lines by reading the SSE stream for a moment.
    pub async fn poll_logs(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/logs/recent", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        Ok(resp.json::<Vec<String>>().await?)
    }

    /// Fetch agent templates from gateway
    pub async fn get_agent_templates(&self) -> Result<Vec<AgentTemplate>> {
        let url = format!("{}/api/system/agent/templates", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch full gateway configuration as JSON
    pub async fn get_config(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/config", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        Ok(resp.json::<serde_json::Value>().await?)
    }

    /// Update gateway configuration
    pub async fn update_config(&self, config: &serde_json::Value) -> Result<ConfigUpdateResult> {
        let url = format!("{}/api/config", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(config)
            .send()
            .await?;
        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            anyhow::bail!("Config update failed: {}", msg);
        }
        Ok(resp.json::<ConfigUpdateResult>().await?)
    }

    pub async fn get_continuation_runtime_status(&self) -> Result<ContinuationRuntimeStatus> {
        let url = format!("{}/api/runtime/continuation", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn cleanup_continuation_cache(
        &self,
        dry_run: bool,
    ) -> Result<ContinuationCacheCleanupReport> {
        let url = format!("{}/api/runtime/continuation/cache/cleanup", self.base_url);
        let request = ContinuationCacheCleanupRequest { dry_run };
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&request)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    /// Fetch A2A diagnostics summary (agents and shared board)
    pub async fn get_a2a_summary(&self) -> Result<A2aSummary> {
        let url = format!("{}/api/a2a/summary", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        Ok(resp.json::<A2aSummary>().await?)
    }

    /// Set A2A/coordinator throttle limits
    pub async fn set_a2a_throttle(
        &self,
        tenant_id: Option<String>,
        agent_role: Option<String>,
        limit: u32,
    ) -> Result<()> {
        let url = format!("{}/api/a2a/throttle", self.base_url);
        let payload = serde_json::json!({
            "tenant_id": tenant_id,
            "agent_role": agent_role,
            "limit": limit,
        });
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_else(|_| "Unknown error".into());
            anyhow::bail!("Throttle update failed: {}", msg);
        }
        Ok(())
    }
}

// ── New data types for Cron / Sessions / Snapshot ────────────────────────────

/// A scheduled cron job
#[derive(Debug, Clone, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub schedule: serde_json::Value,
    pub payload_kind: String,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub error_count: u32,
}

/// Request body for creating a new cron job
#[derive(Debug, Serialize)]
pub struct CreateCronJobRequest {
    pub name: String,
    pub schedule_kind: String,
    pub interval_secs: Option<u64>,
    pub cron_expr: Option<String>,
    pub at: Option<String>,
    pub prompt: Option<String>,
    pub role: Option<String>,
}

/// An active session
#[derive(Debug, Clone, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub agent_role: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionTaskInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub status_detail: Option<String>,
    pub updated_at: String,
    pub thread_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: Option<String>,
    pub witness_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub root_task_id: Option<String>,
    pub delegation_request_id: Option<String>,
    pub delegation_state: Option<String>,
    pub delegated_by: Option<String>,
    pub delegated_to: Option<String>,
    pub delegation_return_mode: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<TaskArtifactInfo>,
    #[serde(default)]
    pub checkpoints: Vec<TaskCheckpointInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskArtifactInfo {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskCheckpointInfo {
    pub step: u32,
    pub label: String,
    pub recorded_at: String,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TaskWaitRequest {
    pub max_wait_seconds: Option<u64>,
    pub return_on_progress: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskStatusInfo {
    pub task: SessionTaskInfo,
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskWaitInfo {
    pub reason: String,
    pub task: SessionTaskInfo,
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskArtifactPreviewInfo {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    pub media_type: Option<String>,
    pub preview: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskOutputInfo {
    pub task: SessionTaskInfo,
    pub result: Option<serde_json::Value>,
    pub artifact_previews: Vec<TaskArtifactPreviewInfo>,
}

impl GatewayClient {
    /// List cron jobs
    pub async fn list_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let url = format!("{}/api/cron/jobs", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        Ok(resp.json::<Vec<CronJob>>().await?)
    }

    /// Create a cron job
    pub async fn create_cron_job(&self, req: CreateCronJobRequest) -> Result<CronJob> {
        let url = format!("{}/api/cron/jobs", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&req)
            .send()
            .await?;
        Ok(resp.json::<CronJob>().await?)
    }

    /// Delete a cron job
    pub async fn delete_cron_job(&self, id: &str) -> Result<()> {
        let url = format!("{}/api/cron/jobs/{}", self.base_url, id);
        self.prepare_request(self.client.delete(&url))
            .send()
            .await?;
        Ok(())
    }

    /// List active sessions
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let url = format!("{}/api/sessions", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        Ok(resp.json::<Vec<SessionInfo>>().await?)
    }

    /// Fetch chat history for a session
    pub async fn get_session_history(&self, id: &str) -> Result<Vec<serde_json::Value>> {
        let url = format!("{}/api/sessions/{}", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        Ok(resp.json::<Vec<serde_json::Value>>().await?)
    }

    /// Delete one persisted chat session.
    pub async fn delete_session(&self, id: &str) -> Result<()> {
        let url = format!("{}/api/sessions/{}", self.base_url, url_encode(id));
        let resp = self
            .prepare_request(self.client.delete(&url))
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            Err(anyhow::anyhow!("Gateway error ({}): {}", status, txt))
        }
    }

    /// Fetch durable task summaries for a session.
    pub async fn list_session_tasks(&self, id: &str) -> Result<Vec<SessionTaskInfo>> {
        let url = format!("{}/api/sessions/{}/tasks", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch one durable task status by task id.
    pub async fn get_task_status(&self, id: &str) -> Result<TaskStatusInfo> {
        let url = format!("{}/api/tasks/{}/status", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Wait for a durable task to finish or emit a progress change.
    pub async fn wait_task(
        &self,
        id: &str,
        max_wait_seconds: Option<u64>,
        return_on_progress: Option<bool>,
    ) -> Result<TaskWaitInfo> {
        let url = format!("{}/api/tasks/{}/wait", self.base_url, url_encode(id));
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&TaskWaitRequest {
                max_wait_seconds,
                return_on_progress,
            })
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    /// Fetch task result and artifact previews.
    pub async fn get_task_output(
        &self,
        id: &str,
        tail_lines: Option<usize>,
    ) -> Result<TaskOutputInfo> {
        let url = match tail_lines {
            Some(lines) => format!(
                "{}/api/tasks/{}/output?tail_lines={}",
                self.base_url,
                url_encode(id),
                lines
            ),
            None => format!("{}/api/tasks/{}/output", self.base_url, url_encode(id)),
        };
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Cancel one durable task by task id.
    pub async fn cancel_runtime_task(&self, id: &str) -> Result<()> {
        let url = format!("{}/api/tasks/{}/cancel", self.base_url, url_encode(id));
        self.prepare_request(self.client.post(&url)).send().await?;
        Ok(())
    }

    pub async fn get_session_delegation_trace(&self, id: &str) -> Result<SessionDelegationTrace> {
        let url = format!(
            "{}/api/sessions/{}/delegation",
            self.base_url,
            url_encode(id)
        );
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch a structured run trace by `trace_id`.
    pub async fn get_run_trace(&self, id: &str) -> Result<benshu_telemetry::RunTrace> {
        let url = format!("{}/api/traces/{}", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch a replay projection for a structured run trace by `trace_id`.
    pub async fn get_run_replay(&self, id: &str) -> Result<benshu_telemetry::RunReplay> {
        let url = format!("{}/api/traces/{}/replay", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch a profiler artifact for a structured run by `run_id`.
    pub async fn get_run_profiler(&self, id: &str) -> Result<benshu_telemetry::ProfilerArtifact> {
        let url = format!("{}/api/traces/{}/profiler", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch a witness summary by `witness_id`.
    pub async fn get_witness_summary(&self, id: &str) -> Result<benshu_telemetry::WitnessSummary> {
        let url = format!("{}/api/witnesses/{}", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch a full witness bundle by `witness_id`.
    pub async fn get_witness_bundle(&self, id: &str) -> Result<benshu_telemetry::WitnessBundle> {
        let url = format!("{}/api/witnesses/{}/bundle", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Fetch a structured witness log entry by `witness_id`.
    pub async fn get_witness_log(&self, id: &str) -> Result<benshu_telemetry::WitnessLogEntry> {
        let url = format!("{}/api/witnesses/{}/log", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Query structured witness logs.
    pub async fn query_witness_logs(
        &self,
        query: &benshu_telemetry::WitnessLogQuery,
    ) -> Result<Vec<benshu_telemetry::WitnessLogEntry>> {
        let url = format!("{}/api/witness-logs", self.base_url);
        let resp = self
            .prepare_request(self.client.get(&url))
            .query(query)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    /// Query persisted profiler artifacts.
    pub async fn query_profiler_artifacts(
        &self,
        query: &benshu_telemetry::ProfilerArtifactQuery,
    ) -> Result<Vec<benshu_telemetry::ProfilerArtifact>> {
        let url = format!("{}/api/profilers", self.base_url);
        let resp = self
            .prepare_request(self.client.get(&url))
            .query(query)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    /// Export profiler artifacts using the stable P6 schema.
    pub async fn export_profiler_artifacts(
        &self,
        query: &benshu_telemetry::ProfilerArtifactQuery,
    ) -> Result<benshu_telemetry::ProfilerExport> {
        let url = format!("{}/api/profilers/export", self.base_url);
        let resp = self
            .prepare_request(self.client.get(&url))
            .query(query)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    /// List structured artifacts by runtime scope or relationship.
    pub async fn list_artifacts(&self, query: &ArtifactQuery) -> Result<Vec<ArtifactRecord>> {
        let url = format!("{}/api/artifacts", self.base_url);
        let resp = self
            .prepare_request(self.client.get(&url))
            .query(query)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    /// Fetch a single artifact by `artifact_id`.
    pub async fn get_artifact(&self, id: &str) -> Result<ArtifactRecord> {
        let url = format!("{}/api/artifacts/{}", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// Run artifact cleanup using lifecycle-aware policy thresholds.
    pub async fn cleanup_artifacts(
        &self,
        policy: &ArtifactCleanupPolicy,
    ) -> Result<ArtifactCleanupReport> {
        let url = format!("{}/api/artifacts", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(policy)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    /// Ask the gateway to open a safe artifact/file/link with the OS default app.
    pub async fn open_artifact_target(
        &self,
        artifact_id: Option<String>,
        target: Option<String>,
    ) -> Result<OpenArtifactTargetResponse> {
        let url = format!("{}/api/artifacts/open", self.base_url);
        let body = OpenArtifactTargetRequest {
            artifact_id,
            target,
        };
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&body)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    /// Fetch a scorecard by `scorecard_id`.
    pub async fn get_scorecard(&self, id: &str) -> Result<benshu_telemetry::Scorecard> {
        let url = format!("{}/api/scorecards/{}", self.base_url, url_encode(id));
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    /// List all persisted scorecards.
    pub async fn list_scorecards(&self) -> Result<Vec<benshu_telemetry::Scorecard>> {
        self.query_scorecards(&benshu_telemetry::ScorecardQuery::default())
            .await
    }

    /// Query scorecards using structured filters.
    pub async fn query_scorecards(
        &self,
        query: &benshu_telemetry::ScorecardQuery,
    ) -> Result<Vec<benshu_telemetry::Scorecard>> {
        let url = format!("{}/api/scorecards", self.base_url);
        let resp = self
            .prepare_request(self.client.get(&url))
            .query(query)
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    /// Install a skill from a GitHub/raw URL or a local folder containing SKILL.md.
    pub async fn install_skill(&self, url: &str) -> Result<InstallSkillResponse> {
        let endpoint = format!("{}/api/skills/install", self.base_url);
        let body = serde_json::json!({ "url": url });
        let resp = self
            .prepare_request(self.client.post(&endpoint))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            anyhow::bail!("Install failed: {}", msg);
        }
        Ok(resp.json::<InstallSkillResponse>().await?)
    }

    pub async fn get_agent(&self, role: &str) -> Result<FileDto> {
        let url = format!(
            "{}/api/system/agent/detail?role={}",
            self.base_url,
            url_encode(role)
        );
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        if resp.status().is_success() {
            let dto: FileDto = resp.json().await?;
            Ok(dto)
        } else {
            let status = resp.status();
            let txt = resp.text().await?;
            Err(anyhow::anyhow!("Gateway error ({}): {}", status, txt))
        }
    }

    pub async fn put_agent(
        &self,
        role: &str,
        content: String,
        runtime: Option<AgentRuntimeConfigDto>,
        artifact_policy: Option<serde_json::Value>,
    ) -> Result<()> {
        let url = format!(
            "{}/api/system/agent/detail?role={}",
            self.base_url,
            url_encode(role)
        );
        let payload = FileDto {
            content,
            runtime,
            artifact_policy,
        };
        self.prepare_request(self.client.put(&url))
            .json(&payload)
            .send()
            .await?;
        Ok(())
    }

    pub async fn get_agent_artifact_policy(&self, role: &str) -> Result<AgentArtifactPolicyDto> {
        let url = format!(
            "{}/api/system/agent/artifact-policy?role={}",
            self.base_url,
            url_encode(role)
        );
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        if resp.status().is_success() {
            let dto: AgentArtifactPolicyDto = resp.json().await?;
            Ok(dto)
        } else {
            let status = resp.status();
            let txt = resp.text().await?;
            Err(anyhow::anyhow!("Gateway error ({}): {}", status, txt))
        }
    }

    pub async fn put_agent_artifact_policy(
        &self,
        role: &str,
        artifact_policy: Option<serde_json::Value>,
    ) -> Result<()> {
        let url = format!(
            "{}/api/system/agent/artifact-policy?role={}",
            self.base_url,
            url_encode(role)
        );
        let payload = serde_json::json!({
            "artifact_policy": artifact_policy,
        });
        let resp = self
            .prepare_request(self.client.put(&url))
            .json(&payload)
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let txt = resp.text().await?;
            Err(anyhow::anyhow!("Gateway error ({}): {}", status, txt))
        }
    }

    pub async fn delete_agent(&self, role: &str) -> Result<()> {
        let url = format!(
            "{}/api/system/agent/delete?role={}",
            self.base_url,
            url_encode(role)
        );
        self.prepare_request(self.client.delete(&url))
            .send()
            .await?;
        Ok(())
    }

    pub async fn list_agents(&self) -> Result<Vec<AgentSummary>> {
        let url = format!("{}/api/system/agents", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn export_agent(&self, role: &str, limit: usize) -> Result<String> {
        let url = format!(
            "{}/api/system/agent/export?role={}",
            self.base_url,
            url_encode(role)
        );
        let payload = serde_json::json!({ "limit": limit });
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            anyhow::bail!("Export failed: {}", msg);
        }
        Ok(resp.text().await?)
    }

    pub async fn import_vessel(&self, vessel_json: String) -> Result<()> {
        let url = format!("{}/api/system/agents", self.base_url);
        let payload = serde_json::json!({ "vessel_json": vessel_json });
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            anyhow::bail!("Import failed: {}", msg);
        }
        Ok(())
    }

    pub async fn create_memory_restore_point(&self) -> Result<MemoryRestorePointManifest> {
        let url = format!("{}/api/system/memory/restore-points", self.base_url);
        let resp = self.prepare_request(self.client.post(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn list_memory_restore_points(&self) -> Result<Vec<MemoryRestorePointManifest>> {
        let url = format!("{}/api/system/memory/restore-points", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn inspect_memory_restore_point(
        &self,
        backup_id: &str,
    ) -> Result<MemoryRestorePointManifest> {
        let url = format!(
            "{}/api/system/memory/restore-point?backup_id={}",
            self.base_url,
            url_encode(backup_id)
        );
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn restore_memory_restore_point(
        &self,
        backup_id: &str,
    ) -> Result<MemoryRestoreReceipt> {
        let url = format!("{}/api/system/memory/restore-point/restore", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&serde_json::json!({ "backup_id": backup_id }))
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    pub async fn delete_memory_restore_point(
        &self,
        backup_id: &str,
        dry_run: bool,
    ) -> Result<MemoryRestoreDeleteReport> {
        let url = format!("{}/api/system/memory/restore-point/delete", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&serde_json::json!({ "backup_id": backup_id, "dry_run": dry_run }))
            .send()
            .await?;
        self.handle_response_json(resp).await
    }

    pub async fn dry_run_memory_restore_point(
        &self,
        backup_id: &str,
    ) -> Result<MemoryRestoreDryRunReport> {
        let url = format!(
            "{}/api/system/memory/restore-point/dry-run?backup_id={}",
            self.base_url,
            url_encode(backup_id)
        );
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn explain_memory_restore_policy(
        &self,
        backup_id: &str,
    ) -> Result<MemoryRestorePolicyBasis> {
        let url = format!(
            "{}/api/system/memory/restore-point/policy?backup_id={}",
            self.base_url,
            url_encode(backup_id)
        );
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn list_memory_restore_receipts(
        &self,
        backup_id: &str,
    ) -> Result<Vec<MemoryRestoreReceipt>> {
        let url = format!(
            "{}/api/system/memory/restore-point/receipts?backup_id={}",
            self.base_url,
            url_encode(backup_id)
        );
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn inspect_memory_restore_receipt(
        &self,
        backup_id: &str,
        receipt_id: &str,
    ) -> Result<MemoryRestoreReceipt> {
        let url = format!(
            "{}/api/system/memory/restore-point/receipt?backup_id={}&receipt_id={}",
            self.base_url,
            url_encode(backup_id),
            url_encode(receipt_id)
        );
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        self.handle_response_json(resp).await
    }

    pub async fn chat(
        &self,
        message: String,
        role: Option<String>,
        session_id: Option<String>,
        media: Vec<ChatMediaAttachment>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/api/chat", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&serde_json::json!({
                "message": message,
                "role": role,
                "session_id": session_id,
                "media": media
            }))
            .send()
            .await?;

        Ok(resp.json::<ChatResponse>().await?)
    }

    pub async fn chat_stream(
        &self,
        message: String,
        role: Option<String>,
        session_id: Option<String>,
        media: Vec<ChatMediaAttachment>,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/api/chat/stream", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&serde_json::json!({
                "message": message,
                "role": role,
                "session_id": session_id,
                "media": media
            }))
            .send()
            .await?;

        Ok(resp)
    }

    pub async fn rollback(&self, original_path: String, backup_path: String) -> Result<()> {
        let url = format!("{}/api/system/rollback", self.base_url);
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&serde_json::json!({
                "original_path": original_path,
                "backup_path": backup_path
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            anyhow::bail!("Rollback failed: {}", msg);
        }
        Ok(())
    }

    /// Run diagnostic checks on the gateway.
    pub async fn doctor_check(&self) -> Result<Vec<DoctorCheckResult>> {
        let url = format!("{}/api/system/doctor", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        if resp.status().is_success() {
            let res = resp.json().await?;
            Ok(res)
        } else {
            let text = resp.text().await?;
            Err(anyhow::anyhow!("Doctor check failed: {}", text))
        }
    }

    pub async fn repair_system(&self, name: &str) -> Result<String> {
        let url = format!("{}/api/system/repair", self.base_url);
        let body = serde_json::json!({ "name": name });
        let resp = self
            .prepare_request(self.client.post(&url))
            .json(&body)
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(resp.json::<String>().await?)
        } else {
            let text = resp.text().await?;
            Err(anyhow::anyhow!("Repair failed: {}", text))
        }
    }

    pub async fn get_active_sandboxes(&self) -> Result<Vec<ActiveSandboxContext>> {
        let url = format!("{}/api/system/sandboxes", self.base_url);
        let resp = self.prepare_request(self.client.get(&url)).send().await?;
        if resp.status().is_success() {
            let res = resp.json().await?;
            Ok(res)
        } else {
            let text = resp.text().await?;
            Err(anyhow::anyhow!("Failed to fetch sandboxes: {}", text))
        }
    }

    /// Request the gateway to shut down.
    pub async fn shutdown_gateway(&self) -> Result<()> {
        let url = format!("{}/api/system/shutdown", self.base_url);
        self.prepare_request(self.client.post(&url)).send().await?;
        Ok(())
    }

    // ── Task Cancellation API (Phase 11-B) ───────────────────────────

    /// Send a cancel signal to abort all active agent tasks.
    pub async fn cancel_task(&self) -> Result<()> {
        let url = format!("{}/api/cancel", self.base_url);
        self.prepare_request(self.client.post(&url)).send().await?;
        Ok(())
    }

    /// Stop the current foreground task for a specific chat session.
    pub async fn cancel_session_task(&self, session_id: &str) -> Result<()> {
        let url = format!(
            "{}/api/sessions/{}/cancel",
            self.base_url,
            url_encode(session_id)
        );
        self.prepare_request(self.client.post(&url)).send().await?;
        Ok(())
    }

    // ── System Update API ──────────────────────────────────────────────────

    pub async fn system_update(&self) -> Result<String> {
        let url = format!("{}/api/system/update", self.base_url);
        let resp = self.prepare_request(self.client.post(&url)).send().await?;
        if resp.status().is_success() {
            Ok(resp.text().await?)
        } else {
            let msg = resp.text().await.unwrap_or_default();
            anyhow::bail!("Update failed: {}", msg);
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatResponse {
    pub response: String,
    pub reasoning: Option<String>,
    pub tool_calls: Option<Vec<ToolCallTrace>>,
    #[serde(default)]
    pub artifacts: Vec<ChatArtifactRef>,
    #[serde(default)]
    pub chat_route: Option<String>,
    #[serde(default)]
    pub tool_surface_mode: Option<String>,
    #[serde(default)]
    pub runtime_persistence_status: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatArtifactRef {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    pub media_type: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatStreamEvent {
    Accepted { session_id: Option<String> },
    Status { text: String },
    Artifact { artifact: ChatArtifactRef },
    Final { response: ChatResponse },
    Error { message: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackupInfo {
    pub original_path: String,
    pub backup_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCallTrace {
    pub name: String,
    pub args: String,
    pub result: Option<String>,
    pub backup: Option<BackupInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentSummary {
    pub id: String,
    pub name: Option<String>,
}

/// Response from POST /api/skills/install
#[derive(Debug, Clone, Deserialize)]
pub struct InstallSkillResponse {
    pub success: bool,
    pub skill_name: String,
    pub message: String,
}

// ... unchanged urlencoding ...
fn url_encode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                vec![c]
            } else {
                let encoded = format!("%{:02X}", c as u32);
                encoded.chars().collect::<Vec<_>>()
            }
        })
        .collect()
}
