use crate::api::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct VaultSetRequest {
    pub key: String,
    pub value: String,
}

pub async fn list_vault_keys(State(state): State<AppState>) -> impl IntoResponse {
    match state.kernel.vault().list_keys() {
        Ok(keys) => Json(json!({ "keys": keys })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn get_vault_value(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match state.kernel.vault().get(&key) {
        Ok(Some(val)) => Json(json!({ "value": val })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Key not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn set_vault_value(
    State(state): State<AppState>,
    Json(req): Json<VaultSetRequest>,
) -> impl IntoResponse {
    match state.kernel.vault().set(&req.key, &req.value) {
        Ok(_) => Json(json!({ "status": "ok" })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn delete_vault_value(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match state.kernel.vault().delete(&key) {
        Ok(_) => Json(json!({ "status": "ok" })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn save_vault_secret(
    State(state): State<AppState>,
    Json(req): Json<VaultSetRequest>,
) -> Result<StatusCode, crate::api::state::AppError> {
    let key = req.key.to_uppercase();
    state.kernel.vault().set(&key, &req.value)?;

    // Also update config memory if it's a known provider key
    {
        let mut cfg = state.app_config.write();
        let name = if key.ends_with("_API_KEY") {
            key.strip_suffix("_API_KEY").unwrap_or(&key).to_lowercase()
        } else {
            key.to_lowercase()
        };

        if !cfg.providers.custom_providers.contains(&name) {
            cfg.providers.custom_providers.push(name);
        }

        let _ = cfg.save_to_file(&state.config_path);
    }

    Ok(StatusCode::OK)
}

pub async fn delete_vault_secret(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<StatusCode, crate::api::state::AppError> {
    let key = key.to_uppercase();
    state.kernel.vault().delete(&key)?;

    {
        let mut cfg = state.app_config.write();
        let name = if key.ends_with("_API_KEY") {
            key.strip_suffix("_API_KEY").unwrap_or(&key).to_lowercase()
        } else {
            key.to_lowercase()
        };

        cfg.providers.custom_providers.retain(|x| x != &name);
        let _ = cfg.save_to_file(&state.config_path);
    }

    Ok(StatusCode::OK)
}
