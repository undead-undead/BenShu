mod adapter;
mod bundle;
mod config;
mod executor;
mod handlers;
mod plan;
mod runtime;
mod types;

use axum::{
    routing::{get, post},
    Router,
};
use bundle::BundleInfo;
use config::RuntimeConfig;
use executor::{ExecutionContext, NativeOnnxImageExecutor};
use handlers::{edit, generate, health, AppState};
use runtime::ImageRuntimeStatus;
use std::{net::SocketAddr, path::Path, sync::Arc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = RuntimeConfig::from_env();
    let bundle = Arc::new(BundleInfo::inspect(Path::new(&config.model_dir))?);
    let runtime = ImageRuntimeStatus::detect();
    let ctx = ExecutionContext {
        config,
        bundle,
        runtime,
    };
    let state = AppState {
        ctx,
        executor: Arc::new(NativeOnnxImageExecutor::new()),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/health", get(health))
        .route("/v1/images/generations", post(generate))
        .route("/v1/images/edits", post(edit))
        .with_state(state.clone());

    let addr: SocketAddr =
        format!("{}:{}", state.ctx.config.host, state.ctx.config.port).parse()?;
    tracing::info!("Starting windows-image-service on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
