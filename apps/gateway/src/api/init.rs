use anyhow::Result;
use axum::{
    http::Method,
    middleware,
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tracing::{error, info, warn};

use benshu_brain::config::{AgentConfigOverrides, TelegramConfig};
use benshu_connectors::{Connector, TelegramConfig as ConnectorTelegramConfig, TelegramConnector};
use benshu_kernel::service::factory::AgentFactory;
use benshu_kernel::KernelRegistry;

use crate::api::bridge::AgentBridge;
use crate::api::handlers::{
    agent, artifact, auth, chat, config, connector, cron, experience, logs, metrics, security,
    skill, system, vault, workspace, writing,
};
use crate::api::middleware::api_guard;
use crate::api::state::AppState;
use benshu_builtin_tools::tool::document_understand::DocumentUnderstandTool;

fn same_local_model_path(lhs: &std::path::Path, rhs: &std::path::Path) -> bool {
    let lhs_canonical = std::fs::canonicalize(lhs).ok();
    let rhs_canonical = std::fs::canonicalize(rhs).ok();
    match (lhs_canonical, rhs_canonical) {
        (Some(lhs), Some(rhs)) => lhs == rhs,
        _ => lhs == rhs,
    }
}

fn gateway_should_defer_visual_ingress_to_prime_multimodal(
    app_config: &parking_lot::RwLock<benshu_brain::config::AppConfig>,
    config_path: &std::path::Path,
) -> bool {
    let config = app_config.read();
    if !config.sensory.enable_local_vision {
        return false;
    }

    let Some(vision_model) = config.sensory.vision_model.as_deref() else {
        return false;
    };
    let trimmed = vision_model.trim();
    if trimmed.is_empty() || trimmed.starts_with("api:") {
        return false;
    }
    let vision_model_path = PathBuf::from(trimmed);

    let base_agent_path = config.agent_path.clone().unwrap_or_else(|| {
        config_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("agents")
    });
    drop(config);

    let prime_agent_file = base_agent_path.join("benshu").join("AGENT.md");
    let Ok(content) = std::fs::read_to_string(prime_agent_file) else {
        return false;
    };
    let (file_overrides, _) = AgentConfigOverrides::parse_frontmatter(&content);
    let overrides = app_config
        .read()
        .apply_hidden_agent_overrides("benshu", file_overrides);
    let Some(agent_model) = overrides.model else {
        return false;
    };
    let agent_model_path = PathBuf::from(agent_model.trim());
    if same_local_model_path(&agent_model_path, &vision_model_path) {
        return true;
    }

    false
}

/// Fully Kernel-Integrated Gateway Bootstrapper
#[allow(clippy::too_many_arguments)]
pub async fn start_server(
    kernel: Arc<KernelRegistry>,
    factory: Arc<AgentFactory>,
    oauth: Arc<benshu_auth::OAuthManager>,
    app_config: Arc<parking_lot::RwLock<benshu_brain::config::AppConfig>>,
    enabled_tools: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    config_path: PathBuf,
    log_sender: broadcast::Sender<String>,
    internal_key: String,
    deployment_mode: crate::LaunchMode,
) -> Result<()> {
    info!("🚀 Core Kernel Boot completed. Initializing Gateway API...");

    // 1. Session & Guard Initialization
    let shutdown_token = tokio_util::sync::CancellationToken::new();
    if matches!(deployment_mode, crate::LaunchMode::Embedded) {
        let s_token_clone = shutdown_token.clone();
        let ss_clone = kernel.security().clone();
        tokio::spawn(async move {
            ss_clone.pid_guard.watch_parent().await;
            s_token_clone.cancel();
        });
    }

    // 1.2 Log Aggregator (Move up to capture early startup logs)
    let log_history = Arc::new(parking_lot::RwLock::new(
        std::collections::VecDeque::with_capacity(300),
    ));
    let log_history_clone = log_history.clone();
    let mut log_rx = log_sender.subscribe();
    tokio::spawn(async move {
        while let Ok(msg) = log_rx.recv().await {
            let mut history = log_history_clone.write();
            if history.len() >= 300 {
                history.pop_front();
            }
            history.push_back(msg);
        }
    });

    // 1.5 Swarm Initialization
    // Only the prime agent is loaded eagerly. Other roles become lazy worker blueprints.
    {
        let base_agent_path = {
            let config = app_config.read();
            let base_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
            config
                .agent_path
                .clone()
                .unwrap_or_else(|| base_dir.join("agents"))
        };

        if let Ok(mut entries) = tokio::fs::read_dir(&base_agent_path).await {
            info!(
                "🌟 Initializing Swarm from {}...",
                base_agent_path.display()
            );
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.path().is_dir() {
                    let role_name = entry.file_name().to_string_lossy().to_string();
                    if role_name.eq_ignore_ascii_case("benshu") {
                        if let Err(e) = factory.reload_agent(&role_name).await {
                            tracing::error!(
                                "Failed to auto-load prime agent '{}': {}",
                                role_name,
                                e
                            );
                        } else {
                            info!("  ✓ Prime agent '{}' is now online", role_name);
                        }
                    } else if let Err(e) = factory.load_worker_blueprint(&role_name).await {
                        tracing::error!("Failed to load worker blueprint '{}': {}", role_name, e);
                    } else {
                        info!(
                            "  ◌ Worker blueprint '{}' is ready for lazy spawn",
                            role_name
                        );
                    }
                }
            }
        }
    }

    // 2. Gateway Control Structures
    let approval_manager = Arc::new(crate::api::security::ApprovalManager::new());
    let approval_handler = Arc::new(crate::api::security::GatewayApprovalHandler::new(
        approval_manager.clone(),
    ));
    let (connector_trigger, trigger_rx) = mpsc::unbounded_channel::<()>();

    // Inject approvals into kernel services
    let _ = kernel
        .coordinator()
        .approval_handler
        .set(approval_handler.clone());
    kernel.skill_loader().set_approval_handler(approval_handler);
    if let Err(e) = kernel.skill_loader().load_all().await {
        tracing::warn!("Failed to load skills during gateway init: {}", e);
    }

    // 4. Final State Construction
    let document_router = Arc::new(
        DocumentUnderstandTool::new(None, None, kernel.sensory().clone())
            .with_prime_multimodal_visual_ingress(
                gateway_should_defer_visual_ingress_to_prime_multimodal(
                    app_config.as_ref(),
                    &config_path,
                ),
            ),
    );

    let state = AppState {
        document_router: document_router.clone(),
        agent_templates: vec![], // Handle seeded agents as virtual templates

        kernel: kernel.clone(),
        app_config,
        factory: factory.clone(),
        oauth,
        approvals: approval_manager.clone(),
        enabled_tools,
        config_path,
        log_sender,
        connector_trigger,
        log_history,
        running_connectors: Arc::new(parking_lot::RwLock::new(std::collections::HashSet::new())),
        channel_observability: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
        cancel_tokens: Arc::new(dashmap::DashMap::new()),
        runtime_persist_limiter: Arc::new(tokio::sync::Semaphore::new(8)),
        bus: Arc::new(benshu_infra::bus::MessageBus::new(100)),
        internal_key,
        deployment_mode,
        intent_router: Arc::new(benshu_knowledge::IntentRouter::new()),
        nlu: kernel.nlu().clone(),
    };
    chat::resume_provider_paused_durable_tasks_after_gateway_restart(&state);

    // 5. Routing Definitions (Full Gateway API Surface)
    let app = Router::new()
        .route("/health", get(system::health_check))
        .route("/api/chat", post(chat::chat_handler))
        .route("/api/chat/stream", post(chat::chat_stream_handler))
        .route("/api/skills", get(skill::list_skills))
        .route("/api/skills/install", post(skill::install_skill))
        .route("/api/skills/{name}/run", post(skill::run_skill))
        .route("/api/skills/{name}/toggle", post(skill::toggle_skill))
        .route("/api/skills/{name}", delete(skill::uninstall_skill))
        .route("/api/providers/schema", get(skill::get_provider_schema))
        .route(
            "/api/config",
            get(config::get_config).post(config::update_config),
        )
        .route(
            "/api/runtime/continuation",
            get(config::get_continuation_runtime_status),
        )
        .route(
            "/api/runtime/continuation/cache/cleanup",
            post(config::cleanup_continuation_cache),
        )
        .route("/api/config/vault", post(vault::save_vault_secret))
        .route(
            "/api/config/vault/{key}",
            delete(vault::delete_vault_secret),
        )
        .route("/api/system/rollback", post(system::rollback_handler))
        .route(
            "/api/system/memory/restore-points",
            get(system::list_memory_restore_points).post(system::create_memory_restore_point),
        )
        .route(
            "/api/system/memory/restore-point",
            get(system::inspect_memory_restore_point),
        )
        .route(
            "/api/system/memory/restore-point/dry-run",
            get(system::dry_run_memory_restore_point),
        )
        .route(
            "/api/system/memory/restore-point/policy",
            get(system::explain_memory_restore_policy),
        )
        .route(
            "/api/system/memory/restore-point/restore",
            post(system::restore_memory_restore_point),
        )
        .route(
            "/api/system/memory/restore-point/delete",
            post(system::delete_memory_restore_point),
        )
        .route(
            "/api/system/memory/restore-point/receipts",
            get(system::list_memory_restore_receipts),
        )
        .route(
            "/api/system/memory/restore-point/receipt",
            get(system::inspect_memory_restore_receipt),
        )
        .route(
            "/api/system/agents",
            get(agent::list_agents).post(agent::import_agent),
        )
        .route(
            "/api/system/agent/detail",
            get(agent::get_agent).put(agent::put_agent),
        )
        .route(
            "/api/system/agent/artifact-policy",
            get(agent::get_agent_artifact_policy).put(agent::put_agent_artifact_policy),
        )
        .route("/api/system/agent/delete", delete(agent::delete_agent))
        .route("/api/system/agent/export", post(agent::export_agent))
        .route(
            "/api/system/agent/templates",
            get(agent::get_agent_identity_templates),
        )
        .route(
            "/api/system/workspaces",
            get(workspace::list_workspaces).post(workspace::add_workspace),
        )
        .route(
            "/api/system/workspaces/remove",
            post(workspace::remove_workspace),
        )
        .route("/api/logs/recent", get(logs::get_recent_logs))
        .route("/api/metrics", get(metrics::metrics_handler))
        .route(
            "/api/cron/jobs",
            get(cron::list_cron_jobs).post(cron::create_cron_job),
        )
        .route("/api/cron/jobs/{id}", delete(cron::delete_cron_job))
        .route("/api/sessions", get(chat::list_sessions))
        .route(
            "/api/sessions/{id}",
            get(chat::get_session_history).delete(chat::delete_session),
        )
        .route("/api/sessions/{id}/cancel", post(chat::cancel_session))
        .route("/api/sessions/{id}/tasks", get(chat::list_session_tasks))
        .route("/api/tasks/{id}/status", get(chat::get_task_status))
        .route("/api/tasks/{id}/wait", post(chat::wait_task))
        .route("/api/tasks/{id}/output", get(chat::get_task_output))
        .route("/api/tasks/{id}/cancel", post(chat::cancel_task))
        .route("/api/experiences/stats", get(experience::stats))
        .route(
            "/api/experiences",
            get(experience::list).post(experience::create),
        )
        .route("/api/experiences/query", post(experience::query))
        .route(
            "/api/experiences/{id}",
            get(experience::get).delete(experience::delete),
        )
        .route(
            "/api/experiences/{id}/selected",
            post(experience::mark_selected),
        )
        .route(
            "/api/experiences/{id}/preflight",
            post(experience::record_preflight),
        )
        .route(
            "/api/experiences/{id}/task-result",
            post(experience::record_task_result),
        )
        .route(
            "/api/experiences/{id}/projection",
            get(experience::projection),
        )
        .route(
            "/api/sessions/{id}/delegation",
            get(chat::get_session_delegation_trace),
        )
        .route("/api/traces/{id}", get(chat::get_run_trace))
        .route("/api/traces/{id}/replay", get(chat::get_run_replay))
        .route("/api/traces/{id}/profiler", get(chat::get_run_profiler))
        .route("/api/witnesses/{id}", get(chat::get_witness_summary))
        .route("/api/witnesses/{id}/bundle", get(chat::get_witness_bundle))
        .route("/api/witnesses/{id}/log", get(chat::get_witness_log))
        .route("/api/witness-logs", get(chat::query_witness_logs))
        .route("/api/profilers", get(chat::query_profiler_artifacts))
        .route(
            "/api/profilers/export",
            get(chat::export_profiler_artifacts),
        )
        .route(
            "/api/artifacts",
            get(artifact::list_artifacts).post(artifact::cleanup_artifacts),
        )
        .route("/api/artifacts/open", post(artifact::open_artifact_target))
        .route("/api/artifacts/{id}", get(artifact::get_artifact))
        .route("/api/scorecards", get(chat::list_scorecards))
        .route("/api/scorecards/{id}", get(chat::get_scorecard))
        .route("/api/cancel", post(system::cancel_handler))
        .route("/api/system/doctor", get(system::doctor_api_handler))
        .route("/api/system/repair", post(system::repair_api_handler))
        .route("/api/system/snapshot", get(system::gateway_snapshot))
        .route("/api/system/runtime-mode", get(system::runtime_mode))
        .route(
            "/api/system/local-model-stack",
            get(system::local_model_stack),
        )
        .route(
            "/api/system/local-model-artifacts",
            get(system::local_model_artifacts),
        )
        .route(
            "/api/system/local-model-pool/unload",
            post(system::local_model_pool_unload),
        )
        .route(
            "/api/system/local-model-pool/prune",
            post(system::local_model_pool_prune),
        )
        .route(
            "/api/system/local-model-pool/clear",
            post(system::local_model_pool_clear),
        )
        .route(
            "/api/system/runtime-hosts/{role}/restart",
            post(system::restart_runtime_host),
        )
        .route(
            "/api/system/knowledge/import",
            post(system::import_knowledge),
        )
        .route(
            "/api/system/knowledge/documents",
            get(system::list_knowledge_documents),
        )
        .route(
            "/api/system/knowledge/document/delete",
            post(system::delete_knowledge_document),
        )
        .route(
            "/api/system/writing/novels",
            get(writing::list_novel_projects),
        )
        .route(
            "/api/system/writing/novels/export",
            post(writing::export_novel_project),
        )
        .route("/api/system/shutdown", post(system::shutdown_handler))
        .route("/api/system/update", post(system::system_update_handler))
        .route("/api/channels/schema", get(connector::get_channel_schema))
        .route("/api/channels/config", post(connector::save_channel_config))
        .route(
            "/api/channels/webhook/{id}",
            post(connector::webhook_handler),
        )
        .route("/api/swarm/summary", get(system::swarm_summary))
        .route("/api/swarm/throttle", post(system::set_swarm_throttle))
        .route("/api/a2a/summary", get(system::swarm_summary))
        .route("/api/a2a/throttle", post(system::set_swarm_throttle))
        .route(
            "/api/auth/init/{provider}",
            get(auth::auth_initiate_handler),
        )
        .route(
            "/api/auth/callback/{provider}",
            get(auth::auth_callback_handler),
        )
        .route("/api/approvals/pending", get(security::list_approvals))
        .route(
            "/api/approvals/receipts",
            get(security::list_approval_receipts),
        )
        .route(
            "/api/approvals/receipts/{id}",
            get(security::get_approval_receipt),
        )
        .route(
            "/api/approvals/{id}/policy-basis",
            get(security::list_approval_policy_basis),
        )
        .route(
            "/api/approvals/{id}/decide",
            post(security::resolve_approval),
        )
        .route("/api/system/sandboxes", get(security::get_active_sandboxes))
        .route(
            "/api/system/sandboxes/{pid}/kill",
            post(security::kill_sandbox),
        )
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                .allow_headers(Any),
        )
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), api_guard))
        .with_state(state.clone());

    // 6. Bridge & Worker Spawning
    // Always start the AgentBridge once the prime agent is online.
    if kernel
        .coordinator()
        .get(&kernel.coordinator().primary_role())
        .is_some()
    {
        let bridge = Arc::new(AgentBridge::new(
            kernel.coordinator().clone(),
            kernel.skill_loader().clone(),
            state.bus.clone(),
            state.channel_observability.clone(),
            document_router,
            Arc::new(crate::api::bridge::EngramSessionStore::new(
                kernel.search_engine().engram_store(),
            )),
            approval_manager,
        ));
        let b1 = bridge.clone();
        tokio::spawn(async move {
            b1.start().await;
        });
    }

    let connector_state = state.clone();
    tokio::spawn(async move {
        supervise_connectors(connector_state, trigger_rx).await;
    });

    // 7. Final Bind & Serve
    let (host, port) = {
        let config = state.app_config.read();
        (config.server.host.clone(), config.server.port)
    };
    let bind_addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    let local_addr = listener.local_addr()?;
    info!(
        "🚀 Gateway ({:?}) bound to http://{}",
        state.deployment_mode, local_addr
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_token.cancelled().await;
        info!("Shutting down Gateway...");
    })
    .await
    .map_err(|e| anyhow::anyhow!("Axum error: {}", e))
}

async fn supervise_connectors(state: AppState, mut trigger_rx: mpsc::UnboundedReceiver<()>) {
    reconcile_connectors(&state).await;

    while trigger_rx.recv().await.is_some() {
        reconcile_connectors(&state).await;
    }
}

async fn reconcile_connectors(state: &AppState) {
    let telegram = state.app_config.read().connectors.telegram.clone();

    if let Some(config) = telegram {
        maybe_start_telegram_connector(state, config).await;
    }
}

async fn maybe_start_telegram_connector(state: &AppState, config: TelegramConfig) {
    if config.bot_token.trim().is_empty() {
        return;
    }

    {
        let mut running = state.running_connectors.write();
        if !running.insert("telegram".to_string()) {
            return;
        }
    }

    let bus = state.bus.clone();
    let running_connectors = state.running_connectors.clone();
    let observability = state.channel_observability.clone();

    tokio::spawn(async move {
        let connector = match TelegramConnector::try_new(ConnectorTelegramConfig {
            bot_token: config.bot_token,
            allowed_chat_ids: config.allowed_chat_ids,
            broadcast_chat_id: config.broadcast_chat_id,
        }) {
            Ok(connector) => connector,
            Err(err) => {
                error!("Failed to initialize Telegram connector: {}", err);
                record_connector_failure(
                    &observability,
                    "telegram",
                    "connector_init_failed",
                    err.to_string(),
                );
                running_connectors.write().remove("telegram");
                return;
            }
        };

        info!("Telegram connector bootstrapped and entering polling loop");
        if let Err(err) = connector.start(bus).await {
            error!("Telegram connector exited with error: {}", err);
            record_connector_failure(
                &observability,
                "telegram",
                "connector_runtime_failed",
                err.to_string(),
            );
        } else {
            warn!("Telegram connector exited unexpectedly without an error");
        }

        running_connectors.write().remove("telegram");
    });
}

fn record_connector_failure(
    observability: &Arc<
        parking_lot::RwLock<
            std::collections::HashMap<String, crate::api::state::ChannelObservability>,
        >,
    >,
    channel_id: &str,
    kind: &str,
    detail: String,
) {
    let mut guard = observability.write();
    let entry = guard.entry(channel_id.to_string()).or_insert_with(|| {
        crate::api::state::ChannelObservability {
            channel_id: channel_id.to_string(),
            ..Default::default()
        }
    });
    entry.last_failure_kind = Some(kind.to_string());
    entry.last_failure_detail = Some(detail);
    entry.last_observed_at = Some(chrono::Utc::now());
}
