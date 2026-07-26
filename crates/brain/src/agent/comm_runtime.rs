use super::core::Agent;
use crate::agent::protocol::AgentEventData;
use crate::agent::provider::Provider;
use crate::error::{Error, Result};
use benshu_comm::protocol::a2a::{
    A2AMessage, DelegationEnvelope, DelegationReturnMode, DelegationState,
};
use benshu_comm::protocol::CommEnvelope;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct A2aInboxEntry {
    message_id: String,
    source: String,
    target: String,
    kind: String,
    request_id: Option<String>,
    session_id: Option<String>,
    trace_id: Option<String>,
    task_id: Option<String>,
    parent_task_id: Option<String>,
    root_task_id: Option<String>,
    summary: String,
    visible_owner: Option<String>,
    memory_owner: Option<String>,
    approval_owner: Option<String>,
    delegated_by: Option<String>,
    delegated_to: Option<String>,
    final_response_owner: Option<String>,
    return_mode: Option<String>,
    delegation_state: Option<String>,
}

impl<P: Provider + 'static> Agent<P> {
    pub(crate) fn comm_metadata_key(&self, suffix: &str) -> String {
        format!("brain.comm.{}.{}", self.config.name, suffix)
    }

    pub(crate) async fn set_comm_metadata_json<T: Serialize>(&self, suffix: &str, value: &T) {
        if let Some(memory) = &self.memory {
            if let Ok(serialized) = serde_json::to_string(value) {
                if let Err(err) = memory
                    .set_metadata(&self.comm_metadata_key(suffix), &serialized)
                    .await
                {
                    debug!("failed to persist comm metadata '{}': {}", suffix, err);
                }
            }
        }
    }

    pub(crate) async fn set_comm_metadata_value(&self, suffix: &str, value: &str) {
        if let Some(memory) = &self.memory {
            if let Err(err) = memory
                .set_metadata(&self.comm_metadata_key(suffix), value)
                .await
            {
                debug!("failed to persist comm metadata '{}': {}", suffix, err);
            }
        }
    }

    async fn append_comm_inbox_entry(&self, entry: A2aInboxEntry) {
        let Some(memory) = &self.memory else {
            return;
        };

        let key = self.comm_metadata_key("inbox.recent_json");
        let existing = match memory.get_metadata(&key).await {
            Ok(Some(existing)) => {
                serde_json::from_str::<Vec<A2aInboxEntry>>(&existing).unwrap_or_default()
            }
            _ => Vec::new(),
        };

        let mut updated = existing;
        updated.push(entry);
        if updated.len() > 32 {
            let drain = updated.len() - 32;
            updated.drain(0..drain);
        }

        if let Ok(serialized) = serde_json::to_string(&updated) {
            if let Err(err) = memory.set_metadata(&key, &serialized).await {
                debug!("failed to persist comm inbox metadata: {}", err);
            }
        }
    }

    fn emit_a2a_processed(
        &self,
        envelope: &CommEnvelope,
        kind: &str,
        request_id: Option<String>,
        delegation: Option<&DelegationEnvelope>,
    ) {
        self.emit(AgentEventData::A2aEnvelopeProcessed {
            kind: kind.to_string(),
            message_id: Some(envelope.meta.id.to_string()),
            request_id,
            runtime_profile: self
                .comm_client
                .as_ref()
                .map(|client| client.runtime_profile().as_str().to_string())
                .unwrap_or_else(|| "disabled".to_string()),
            source: Some(envelope.meta.source.to_string()),
            target: Some(envelope.target.to_string()),
            session_id: delegation.and_then(|d| d.session_id.clone()),
            trace_id: delegation.and_then(|d| d.trace_id.clone()),
            task_id: delegation.and_then(|d| d.task_id.clone()),
            parent_task_id: delegation.and_then(|d| d.parent_task_id.clone()),
            root_task_id: delegation.and_then(|d| d.root_task_id.clone()),
            visible_owner: delegation.map(|d| d.visible_owner_id.clone()),
            memory_owner: delegation.map(|d| d.memory_owner_id.clone()),
            approval_owner: delegation.map(|d| d.approval_owner_id.clone()),
            delegated_by: delegation.map(|d| d.delegated_by_id.clone()),
            delegated_to: delegation.map(|d| d.delegated_to_id.clone()),
            final_response_owner: delegation.map(|d| d.final_response_owner_id.clone()),
            return_mode: delegation.map(|d| match d.return_mode {
                DelegationReturnMode::ReturnToOwner => "return_to_owner".to_string(),
                DelegationReturnMode::SessionTransfer => "session_transfer".to_string(),
            }),
            delegation_state: delegation.map(|d| d.state.as_str().to_string()),
        });
    }

    fn return_mode_label(delegation: Option<&DelegationEnvelope>) -> Option<String> {
        delegation.map(|d| match d.return_mode {
            DelegationReturnMode::ReturnToOwner => "return_to_owner".to_string(),
            DelegationReturnMode::SessionTransfer => "session_transfer".to_string(),
        })
    }

    fn delegation_state_label(delegation: Option<&DelegationEnvelope>) -> Option<String> {
        delegation.map(|d| d.state.as_str().to_string())
    }

    async fn record_owner_rollup(
        &self,
        kind: &str,
        request_id: Option<&str>,
        session_id: Option<&str>,
        summary: impl Into<String>,
        delegation: &DelegationEnvelope,
    ) {
        if delegation.final_response_owner_id != self.config.name {
            return;
        }

        let payload = serde_json::json!({
            "kind": kind,
            "request_id": request_id,
            "session_id": session_id,
            "visible_owner_id": delegation.visible_owner_id,
            "memory_owner_id": delegation.memory_owner_id,
            "approval_owner_id": delegation.approval_owner_id,
            "final_response_owner_id": delegation.final_response_owner_id,
            "delegated_by_id": delegation.delegated_by_id,
            "delegated_to_id": delegation.delegated_to_id,
            "return_mode": Self::return_mode_label(Some(delegation)),
            "summary": summary.into(),
        });
        self.set_comm_metadata_json("owner_rollup.last_json", &payload)
            .await;
    }

    async fn handle_comm_envelope(&self, envelope: CommEnvelope) {
        let Ok(message) = serde_json::from_slice::<A2AMessage>(&envelope.payload) else {
            debug!(
                "{}: received non-A2A comm envelope from {}",
                self.config.name, envelope.meta.source
            );
            return;
        };

        match message {
            A2AMessage::Announcement(manifest) => {
                let entry = A2aInboxEntry {
                    message_id: envelope.meta.id.to_string(),
                    source: envelope.meta.source.to_string(),
                    target: envelope.target.to_string(),
                    kind: "announcement".to_string(),
                    request_id: None,
                    session_id: None,
                    trace_id: None,
                    task_id: None,
                    parent_task_id: None,
                    root_task_id: None,
                    summary: format!(
                        "agent '{}' is {}",
                        manifest.id,
                        serde_json::to_string(&manifest.status)
                            .unwrap_or_else(|_| "online".to_string())
                    ),
                    visible_owner: None,
                    memory_owner: None,
                    approval_owner: None,
                    delegated_by: None,
                    delegated_to: None,
                    final_response_owner: None,
                    return_mode: None,
                    delegation_state: None,
                };
                self.set_comm_metadata_json("announcement.last_json", &manifest)
                    .await;
                self.append_comm_inbox_entry(entry).await;
                self.emit_a2a_processed(&envelope, "announcement", None, None);
            }
            A2AMessage::TaskRequest {
                request_id,
                requester_id,
                task_content,
                delegation,
                ..
            } => {
                let entry = A2aInboxEntry {
                    message_id: envelope.meta.id.to_string(),
                    source: envelope.meta.source.to_string(),
                    target: envelope.target.to_string(),
                    kind: "task_request".to_string(),
                    request_id: Some(request_id.clone()),
                    session_id: delegation.as_ref().and_then(|d| d.session_id.clone()),
                    trace_id: delegation.as_ref().and_then(|d| d.trace_id.clone()),
                    task_id: delegation.as_ref().and_then(|d| d.task_id.clone()),
                    parent_task_id: delegation.as_ref().and_then(|d| d.parent_task_id.clone()),
                    root_task_id: delegation.as_ref().and_then(|d| d.root_task_id.clone()),
                    summary: task_content.clone(),
                    visible_owner: delegation.as_ref().map(|d| d.visible_owner_id.clone()),
                    memory_owner: delegation.as_ref().map(|d| d.memory_owner_id.clone()),
                    approval_owner: delegation.as_ref().map(|d| d.approval_owner_id.clone()),
                    delegated_by: delegation
                        .as_ref()
                        .map(|d| d.delegated_by_id.clone())
                        .or(Some(requester_id)),
                    delegated_to: delegation.as_ref().map(|d| d.delegated_to_id.clone()),
                    final_response_owner: delegation
                        .as_ref()
                        .map(|d| d.final_response_owner_id.clone()),
                    return_mode: Self::return_mode_label(delegation.as_ref()),
                    delegation_state: Self::delegation_state_label(delegation.as_ref()),
                };
                self.set_comm_metadata_json("task_request.last_json", &entry)
                    .await;
                self.append_comm_inbox_entry(entry).await;
                self.emit_a2a_processed(
                    &envelope,
                    "task_request",
                    Some(request_id),
                    delegation.as_ref(),
                );
            }
            A2AMessage::TaskAssignment {
                request_id,
                assigned_to,
                task_context,
                delegation,
                ..
            } => {
                let entry = A2aInboxEntry {
                    message_id: envelope.meta.id.to_string(),
                    source: envelope.meta.source.to_string(),
                    target: envelope.target.to_string(),
                    kind: "task_assignment".to_string(),
                    request_id: Some(request_id.clone()),
                    session_id: delegation.as_ref().and_then(|d| d.session_id.clone()),
                    trace_id: delegation.as_ref().and_then(|d| d.trace_id.clone()),
                    task_id: delegation.as_ref().and_then(|d| d.task_id.clone()),
                    parent_task_id: delegation.as_ref().and_then(|d| d.parent_task_id.clone()),
                    root_task_id: delegation.as_ref().and_then(|d| d.root_task_id.clone()),
                    summary: task_context.clone(),
                    visible_owner: delegation.as_ref().map(|d| d.visible_owner_id.clone()),
                    memory_owner: delegation.as_ref().map(|d| d.memory_owner_id.clone()),
                    approval_owner: delegation.as_ref().map(|d| d.approval_owner_id.clone()),
                    delegated_by: delegation.as_ref().map(|d| d.delegated_by_id.clone()),
                    delegated_to: delegation
                        .as_ref()
                        .map(|d| d.delegated_to_id.clone())
                        .or(Some(assigned_to)),
                    final_response_owner: delegation
                        .as_ref()
                        .map(|d| d.final_response_owner_id.clone()),
                    return_mode: Self::return_mode_label(delegation.as_ref()),
                    delegation_state: Self::delegation_state_label(delegation.as_ref()),
                };
                self.set_comm_metadata_json("task_assignment.last_json", &entry)
                    .await;
                self.append_comm_inbox_entry(entry).await;
                self.emit_a2a_processed(
                    &envelope,
                    "task_assignment",
                    Some(request_id),
                    delegation.as_ref(),
                );
            }
            A2AMessage::Result {
                request_id,
                performer_id,
                output,
                success,
                delegation,
            } => {
                let session_id = delegation.as_ref().and_then(|d| d.session_id.clone());
                let summary = if success {
                    format!("result from {}: {}", performer_id, output)
                } else {
                    format!("failed result from {}: {}", performer_id, output)
                };
                let entry = A2aInboxEntry {
                    message_id: envelope.meta.id.to_string(),
                    source: envelope.meta.source.to_string(),
                    target: envelope.target.to_string(),
                    kind: "result".to_string(),
                    request_id: Some(request_id.clone()),
                    session_id: session_id.clone(),
                    trace_id: delegation.as_ref().and_then(|d| d.trace_id.clone()),
                    task_id: delegation.as_ref().and_then(|d| d.task_id.clone()),
                    parent_task_id: delegation.as_ref().and_then(|d| d.parent_task_id.clone()),
                    root_task_id: delegation.as_ref().and_then(|d| d.root_task_id.clone()),
                    summary: summary.clone(),
                    visible_owner: delegation.as_ref().map(|d| d.visible_owner_id.clone()),
                    memory_owner: delegation.as_ref().map(|d| d.memory_owner_id.clone()),
                    approval_owner: delegation.as_ref().map(|d| d.approval_owner_id.clone()),
                    delegated_by: delegation.as_ref().map(|d| d.delegated_by_id.clone()),
                    delegated_to: delegation.as_ref().map(|d| d.delegated_to_id.clone()),
                    final_response_owner: delegation
                        .as_ref()
                        .map(|d| d.final_response_owner_id.clone()),
                    return_mode: Self::return_mode_label(delegation.as_ref()),
                    delegation_state: Self::delegation_state_label(delegation.as_ref()),
                };
                self.set_comm_metadata_json("result.last_json", &entry)
                    .await;
                self.append_comm_inbox_entry(entry).await;
                if let Some(delegation) = delegation.as_ref() {
                    self.record_owner_rollup(
                        "result",
                        Some(&request_id),
                        session_id.as_deref(),
                        summary,
                        delegation,
                    )
                    .await;
                }
                self.emit_a2a_processed(&envelope, "result", Some(request_id), delegation.as_ref());
            }
            A2AMessage::Handover {
                session_id,
                from_agent_id,
                to_agent_id,
                context_summary,
            } => {
                let entry = A2aInboxEntry {
                    message_id: envelope.meta.id.to_string(),
                    source: envelope.meta.source.to_string(),
                    target: envelope.target.to_string(),
                    kind: "handover".to_string(),
                    request_id: None,
                    session_id: Some(session_id.clone()),
                    trace_id: None,
                    task_id: None,
                    parent_task_id: None,
                    root_task_id: None,
                    summary: context_summary,
                    visible_owner: None,
                    memory_owner: None,
                    approval_owner: None,
                    delegated_by: Some(from_agent_id),
                    delegated_to: Some(to_agent_id),
                    final_response_owner: None,
                    return_mode: Some("session_transfer".to_string()),
                    delegation_state: Some(DelegationState::Transferred.as_str().to_string()),
                };
                self.set_comm_metadata_json("handover.last_json", &entry)
                    .await;
                self.append_comm_inbox_entry(entry).await;
                self.emit_a2a_processed(&envelope, "handover", None, None);
            }
            A2AMessage::Heartbeat {
                agent_id,
                status,
                load,
                timestamp,
            } => {
                let payload = serde_json::json!({
                    "agent_id": agent_id,
                    "status": status,
                    "load": load,
                    "timestamp": timestamp,
                });
                self.set_comm_metadata_json("heartbeat.last_json", &payload)
                    .await;
                self.emit_a2a_processed(&envelope, "heartbeat", None, None);
            }
            A2AMessage::Broadcast { from_id, message } => {
                let entry = A2aInboxEntry {
                    message_id: envelope.meta.id.to_string(),
                    source: envelope.meta.source.to_string(),
                    target: envelope.target.to_string(),
                    kind: "broadcast".to_string(),
                    request_id: None,
                    session_id: None,
                    trace_id: None,
                    task_id: None,
                    parent_task_id: None,
                    root_task_id: None,
                    summary: message,
                    visible_owner: None,
                    memory_owner: None,
                    approval_owner: None,
                    delegated_by: Some(from_id),
                    delegated_to: None,
                    final_response_owner: None,
                    return_mode: None,
                    delegation_state: None,
                };
                self.set_comm_metadata_json("broadcast.last_json", &entry)
                    .await;
                self.append_comm_inbox_entry(entry).await;
                self.emit_a2a_processed(&envelope, "broadcast", None, None);
            }
            A2AMessage::Bid {
                request_id,
                bidder_id,
                bid_amount,
                metadata,
            } => {
                let entry = A2aInboxEntry {
                    message_id: envelope.meta.id.to_string(),
                    source: envelope.meta.source.to_string(),
                    target: envelope.target.to_string(),
                    kind: "bid".to_string(),
                    request_id: Some(request_id.clone()),
                    session_id: None,
                    trace_id: None,
                    task_id: None,
                    parent_task_id: None,
                    root_task_id: None,
                    summary: metadata
                        .clone()
                        .unwrap_or_else(|| format!("bid={}", bid_amount)),
                    visible_owner: None,
                    memory_owner: None,
                    approval_owner: None,
                    delegated_by: Some(bidder_id.clone()),
                    delegated_to: None,
                    final_response_owner: None,
                    return_mode: None,
                    delegation_state: None,
                };
                let payload = serde_json::json!({
                    "request_id": request_id.clone(),
                    "bidder_id": bidder_id,
                    "bid_amount": bid_amount,
                    "metadata": metadata,
                });
                self.set_comm_metadata_json("bid.last_json", &payload).await;
                self.append_comm_inbox_entry(entry).await;
                self.emit_a2a_processed(&envelope, "bid", Some(request_id), None);
            }
        }
    }

    pub async fn poll_comm_once(&self) -> Result<bool> {
        let Some(comm_client) = &self.comm_client else {
            return Ok(false);
        };

        match comm_client.receive_next().await {
            Ok(Some(envelope)) => {
                self.handle_comm_envelope(envelope).await;
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(err) => Err(Error::AgentCoordination(format!(
                "Comm client receive loop error: {}",
                err
            ))),
        }
    }
}
