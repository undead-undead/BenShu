use async_trait::async_trait;
use benshu_brain::agent::ApprovalHandler;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use uuid::Uuid;

const APPROVAL_RECEIPT_LIMIT: usize = 512;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityDecisionKind {
    Permit,
    Defer,
    Deny,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApprovalInfo {
    pub id: String,
    pub tool_name: String,
    pub arguments: String,
    pub challenge_code: String,
    pub decision_kind: SecurityDecisionKind,
    pub policy_basis: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalation_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApprovalInfoDto {
    pub id: String,
    pub tool_name: String,
    pub arguments: String,
    pub challenge_code: String,
    pub decision_kind: SecurityDecisionKind,
    pub policy_basis: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalation_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

impl From<ApprovalInfo> for ApprovalInfoDto {
    fn from(value: ApprovalInfo) -> Self {
        Self {
            id: value.id,
            tool_name: value.tool_name,
            arguments: value.arguments,
            challenge_code: value.challenge_code,
            decision_kind: value.decision_kind,
            policy_basis: value.policy_basis,
            escalation_reason: value.escalation_reason,
            created_at: value.created_at,
            trace_id: value.trace_id,
            run_id: value.run_id,
            task_id: value.task_id,
            session_id: value.session_id,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ApprovalRuntimeRefs {
    trace_id: Option<String>,
    run_id: Option<String>,
    task_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApprovalDecisionReceipt {
    pub receipt_id: String,
    pub approval_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub decision_kind: SecurityDecisionKind,
    pub policy_basis: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalation_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_code: Option<String>,
    pub trace_id: Option<String>,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApprovalDecisionReceiptDto {
    pub receipt_id: String,
    pub approval_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub decision_kind: SecurityDecisionKind,
    pub policy_basis: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalation_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_code: Option<String>,
    pub trace_id: Option<String>,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

impl From<ApprovalDecisionReceipt> for ApprovalDecisionReceiptDto {
    fn from(value: ApprovalDecisionReceipt) -> Self {
        Self {
            receipt_id: value.receipt_id,
            approval_id: value.approval_id,
            tool_name: value.tool_name,
            arguments: value.arguments,
            decision_kind: value.decision_kind,
            policy_basis: value.policy_basis,
            escalation_reason: value.escalation_reason,
            policy_reason: value.policy_reason,
            challenge_code: value.challenge_code,
            trace_id: value.trace_id,
            run_id: value.run_id,
            task_id: value.task_id,
            session_id: value.session_id,
            created_at: value.created_at,
            resolved_at: value.resolved_at,
        }
    }
}

pub struct PendingApproval {
    pub info: ApprovalInfo,
    pub responder: oneshot::Sender<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ApprovalEvent {
    Added(ApprovalInfo),
    Resolved { id: String, approved: bool },
}

pub struct ApprovalManager {
    pending: DashMap<String, PendingApproval>,
    receipts: parking_lot::RwLock<VecDeque<ApprovalDecisionReceipt>>,
    event_tx: broadcast::Sender<ApprovalEvent>,
}

impl Default for ApprovalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovalManager {
    fn current_runtime_refs() -> ApprovalRuntimeRefs {
        benshu_brain::skills::CURRENT_RUNTIME_SECURITY_CONTEXT
            .try_with(|ctx| ApprovalRuntimeRefs {
                trace_id: ctx.trace_id.clone(),
                run_id: ctx.run_id.clone(),
                task_id: ctx.task_id.clone(),
                session_id: ctx.session_id.clone(),
            })
            .unwrap_or_default()
    }

    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            pending: DashMap::new(),
            receipts: parking_lot::RwLock::new(VecDeque::new()),
            event_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApprovalEvent> {
        self.event_tx.subscribe()
    }

    pub fn add_request(
        &self,
        tool_name: &str,
        arguments: &str,
    ) -> (ApprovalInfo, oneshot::Receiver<bool>) {
        let (tx, rx) = oneshot::channel();
        let id = Uuid::new_v4().to_string();

        // Generate a 4-digit challenge code (Roadmap Phase 6.1)
        use rand::Rng;
        let challenge_code = format!("{:04}", rand::thread_rng().gen_range(0..10000));
        let created_at = Utc::now();
        let runtime_refs = Self::current_runtime_refs();

        let info = ApprovalInfo {
            id: id.clone(),
            tool_name: tool_name.to_string(),
            arguments: arguments.to_string(),
            challenge_code: challenge_code.clone(),
            decision_kind: SecurityDecisionKind::Defer,
            policy_basis: "manual_approval_required".to_string(),
            escalation_reason: Some(
                "High-risk tool execution requires explicit approval.".to_string(),
            ),
            created_at,
            trace_id: runtime_refs.trace_id.clone(),
            run_id: runtime_refs.run_id.clone(),
            task_id: runtime_refs.task_id.clone(),
            session_id: runtime_refs.session_id.clone(),
        };

        self.pending.insert(
            id.clone(),
            PendingApproval {
                info: info.clone(),
                responder: tx,
            },
        );

        self.push_receipt(ApprovalDecisionReceipt {
            receipt_id: Uuid::new_v4().to_string(),
            approval_id: id.clone(),
            tool_name: info.tool_name.clone(),
            arguments: info.arguments.clone(),
            decision_kind: SecurityDecisionKind::Defer,
            policy_basis: info.policy_basis.clone(),
            escalation_reason: info.escalation_reason.clone(),
            policy_reason: None,
            challenge_code: Some(challenge_code),
            trace_id: runtime_refs.trace_id,
            run_id: runtime_refs.run_id,
            task_id: runtime_refs.task_id,
            session_id: runtime_refs.session_id,
            created_at,
            resolved_at: None,
        });

        let _ = self.event_tx.send(ApprovalEvent::Added(info));

        let info = self
            .pending
            .get(&id)
            .map(|entry| entry.value().info.clone())
            .expect("approval request must exist immediately after insertion");

        (info, rx)
    }

    pub fn list_pending(&self) -> Vec<ApprovalInfo> {
        self.pending
            .iter()
            .map(|item| item.value().info.clone())
            .collect()
    }

    pub fn resolve(&self, id: &str, approved: bool) -> bool {
        if let Some((_, pending)) = self.pending.remove(id) {
            let _ = pending.responder.send(approved);
            let resolved_at = Utc::now();
            self.push_receipt(ApprovalDecisionReceipt {
                receipt_id: Uuid::new_v4().to_string(),
                approval_id: pending.info.id.clone(),
                tool_name: pending.info.tool_name.clone(),
                arguments: pending.info.arguments.clone(),
                decision_kind: if approved {
                    SecurityDecisionKind::Permit
                } else {
                    SecurityDecisionKind::Deny
                },
                policy_basis: pending.info.policy_basis.clone(),
                escalation_reason: pending.info.escalation_reason.clone(),
                policy_reason: if approved {
                    Some("Approved by explicit human confirmation.".to_string())
                } else {
                    Some("Rejected by explicit human confirmation.".to_string())
                },
                challenge_code: Some(pending.info.challenge_code.clone()),
                trace_id: pending.info.trace_id.clone(),
                run_id: pending.info.run_id.clone(),
                task_id: pending.info.task_id.clone(),
                session_id: pending.info.session_id.clone(),
                created_at: pending.info.created_at,
                resolved_at: Some(resolved_at),
            });
            let _ = self.event_tx.send(ApprovalEvent::Resolved {
                id: id.to_string(),
                approved,
            });
            true
        } else {
            false
        }
    }

    /// Resolve an approval via the challenge code (for non-rich messaging apps)
    pub fn resolve_by_challenge(&self, code: &str, approved: bool) -> bool {
        let mut target_id = None;
        for entry in self.pending.iter() {
            if entry.value().info.challenge_code == code {
                target_id = Some(entry.key().clone());
                break;
            }
        }

        if let Some(id) = target_id {
            self.resolve(&id, approved)
        } else {
            false
        }
    }

    pub fn expire(&self, id: &str, reason: &str) -> bool {
        if let Some((_, pending)) = self.pending.remove(id) {
            let _ = pending.responder.send(false);
            let resolved_at = Utc::now();
            self.push_receipt(ApprovalDecisionReceipt {
                receipt_id: Uuid::new_v4().to_string(),
                approval_id: pending.info.id.clone(),
                tool_name: pending.info.tool_name.clone(),
                arguments: pending.info.arguments.clone(),
                decision_kind: SecurityDecisionKind::Deny,
                policy_basis: pending.info.policy_basis.clone(),
                escalation_reason: pending.info.escalation_reason.clone(),
                policy_reason: Some(reason.to_string()),
                challenge_code: Some(pending.info.challenge_code.clone()),
                trace_id: pending.info.trace_id.clone(),
                run_id: pending.info.run_id.clone(),
                task_id: pending.info.task_id.clone(),
                session_id: pending.info.session_id.clone(),
                created_at: pending.info.created_at,
                resolved_at: Some(resolved_at),
            });
            let _ = self.event_tx.send(ApprovalEvent::Resolved {
                id: id.to_string(),
                approved: false,
            });
            true
        } else {
            false
        }
    }

    pub fn list_receipts(&self) -> Vec<ApprovalDecisionReceipt> {
        self.receipts.read().iter().cloned().collect()
    }

    pub fn get_receipt(&self, receipt_id: &str) -> Option<ApprovalDecisionReceipt> {
        self.receipts
            .read()
            .iter()
            .find(|receipt| receipt.receipt_id == receipt_id)
            .cloned()
    }

    pub fn list_receipts_for_approval(&self, approval_id: &str) -> Vec<ApprovalDecisionReceipt> {
        self.receipts
            .read()
            .iter()
            .filter(|receipt| receipt.approval_id == approval_id)
            .cloned()
            .collect()
    }

    fn push_receipt(&self, receipt: ApprovalDecisionReceipt) {
        let mut receipts = self.receipts.write();
        receipts.push_front(receipt);
        while receipts.len() > APPROVAL_RECEIPT_LIMIT {
            receipts.pop_back();
        }
    }
}

pub struct GatewayApprovalHandler {
    manager: Arc<ApprovalManager>,
}

impl GatewayApprovalHandler {
    pub fn new(manager: Arc<ApprovalManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ApprovalHandler for GatewayApprovalHandler {
    async fn approve_with_timeout(
        &self,
        tool_name: &str,
        arguments: &str,
        _safety: benshu_brain::skills::tool::SafetyLevel,
        timeout: std::time::Duration,
    ) -> anyhow::Result<bool> {
        let (info, rx) = self.manager.add_request(tool_name, arguments);

        // Wait for user to resolve or timeout
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(approved)) => Ok(approved),
            Ok(Err(_)) => {
                self.manager.expire(
                    &info.id,
                    "Approval responder dropped before a final decision was recorded.",
                );
                Ok(false)
            }
            Err(_) => {
                self.manager.expire(
                    &info.id,
                    "Approval request timed out before explicit confirmation.",
                );
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use benshu_brain::skills::RuntimeSecurityContext;

    #[test]
    fn approval_manager_records_defer_and_resolution_receipts() {
        let manager = ApprovalManager::new();
        let (info, _rx) = manager.add_request("exec_command", "{\"cmd\":\"ls\"}");
        let receipts = manager.list_receipts_for_approval(&info.id);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].decision_kind, SecurityDecisionKind::Defer);

        assert!(manager.resolve(&info.id, true));
        let receipts = manager.list_receipts_for_approval(&info.id);
        assert_eq!(receipts.len(), 2);
        assert!(receipts
            .iter()
            .any(|r| r.decision_kind == SecurityDecisionKind::Permit));
    }

    #[test]
    fn approval_manager_records_timeout_style_denial_receipts() {
        let manager = ApprovalManager::new();
        let (info, _rx) = manager.add_request("exec_command", "{\"cmd\":\"ls\"}");
        assert!(manager.expire(&info.id, "timed out"));
        let receipts = manager.list_receipts_for_approval(&info.id);
        assert!(receipts.iter().any(|r| {
            r.decision_kind == SecurityDecisionKind::Deny
                && r.policy_reason.as_deref() == Some("timed out")
        }));
    }

    #[tokio::test]
    async fn approval_manager_attaches_runtime_refs_from_task_context() {
        let manager = ApprovalManager::new();
        let (info, _rx) = benshu_brain::skills::CURRENT_RUNTIME_SECURITY_CONTEXT
            .scope(
                RuntimeSecurityContext {
                    trace_id: Some("trace-1".to_string()),
                    run_id: Some("run-1".to_string()),
                    task_id: Some("task-1".to_string()),
                    session_id: Some("session-1".to_string()),
                },
                async { manager.add_request("exec_command", "{\"cmd\":\"ls\"}") },
            )
            .await;

        assert_eq!(info.trace_id.as_deref(), Some("trace-1"));
        assert_eq!(info.run_id.as_deref(), Some("run-1"));
        assert_eq!(info.task_id.as_deref(), Some("task-1"));
        assert_eq!(info.session_id.as_deref(), Some("session-1"));

        let receipt = manager
            .list_receipts_for_approval(&info.id)
            .into_iter()
            .find(|receipt| receipt.decision_kind == SecurityDecisionKind::Defer)
            .expect("defer receipt should exist");
        assert_eq!(receipt.trace_id.as_deref(), Some("trace-1"));
        assert_eq!(receipt.run_id.as_deref(), Some("run-1"));
        assert_eq!(receipt.task_id.as_deref(), Some("task-1"));
        assert_eq!(receipt.session_id.as_deref(), Some("session-1"));
    }
}
