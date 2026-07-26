use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use benshu_builtin_tools::tool::document_understand::DocumentUnderstandTool;
use benshu_infra::bus::MessageBus;
use benshu_kernel::service::factory::AgentFactory;
use benshu_kernel::KernelRegistry;
use benshu_knowledge::IntentRouter;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Semaphore};

/// App state shared across handlers, backed by the Kernel Registry.
/// Re-exposes only gateway-local state and ingress adapters that still live at
/// the gateway boundary.
#[derive(Clone)]
pub struct AppState {
    pub kernel: Arc<KernelRegistry>,
    pub app_config: Arc<parking_lot::RwLock<benshu_brain::config::AppConfig>>,
    pub factory: Arc<AgentFactory>,

    // --- Gateway-local ingress adapters still used directly by handlers ---
    pub document_router: Arc<DocumentUnderstandTool>,
    pub agent_templates: Vec<benshu_kernel::AgentTemplate>, // Blueprints/Templates for seeding

    // --- Gateway-specific UI/Control state ---
    pub oauth: Arc<benshu_auth::OAuthManager>,
    pub approvals: Arc<crate::api::security::ApprovalManager>,
    pub enabled_tools: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    pub config_path: PathBuf,
    pub log_sender: broadcast::Sender<String>,
    pub connector_trigger: mpsc::UnboundedSender<()>,
    pub log_history: Arc<parking_lot::RwLock<std::collections::VecDeque<String>>>,
    pub running_connectors: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    pub channel_observability:
        Arc<parking_lot::RwLock<std::collections::HashMap<String, ChannelObservability>>>,
    pub cancel_tokens: Arc<dashmap::DashMap<String, tokio_util::sync::CancellationToken>>,
    pub runtime_persist_limiter: Arc<Semaphore>,
    pub bus: Arc<MessageBus>,
    pub internal_key: String,
    pub deployment_mode: crate::LaunchMode,
    pub intent_router: Arc<IntentRouter>,
    pub nlu: Arc<dyn benshu_infra::traits::nlu::NluEngine>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ChannelObservability {
    pub channel_id: String,
    pub inbound_total: u64,
    pub outbound_total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_inbound_session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_observed_at: Option<DateTime<Utc>>,
}

// --- Error Handling ---

pub struct AppError(pub anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Error: {}", self.0),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
