use crate::executor::{ExecutionContext, ImageExecutor};
use crate::types::{ErrorBody, ErrorPayload, ImageRequest};
use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub ctx: ExecutionContext,
    pub executor: Arc<dyn ImageExecutor>,
}

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "ok": true,
        "backend": "local-image-runtime-rs",
        "model": state.ctx.config.model_name,
        "runtime": state.ctx.config,
        "windows_native_runtime": state.ctx.runtime,
        "bundle": state.ctx.bundle,
        "status": "control_plane_ready"
    }))
}

pub async fn generate(
    State(state): State<AppState>,
    Json(request): Json<ImageRequest>,
) -> impl IntoResponse {
    handle_request(state, request, false).await
}

pub async fn edit(
    State(state): State<AppState>,
    Json(request): Json<ImageRequest>,
) -> impl IntoResponse {
    handle_request(state, request, true).await
}

async fn handle_request(
    state: AppState,
    request: ImageRequest,
    editing: bool,
) -> impl IntoResponse {
    let normalized = match request.normalize(editing) {
        Ok(normalized) => normalized,
        Err(error) => {
            return (
                error.status,
                Json(ErrorPayload {
                    error: ErrorBody {
                        message: error.message,
                        kind: error.kind,
                    },
                }),
            )
                .into_response();
        }
    };

    match state
        .executor
        .execute(&state.ctx, normalized, editing)
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => (
            error.status,
            Json(ErrorPayload {
                error: ErrorBody {
                    message: error.message,
                    kind: error.kind,
                },
            }),
        )
            .into_response(),
    }
}
