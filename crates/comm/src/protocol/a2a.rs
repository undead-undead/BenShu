use benshu_infra::agent::AgentRole;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Status of an agent in the swarm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Online,
    Busy,
    Offline,
    Error,
}

/// Events that an agent receives from the A2A system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum A2AEvent {
    /// Execute a task delegated by another agent
    ExecuteTask {
        request_id: String,
        task: String,
        context: String,
    },
    /// Result of a task that this agent delegated
    TaskResult {
        request_id: String,
        result: String,
        success: bool,
    },
    /// Handover session control
    Handover {
        session_id: String,
        from_agent_id: String,
        context_summary: String,
    },
    /// Heartbeat from peer
    PeerHeartbeat {
        agent_id: String,
        status: AgentStatus,
        load: f64,
    },
}

/// Metadata about an agent for A2A communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub id: String,
    pub name: String,
    pub role: AgentRole,
    pub capabilities: Vec<String>,
    pub address: Option<String>,
    pub status: AgentStatus,
    pub last_seen: DateTime<Utc>,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationReturnMode {
    ReturnToOwner,
    SessionTransfer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationOwnership {
    pub visible_owner_id: String,
    pub memory_owner_id: String,
    pub approval_owner_id: String,
    pub final_response_owner_id: String,
}

impl DelegationOwnership {
    pub fn new(owner_id: impl Into<String>) -> Self {
        let owner_id = owner_id.into();
        Self {
            visible_owner_id: owner_id.clone(),
            memory_owner_id: owner_id.clone(),
            approval_owner_id: owner_id.clone(),
            final_response_owner_id: owner_id,
        }
    }

    pub fn with_memory_owner(mut self, owner_id: impl Into<String>) -> Self {
        self.memory_owner_id = owner_id.into();
        self
    }

    pub fn with_approval_owner(mut self, owner_id: impl Into<String>) -> Self {
        self.approval_owner_id = owner_id.into();
        self
    }

    pub fn with_final_response_owner(mut self, owner_id: impl Into<String>) -> Self {
        self.final_response_owner_id = owner_id.into();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationState {
    Created,
    Accepted,
    Running,
    Failed,
    Returned,
    Transferred,
}

impl DelegationState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Accepted => "accepted",
            Self::Running => "running",
            Self::Failed => "failed",
            Self::Returned => "returned",
            Self::Transferred => "transferred",
        }
    }
}

fn default_delegation_state() -> DelegationState {
    DelegationState::Created
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationEnvelope {
    pub session_id: Option<String>,
    #[serde(default = "default_delegation_state")]
    pub state: DelegationState,
    pub visible_owner_id: String,
    pub memory_owner_id: String,
    pub approval_owner_id: String,
    pub final_response_owner_id: String,
    pub delegated_by_id: String,
    pub delegated_to_id: String,
    pub return_mode: DelegationReturnMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_task_id: Option<String>,
}

impl DelegationEnvelope {
    pub fn new(
        ownership: DelegationOwnership,
        delegated_by_id: impl Into<String>,
        delegated_to_id: impl Into<String>,
        return_mode: DelegationReturnMode,
    ) -> Self {
        Self {
            session_id: None,
            state: DelegationState::Created,
            visible_owner_id: ownership.visible_owner_id,
            memory_owner_id: ownership.memory_owner_id,
            approval_owner_id: ownership.approval_owner_id,
            final_response_owner_id: ownership.final_response_owner_id,
            delegated_by_id: delegated_by_id.into(),
            delegated_to_id: delegated_to_id.into(),
            return_mode,
            trace_id: None,
            task_id: None,
            parent_task_id: None,
            root_task_id: None,
        }
    }

    pub fn ownership(&self) -> DelegationOwnership {
        DelegationOwnership {
            visible_owner_id: self.visible_owner_id.clone(),
            memory_owner_id: self.memory_owner_id.clone(),
            approval_owner_id: self.approval_owner_id.clone(),
            final_response_owner_id: self.final_response_owner_id.clone(),
        }
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_trace_context(
        mut self,
        trace_id: Option<String>,
        task_id: Option<String>,
        parent_task_id: Option<String>,
        root_task_id: Option<String>,
    ) -> Self {
        self.trace_id = trace_id;
        self.task_id = task_id;
        self.parent_task_id = parent_task_id;
        self.root_task_id = root_task_id;
        self
    }

    pub fn with_state(mut self, state: DelegationState) -> Self {
        self.state = state;
        self
    }
}

impl AgentManifest {
    pub fn new(id: impl Into<String>, name: impl Into<String>, role: AgentRole) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            role,
            capabilities: Vec::new(),
            address: None,
            status: AgentStatus::Online,
            last_seen: Utc::now(),
            version: "0.3.5".to_string(), // 统一使用 workspace 版本或通过环境变量传入
        }
    }
}

/// Messages exchanged specifically between agents (A2A)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum A2AMessage {
    /// Announcement of an agent's presence in the swarm
    Announcement(AgentManifest),

    /// Request for a task to be performed
    TaskRequest {
        request_id: String,
        requester_id: String,
        task_content: String,
        required_capabilities: Vec<String>,
        #[serde(default)]
        delegation: Option<DelegationEnvelope>,
    },

    /// Bid from an agent willing to perform the task
    Bid {
        request_id: String,
        bidder_id: String,
        bid_amount: f64,
        metadata: Option<String>,
    },

    /// Assignment of a task to a specific bidder
    TaskAssignment {
        request_id: String,
        assigned_to: String,
        task_context: String,
        #[serde(default)]
        delegation: Option<DelegationEnvelope>,
    },

    /// Result of a performed task
    Result {
        request_id: String,
        performer_id: String,
        output: String,
        success: bool,
        #[serde(default)]
        delegation: Option<DelegationEnvelope>,
    },

    /// Phase 12-A: Recursive Handover (Recursive Handover)
    Handover {
        session_id: String,
        from_agent_id: String,
        to_agent_id: String,
        context_summary: String,
    },

    /// Phase 12-B: Collective observability
    Heartbeat {
        agent_id: String,
        status: AgentStatus,
        load: f64,
        timestamp: u64,
    },

    /// System-wide broadcast/advisory
    Broadcast { from_id: String, message: String },
}

impl A2AMessage {
    pub fn new_request(
        requester_id: impl Into<String>,
        task: impl Into<String>,
        required_capabilities: Vec<String>,
    ) -> Self {
        Self::TaskRequest {
            request_id: Uuid::new_v4().to_string(),
            requester_id: requester_id.into(),
            task_content: task.into(),
            required_capabilities,
            delegation: None,
        }
    }

    pub fn new_delegated_request(
        requester_id: impl Into<String>,
        task: impl Into<String>,
        required_capabilities: Vec<String>,
        delegation: DelegationEnvelope,
    ) -> Self {
        Self::TaskRequest {
            request_id: Uuid::new_v4().to_string(),
            requester_id: requester_id.into(),
            task_content: task.into(),
            required_capabilities,
            delegation: Some(delegation),
        }
    }

    pub fn new_assignment(
        request_id: impl Into<String>,
        assigned_to: impl Into<String>,
        task_context: impl Into<String>,
        delegation: DelegationEnvelope,
    ) -> Self {
        Self::TaskAssignment {
            request_id: request_id.into(),
            assigned_to: assigned_to.into(),
            task_context: task_context.into(),
            delegation: Some(delegation),
        }
    }

    pub fn new_result(
        request_id: impl Into<String>,
        performer_id: impl Into<String>,
        output: impl Into<String>,
        success: bool,
        delegation: DelegationEnvelope,
    ) -> Self {
        Self::Result {
            request_id: request_id.into(),
            performer_id: performer_id.into(),
            output: output.into(),
            success,
            delegation: Some(delegation),
        }
    }

    pub fn new_handover(
        session_id: impl Into<String>,
        from_agent_id: impl Into<String>,
        to_agent_id: impl Into<String>,
        context_summary: impl Into<String>,
    ) -> Self {
        Self::Handover {
            session_id: session_id.into(),
            from_agent_id: from_agent_id.into(),
            to_agent_id: to_agent_id.into(),
            context_summary: context_summary.into(),
        }
    }

    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::TaskRequest { request_id, .. } => Some(request_id),
            Self::Bid { request_id, .. } => Some(request_id),
            Self::TaskAssignment { request_id, .. } => Some(request_id),
            Self::Result { request_id, .. } => Some(request_id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegation_helpers_preserve_ownership_and_return_mode() {
        let ownership = DelegationOwnership::new("owner")
            .with_memory_owner("memory-owner")
            .with_approval_owner("approval-owner")
            .with_final_response_owner("final-owner");
        let delegation = DelegationEnvelope::new(
            ownership.clone(),
            "router",
            "worker",
            DelegationReturnMode::ReturnToOwner,
        )
        .with_session_id("session-1")
        .with_trace_context(
            Some("trace-1".to_string()),
            Some("task-1".to_string()),
            Some("task-parent".to_string()),
            Some("task-root".to_string()),
        )
        .with_state(DelegationState::Running);

        assert_eq!(delegation.ownership(), ownership);
        assert_eq!(delegation.session_id.as_deref(), Some("session-1"));
        assert_eq!(delegation.trace_id.as_deref(), Some("trace-1"));
        assert_eq!(delegation.state, DelegationState::Running);

        let message = A2AMessage::new_result("req-1", "worker", "done", true, delegation.clone());
        match message {
            A2AMessage::Result {
                request_id,
                success,
                delegation: Some(inner),
                ..
            } => {
                assert_eq!(request_id, "req-1");
                assert!(success);
                assert_eq!(inner.return_mode, DelegationReturnMode::ReturnToOwner);
                assert_eq!(inner.final_response_owner_id, "final-owner");
            }
            _ => panic!("expected delegated result"),
        }
    }
}
