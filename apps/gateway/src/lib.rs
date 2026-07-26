pub mod api;
pub mod doctor;
pub mod onboard;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use tokio::sync::broadcast;

use benshu_kernel::KernelBootstrapper;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Embedded,
    Standalone,
}

#[derive(Debug, Clone, Copy)]
pub struct LaunchOptions {
    pub mode: LaunchMode,
    pub port_override: Option<u16>,
}

impl LaunchOptions {
    pub fn embedded(port_override: Option<u16>) -> Self {
        Self {
            mode: LaunchMode::Embedded,
            port_override,
        }
    }

    pub fn standalone(port_override: Option<u16>) -> Self {
        Self {
            mode: LaunchMode::Standalone,
            port_override,
        }
    }
}

/// Core launch logic for the Gateway engine.
/// Refactored to leverage benshu-kernel for Phase 4.1 Minimalism.
pub async fn launch_engine(
    base_dir: PathBuf,
    options: LaunchOptions,
    external_log_tx: Option<broadcast::Sender<String>>,
) -> Result<()> {
    // 1. Initial configuration loading
    let config_path = base_dir.join("benshu.yaml");
    let mut app_config = benshu_brain::config::AppConfig::load_from_file(&config_path)?;
    if app_config.migrate_agent_runtime_overrides_from_frontmatter(&config_path)? {
        app_config.save_to_file(&config_path)?;
        info!("Migrated agent runtime overrides from AGENT.md frontmatter into benshu.yaml");
    }
    if let Some(port) = options.port_override {
        app_config.server.port = port;
    }
    if let Ok(host) = std::env::var("BENSHU_GATEWAY_HOST") {
        let host = host.trim();
        if !host.is_empty() {
            app_config.server.host = host.to_string();
        }
    }
    if matches!(options.mode, LaunchMode::Embedded) {
        app_config.server.host = "127.0.0.1".to_string();
    }

    // 2. Central Kernel Initialization
    let bootstrapper = KernelBootstrapper::new(base_dir.clone(), app_config.clone());
    let kernel = Arc::new(bootstrapper.boot().await?);

    // 3. Gateway-specific Secret Extraction (Using the bootstrapped vault)
    let internal_key = if let Ok(env_token) = std::env::var("BENSHU_SESSION_TOKEN") {
        info!("Unified: Using session-scoped handshake token from Panel.");
        env_token
    } else {
        let vault = kernel.vault();
        if let Some(key) = vault.get("GATEWAY_INTERNAL_KEY")? {
            key
        } else {
            let bytes: [u8; 16] = rand::random();
            let new_key = hex::encode(bytes);
            let _ = vault.set("GATEWAY_INTERNAL_KEY", &new_key);
            new_key
        }
    };
    info!("Gateway internal auth key initialized.");

    // 4. Higher-Level Service Assembly
    let enabled_tools = Arc::new(parking_lot::RwLock::new(std::collections::HashSet::new()));
    let shared_config = Arc::new(parking_lot::RwLock::new(app_config.clone()));
    let log_tx = external_log_tx.unwrap_or_else(|| broadcast::channel(100).0);

    // Kernel Factory - Central agent construction
    let factory = Arc::new(benshu_kernel::service::factory::AgentFactory::new(
        kernel.clone(),
        shared_config.clone(),
        enabled_tools.clone(),
        None,
    ));
    factory.install_worker_spawner();

    // 5. OAuth & Multi-Session Logic
    let oauth_manager = Arc::new(benshu_auth::OAuthManager::new(Arc::new(
        benshu_auth::VaultTokenStore::new(kernel.vault().clone()),
    )));
    // 6. Launch API Server
    api::server::start_server(
        kernel,
        factory,
        oauth_manager,
        shared_config,
        enabled_tools,
        config_path,
        log_tx,
        internal_key,
        options.mode,
    )
    .await
}
