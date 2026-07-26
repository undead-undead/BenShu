use crate::api::state::{AppError, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AuthCallbackQuery {
    pub code: String,
    pub state: String,
}

pub async fn auth_initiate_handler(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let (auth_url, _csrf_token) = state.oauth.initiate_auth(&provider)?;
    Ok(Redirect::to(&auth_url))
}

pub async fn auth_callback_handler(
    State(state): State<AppState>,
    Path(_provider): Path<String>,
    Query(query): Query<AuthCallbackQuery>,
) -> Result<impl IntoResponse, AppError> {
    let token = state.oauth.handle_callback(query.code, query.state).await?;
    Ok((
        StatusCode::OK,
        format!(
            "Authentication successful! Token acquired. (Expires at: {:?})",
            token.expires_at
        ),
    ))
}
