use crate::api::state::AppState;
use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

fn matches_api_key(candidate: &str, expected: &str) -> bool {
    candidate == expected || candidate.strip_prefix("Bearer ") == Some(expected)
}

pub async fn api_guard(
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    // 1. Health / Public / Webhook bypass (minimal)
    let path = req.uri().path();
    if path == "/health" || path.starts_with("/api/v1/webhook") || path.starts_with("/api/auth") {
        return next.run(req).await;
    }

    // 2. Auth Check: Session Token takes priority, then fallback to Internal Key
    // Strict requirement: No more localhost bypass for Zero-Trust (Anti-Tamper)
    let auth_header = req
        .headers()
        .get("X-API-Key")
        .or_else(|| req.headers().get("Authorization")) // Support standard header
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(key_str) => {
            // Priority 1: Embedded session token (panel-launched desktop mode only)
            let session_token = if matches!(state.deployment_mode, crate::LaunchMode::Embedded) {
                std::env::var("BENSHU_SESSION_TOKEN").ok()
            } else {
                None
            };
            let is_session_auth = if let Some(st) = session_token {
                matches_api_key(key_str, &st)
            } else {
                false
            };

            if is_session_auth {
                /* // Phase 2.3: PID-Bound Handshake Verification (Temporarily disabled for Dev Stability)
                if let Some(pid_val) = req.headers().get("X-Parent-PID") {
                    if let Ok(pid_str) = pid_val.to_str() {
                        if let Ok(claimed_pid) = pid_str.parse::<u32>() {
                            if !state.system_security.verify_parent(claimed_pid) {
                                tracing::error!("Anti-Tamper: PID mismatch for SessionToken auth! Claimed: {}", claimed_pid);
                                return (StatusCode::FORBIDDEN, "Security Violation: Parent PID Mismatch").into_response();
                            }
                        }
                    }
                } */
                return next.run(req).await;
            }

            // Priority 2: Fallback to vault-persisted key (for multi-device/longer sessions)
            if matches_api_key(key_str, &state.internal_key) {
                return next.run(req).await;
            }

            (
                StatusCode::UNAUTHORIZED,
                "Unauthorized: Invalid Session Token or API Key",
            )
                .into_response()
        }
        None => (StatusCode::UNAUTHORIZED, "Unauthorized: Missing X-API-Key").into_response(),
    }
}
