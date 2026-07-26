use crate::api::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ResolveApprovalRequest {
    pub approved: bool,
}

pub async fn list_approvals(
    State(state): State<AppState>,
) -> Json<Vec<crate::api::security::ApprovalInfoDto>> {
    Json(
        state
            .approvals
            .list_pending()
            .into_iter()
            .map(crate::api::security::ApprovalInfoDto::from)
            .collect(),
    )
}

pub async fn list_approval_receipts(
    State(state): State<AppState>,
) -> Json<Vec<crate::api::security::ApprovalDecisionReceiptDto>> {
    Json(
        state
            .approvals
            .list_receipts()
            .into_iter()
            .map(crate::api::security::ApprovalDecisionReceiptDto::from)
            .collect(),
    )
}

pub async fn get_approval_receipt(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::api::security::ApprovalDecisionReceiptDto>, StatusCode> {
    state
        .approvals
        .get_receipt(&id)
        .map(crate::api::security::ApprovalDecisionReceiptDto::from)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn list_approval_policy_basis(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Vec<serde_json::Value>> {
    Json(
        state
            .approvals
            .list_receipts_for_approval(&id)
            .into_iter()
            .map(|receipt| {
                serde_json::json!({
                    "receipt_id": receipt.receipt_id,
                    "approval_id": receipt.approval_id,
                    "decision_kind": receipt.decision_kind,
                    "policy_basis": receipt.policy_basis,
                    "escalation_reason": receipt.escalation_reason,
                    "policy_reason": receipt.policy_reason,
                    "trace_id": receipt.trace_id,
                    "run_id": receipt.run_id,
                    "task_id": receipt.task_id,
                    "session_id": receipt.session_id,
                    "created_at": receipt.created_at,
                    "resolved_at": receipt.resolved_at,
                })
            })
            .collect(),
    )
}

pub async fn resolve_approval(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ResolveApprovalRequest>,
) -> StatusCode {
    if state.approvals.resolve(&id, payload.approved) {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

pub async fn get_internal_key(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "key": state.internal_key }))
}

pub async fn get_active_sandboxes() -> Json<Vec<benshu_security::sandbox::ActiveSandboxContext>> {
    let mut sandboxes = Vec::new();
    for entry in benshu_security::sandbox::ACTIVE_SANDBOXES.iter() {
        sandboxes.push(entry.value().clone());
    }
    Json(sandboxes)
}

pub async fn kill_sandbox(Path(pid): Path<u32>) -> (StatusCode, Json<serde_json::Value>) {
    let Some(existing) = benshu_security::sandbox::ACTIVE_SANDBOXES
        .get(&pid)
        .map(|entry| entry.value().clone())
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "message": format!("Sandbox PID {} not found or already terminated", pid)
            })),
        );
    };

    #[cfg(target_os = "windows")]
    let output = std::process::Command::new("taskkill")
        .args(&["/F", "/PID", &pid.to_string()])
        .output();

    #[cfg(not(target_os = "windows"))]
    let output = std::process::Command::new("kill")
        .args(&["-9", &pid.to_string()])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            benshu_security::sandbox::ACTIVE_SANDBOXES.remove(&pid);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "message": format!("Sandbox PID {} killed successfully", pid)
                })),
            )
        }
        Ok(o) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "message": format!("Failed to kill PID {}: {}", pid, String::from_utf8_lossy(&o.stderr)),
                "sandbox_engine": existing.sandbox_engine,
                "isolation_state": existing.isolation_state
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "message": format!("Error executing kill command: {}", e),
                "sandbox_engine": existing.sandbox_engine,
                "isolation_state": existing.isolation_state
            })),
        ),
    }
}
