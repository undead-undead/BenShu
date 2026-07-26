use crate::api::state::{AppError, AppState};
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct WorkspaceRequest {
    pub path: String,
}

pub async fn list_workspaces(State(state): State<AppState>) -> Json<Vec<String>> {
    let config = state.app_config.read();
    Json(
        config
            .trusted_workspaces
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    )
}

pub async fn add_workspace(
    State(state): State<AppState>,
    Json(payload): Json<WorkspaceRequest>,
) -> Result<StatusCode, AppError> {
    let mut config = state.app_config.write();
    let path = PathBuf::from(&payload.path);
    if !config.trusted_workspaces.contains(&path) {
        config.trusted_workspaces.push(path.clone());
        let _ = config.save_to_file(&state.config_path);
        tracing::info!("Added trusted workspace: {}", path.display());
    }
    Ok(StatusCode::OK)
}

pub async fn remove_workspace(
    State(state): State<AppState>,
    Json(payload): Json<WorkspaceRequest>,
) -> Result<StatusCode, AppError> {
    let mut config = state.app_config.write();
    let path = PathBuf::from(&payload.path);
    config.trusted_workspaces.retain(|p| p != &path);
    let _ = config.save_to_file(&state.config_path);
    tracing::info!("Removed trusted workspace: {}", path.display());
    Ok(StatusCode::OK)
}
