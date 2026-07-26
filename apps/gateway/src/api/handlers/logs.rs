use crate::api::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use serde::Deserialize;

pub async fn get_recent_logs(State(state): State<AppState>) -> Json<Vec<String>> {
    let history = state.log_history.read();
    Json(history.iter().cloned().collect())
}

pub async fn logs_stream(
    State(state): State<AppState>,
) -> Sse<impl futures::Stream<Item = Result<Event, axum::Error>>> {
    let mut rx = state.log_sender.subscribe();

    let stream = async_stream::stream! {
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().data(msg));
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

#[derive(Deserialize)]
pub struct AuditQueryParams {
    pub limit: Option<usize>,
}

pub async fn get_audit_entries(
    State(state): State<AppState>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<Vec<benshu_security::audit::AuditEntry>>, (StatusCode, String)> {
    let limit = params.limit.unwrap_or(100);
    state
        .kernel
        .security()
        .audit
        .retrieve_recent(limit)
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to retrieve audit logs: {}", e),
            )
        })
}
