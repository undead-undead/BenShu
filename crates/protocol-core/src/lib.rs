use benshu_infra::agent::AgentRole;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
    Fact { fact: benshu_memory_core::Fact },
    SystemNotification { notice: String },
    Cancelled { reason: String },
}

impl Content {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn parts(parts: Vec<ContentPart>) -> Self {
        Self::Parts(parts)
    }

    pub fn notification(notice: impl Into<String>) -> Self {
        Self::SystemNotification {
            notice: notice.into(),
        }
    }

    pub fn cancelled(reason: impl Into<String>) -> Self {
        Self::Cancelled {
            reason: reason.into(),
        }
    }

    pub fn fact(fact: benshu_memory_core::Fact) -> Self {
        Self::Fact { fact }
    }

    pub fn as_text(&self) -> String {
        match self {
            Self::Text(t) => t.clone(),
            Self::Parts(parts) => parts
                .iter()
                .map(|p| p.as_text())
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Fact { fact } => format!("[Fact: {}] {}", fact.category, fact.content),
            Self::SystemNotification { notice } => format!("[System] {}", notice),
            Self::Cancelled { reason } => format!("[Cancelled] Reason: {}", reason),
        }
    }

    pub fn soft_trim(&mut self, limit: usize) {
        let char_limit = limit;
        if char_limit < 100 {
            return;
        }

        match self {
            Self::Text(t) => {
                *t = benshu_compression::head_tail_with_notice(
                    t,
                    char_limit,
                    benshu_compression::TruncationNotice::ContextSafety,
                )
                .content;
            }
            Self::Parts(parts) => {
                for part in parts {
                    if let ContentPart::ToolResult { content, .. } = part {
                        *content = benshu_compression::head_tail_with_notice(
                            content,
                            char_limit,
                            benshu_compression::TruncationNotice::ContextSafety,
                        )
                        .content;
                    }
                }
            }
            Self::Fact { .. } | Self::SystemNotification { .. } | Self::Cancelled { .. } => {}
        }
    }

    pub fn hard_clear(&mut self) {
        match self {
            Self::Text(t) => {
                *t = "[CONTENT_CLEARED_TO_SAVE_CONTEXT_WINDOW]".to_string();
            }
            Self::Parts(parts) => {
                for part in parts {
                    match part {
                        ContentPart::Text { text } => {
                            *text = "[TEXT_CLEARED]".to_string();
                        }
                        ContentPart::ToolResult { content, .. } => {
                            *content = "[RESULT_CLEARED]".to_string();
                        }
                        _ => {}
                    }
                }
            }
            Self::Fact { .. } | Self::SystemNotification { .. } | Self::Cancelled { .. } => {}
        }
    }
}

impl From<String> for Content {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for Content {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

impl From<&String> for Content {
    fn from(s: &String) -> Self {
        Self::Text(s.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        tool_call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        content: String,
    },
    Audio {
        source: AudioSource,
    },
    Video {
        source: VideoSource,
    },
}

impl ContentPart {
    pub fn as_text(&self) -> String {
        match self {
            Self::Text { text } => text.clone(),
            Self::ToolCall {
                name, arguments, ..
            } => format!("[Tool Call: {} with args: {}]", name, arguments),
            Self::ToolResult { name, content, .. } => {
                let tool_name = name.as_deref().unwrap_or("unknown_tool");
                format!("[Tool Result: {}] {}", tool_name, content)
            }
            Self::Image { .. } => "[Image Content]".to_string(),
            Self::Audio { .. } => "[Audio Content]".to_string(),
            Self::Video { .. } => "[Video Content]".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VideoSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub unverified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_collection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default = "default_utility")]
    pub utility_score: f32,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub used_experience_ids: Vec<String>,
    #[serde(default)]
    pub used_anti_pattern_ids: Vec<String>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

fn default_confidence() -> f32 {
    1.0
}

fn default_utility() -> f32 {
    0.5
}

fn default_session_contract_version() -> u32 {
    1
}

impl Message {
    pub fn new(role: Role, content: impl Into<Content>) -> Self {
        let now = chrono::Utc::now();
        Self {
            role,
            content: content.into(),
            name: None,
            unverified: false,
            source_collection: None,
            source_path: None,
            utility_score: 0.5,
            last_accessed: now,
            created_at: now,
            confidence: 1.0,
            used_experience_ids: Vec::new(),
            used_anti_pattern_ids: Vec::new(),
            metadata: std::collections::HashMap::new(),
        }
    }

    pub fn system(content: impl Into<Content>) -> Self {
        Self::new(Role::System, content)
    }

    pub fn user(content: impl Into<Content>) -> Self {
        Self::new(Role::User, content)
    }

    pub fn assistant(content: impl Into<Content>) -> Self {
        Self::new(Role::Assistant, content)
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        let content = Content::Parts(vec![ContentPart::ToolResult {
            tool_call_id: tool_call_id.into(),
            name: None,
            content: content.into(),
        }]);
        Self::new(Role::Tool, content)
    }

    pub fn runtime_tool_error_result(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let tool_name = tool_name.into();
        let mut message =
            Self::tool_result(tool_call_id, content).with_tool_name(tool_name.clone());
        message
            .metadata
            .insert("tool_message_kind".to_string(), "error".to_string());
        message
            .metadata
            .insert("tool_error".to_string(), "true".to_string());
        message
            .metadata
            .insert("tool_error_origin".to_string(), "runtime".to_string());
        message.metadata.insert("tool_name".to_string(), tool_name);
        message
    }

    pub fn with_tool_name(mut self, tool_name: impl Into<String>) -> Self {
        let tool_name = tool_name.into();

        if let Content::Parts(parts) = &mut self.content {
            for part in parts {
                if let ContentPart::ToolResult { name, .. } = part {
                    *name = Some(tool_name.clone());
                    break;
                }
            }
        }
        self.metadata
            .insert("tool_name".to_string(), tool_name.clone());
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn text(&self) -> String {
        self.content.as_text()
    }

    pub fn soft_trim(&mut self, limit: usize) {
        self.content.soft_trim(limit);
    }

    pub fn hard_clear(&mut self) {
        self.content.hard_clear();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

impl ToolCall {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    pub fn parse_args<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.arguments.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOwnership {
    pub visible_owner: AgentRole,
    pub execution_owner: AgentRole,
    pub memory_owner: AgentRole,
    pub approval_owner: AgentRole,
    pub final_response_owner: AgentRole,
    pub session_id: Option<String>,
}

impl TaskOwnership {
    pub fn direct(owner: AgentRole, session_id: Option<String>) -> Self {
        Self {
            visible_owner: owner.clone(),
            execution_owner: owner.clone(),
            memory_owner: owner.clone(),
            approval_owner: owner.clone(),
            final_response_owner: owner,
            session_id,
        }
    }

    pub fn prime_owned(
        prime_owner: AgentRole,
        execution_owner: AgentRole,
        session_id: Option<String>,
    ) -> Self {
        Self {
            visible_owner: prime_owner.clone(),
            execution_owner,
            memory_owner: prime_owner.clone(),
            approval_owner: prime_owner.clone(),
            final_response_owner: prime_owner,
            session_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationMode {
    InternalRecommendation,
    InternalAssignment,
    SessionTransfer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationRecord {
    pub delegated_by: AgentRole,
    pub delegated_to: AgentRole,
    pub mode: DelegationMode,
    pub task_owner: AgentRole,
    pub session_id: Option<String>,
    pub summary: Option<String>,
}

/// Status of an agent session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Thinking,
    PendingTools,
    AwaitingClarification {
        clarification: String,
        original_request: String,
    },
    AwaitingApproval {
        tool_name: String,
        arguments: String,
    },
    Executing,
    Completed,
    Failed(String),
}

impl SessionStatus {
    pub fn status_label(&self) -> &'static str {
        match self {
            SessionStatus::AwaitingClarification { .. } => "awaiting_clarification",
            SessionStatus::AwaitingApproval { .. } => "awaiting_approval",
            SessionStatus::Thinking => "thinking",
            SessionStatus::PendingTools => "pending_tools",
            SessionStatus::Executing => "executing",
            SessionStatus::Completed => "completed",
            SessionStatus::Failed(_) => "failed",
        }
    }

    pub fn encode_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    pub fn decode_json(encoded: &str) -> Option<Self> {
        serde_json::from_str(encoded).ok()
    }

    pub fn apply_message_metadata(&self, message: &mut Message) {
        message.metadata.insert(
            "session_status".to_string(),
            self.status_label().to_string(),
        );
        if let Some(encoded) = self.encode_json() {
            message
                .metadata
                .insert("session_status_json".to_string(), encoded);
        }
    }

    pub fn decode_from_message(message: &Message) -> Option<Self> {
        message
            .metadata
            .get("session_status_json")
            .and_then(|value| Self::decode_json(value))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClarificationSessionState {
    pub clarification: String,
    pub original_request: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClarificationSessionEvent {
    Awaiting,
    StatusSurface,
    Resolved,
    Cancelled,
}

impl ClarificationSessionEvent {
    fn status_kind(self) -> &'static str {
        match self {
            Self::Awaiting | Self::StatusSurface => "awaiting_clarification",
            Self::Resolved => "thinking",
            Self::Cancelled => "failed",
        }
    }
}

impl ClarificationSessionState {
    pub fn as_session_status(&self) -> SessionStatus {
        SessionStatus::AwaitingClarification {
            clarification: self.clarification.clone(),
            original_request: self.original_request.clone(),
        }
    }

    pub fn apply_message_metadata(
        &self,
        message: &mut Message,
        event: ClarificationSessionEvent,
        failure_reason: Option<&str>,
    ) {
        let status = match event {
            ClarificationSessionEvent::Awaiting | ClarificationSessionEvent::StatusSurface => {
                self.as_session_status()
            }
            ClarificationSessionEvent::Resolved => SessionStatus::Thinking,
            ClarificationSessionEvent::Cancelled => SessionStatus::Failed(
                failure_reason
                    .unwrap_or("clarification_cancelled")
                    .to_string(),
            ),
        };
        status.apply_message_metadata(message);
        message.metadata.insert(
            "clarification_prompt".to_string(),
            self.clarification.clone(),
        );
        message.metadata.insert(
            "clarification_original_request".to_string(),
            self.original_request.clone(),
        );
        message.metadata.insert(
            "clarification_status_kind".to_string(),
            event.status_kind().to_string(),
        );

        match event {
            ClarificationSessionEvent::Awaiting => {}
            ClarificationSessionEvent::StatusSurface => {
                message.metadata.insert(
                    "clarification_status_surface".to_string(),
                    "true".to_string(),
                );
            }
            ClarificationSessionEvent::Resolved => {
                message
                    .metadata
                    .insert("clarification_resolved".to_string(), "true".to_string());
            }
            ClarificationSessionEvent::Cancelled => {
                message
                    .metadata
                    .insert("clarification_cancelled".to_string(), "true".to_string());
                if let Some(reason) = failure_reason.filter(|value| !value.trim().is_empty()) {
                    message.metadata.insert(
                        "clarification_failure_reason".to_string(),
                        reason.to_string(),
                    );
                }
            }
        }
    }

    pub fn recover_from_history(history: &[Message]) -> Option<Self> {
        for message in history.iter().rev() {
            if let Some(status) = SessionStatus::decode_from_message(message) {
                match status {
                    SessionStatus::Thinking => {
                        if message.metadata.contains_key("clarification_resolved") {
                            return None;
                        }
                    }
                    SessionStatus::Failed(_) => {
                        if message.metadata.contains_key("clarification_cancelled") {
                            return None;
                        }
                    }
                    SessionStatus::AwaitingClarification {
                        clarification,
                        original_request,
                    } => {
                        return Some(Self {
                            clarification,
                            original_request,
                        });
                    }
                    _ => {}
                }
            }

            let session_status = message.metadata.get("session_status").map(String::as_str);
            match session_status {
                Some("thinking") if message.metadata.contains_key("clarification_resolved") => {
                    return None;
                }
                Some("failed") if message.metadata.contains_key("clarification_cancelled") => {
                    return None;
                }
                Some("awaiting_clarification") => {
                    let clarification = message.metadata.get("clarification_prompt")?.clone();
                    let original_request = message
                        .metadata
                        .get("clarification_original_request")?
                        .clone();
                    return Some(Self {
                        clarification,
                        original_request,
                    });
                }
                _ => {}
            }
        }
        None
    }
}

/// Lifecycle metadata for archive / recovery / retention semantics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionLifecycle {
    #[serde(default = "default_session_contract_version")]
    pub contract_version: u32,
    #[serde(default)]
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub archive_reason: Option<String>,
    #[serde(default)]
    pub retention_until: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub last_recovered_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub recovered_from: Option<String>,
}

impl Default for SessionLifecycle {
    fn default() -> Self {
        Self {
            contract_version: default_session_contract_version(),
            archived_at: None,
            archive_reason: None,
            retention_until: None,
            last_recovered_at: None,
            recovered_from: None,
        }
    }
}

/// A persistent session representing an agent's current state and history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    /// Unique identifier for the session.
    pub id: String,
    /// Dialogue history.
    pub messages: Vec<Message>,
    /// Current step in the reasoning loop.
    pub step: usize,
    /// Current status of the agent.
    pub status: SessionStatus,
    /// Timestamp of the last update.
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Whether this session has been distilled into facts.
    #[serde(default)]
    pub is_distilled: bool,
    /// Skills created in this session that should not be cleaned up.
    #[serde(default)]
    pub hardened_skills: Vec<String>,
    /// Associated agent role for multi-agent routing.
    pub agent_role: Option<String>,
    /// Maximum allowed steps before automatic termination.
    pub max_steps: usize,
    /// History of executed tool names for resuming logic.
    #[serde(default)]
    pub executed_tools: Vec<String>,
    /// Lifecycle metadata for archive / recovery / retention policies.
    #[serde(default)]
    pub lifecycle: SessionLifecycle,
    /// Persistent background persona/session layer carried with this session.
    #[serde(default)]
    pub background_envelope: Option<benshu_memory_core::BackgroundEnvelope>,
}

impl AgentSession {
    /// Create a new session.
    pub fn new(id: String) -> Self {
        Self {
            id,
            messages: Vec::new(),
            step: 0,
            status: SessionStatus::Thinking,
            updated_at: chrono::Utc::now(),
            is_distilled: false,
            hardened_skills: Vec::new(),
            agent_role: None,
            max_steps: 10,
            executed_tools: Vec::new(),
            lifecycle: SessionLifecycle::default(),
            background_envelope: None,
        }
    }

    /// Archive a session while preserving it for later recovery until retention expires.
    pub fn archive(
        &mut self,
        reason: Option<String>,
        retention_until: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        self.lifecycle.archived_at = Some(chrono::Utc::now());
        self.lifecycle.archive_reason = reason.filter(|value| !value.trim().is_empty());
        self.lifecycle.retention_until = retention_until;
        self.updated_at = chrono::Utc::now();
    }

    /// Mark a session as recovered from a lower/secondary storage layer.
    pub fn mark_recovered(&mut self, source: impl Into<String>) {
        self.lifecycle.last_recovered_at = Some(chrono::Utc::now());
        self.lifecycle.recovered_from = Some(source.into());
        self.updated_at = chrono::Utc::now();
    }

    pub fn is_archived(&self) -> bool {
        self.lifecycle.archived_at.is_some()
    }

    pub fn retention_expired_at(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.is_archived()
            && self
                .lifecycle
                .retention_until
                .map(|deadline| deadline <= now)
                .unwrap_or(false)
    }

    /// Update session status with state transition validation.
    pub fn set_status(&mut self, new_status: SessionStatus) -> benshu_infra::error::Result<()> {
        use SessionStatus::*;

        let valid = match (&self.status, &new_status) {
            (s1, s2) if s1 == s2 => true,
            (Thinking, PendingTools) => true,
            (Thinking, AwaitingClarification { .. }) => true,
            (Thinking, Completed) => true,
            (Thinking, Failed(_)) => true,
            (PendingTools, AwaitingClarification { .. }) => true,
            (PendingTools, AwaitingApproval { .. }) => true,
            (PendingTools, Executing) => true,
            (PendingTools, Failed(_)) => true,
            (AwaitingClarification { .. }, Thinking) => true,
            (AwaitingClarification { .. }, Failed(_)) => true,
            (AwaitingApproval { .. }, Executing) => true,
            (AwaitingApproval { .. }, Failed(_)) => true,
            (Executing, Thinking) => true,
            (Executing, Completed) => true,
            (Executing, Failed(_)) => true,
            (Completed, Thinking) => true,
            (Failed(_), Thinking) => true,
            _ => false,
        };

        if !valid {
            return Err(benshu_infra::error::Error::Agent(format!(
                "Invalid state transition for session {}: {:?} -> {:?}",
                self.id, self.status, new_status
            )));
        }

        self.status = new_status;
        self.updated_at = chrono::Utc::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_creation_uses_expected_role() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text(), "Hello");
    }

    #[test]
    fn tool_call_parse_decodes_typed_arguments() {
        #[derive(Deserialize)]
        struct SwapArgs {
            from: String,
            to: String,
            amount: f64,
        }

        let call = ToolCall::new(
            "call_123",
            "swap_tokens",
            serde_json::json!({
                "from": "USDC",
                "to": "SOL",
                "amount": 100.0
            }),
        );

        let args: SwapArgs = call.parse_args().expect("parse should succeed");
        assert_eq!(args.from, "USDC");
        assert_eq!(args.to, "SOL");
        assert!((args.amount - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tool_result_name_is_added_to_part_and_metadata() {
        let msg = Message::tool_result("call_1", "result").with_tool_name("get_price");
        assert_eq!(
            msg.metadata.get("tool_name"),
            Some(&"get_price".to_string())
        );
        if let Content::Parts(parts) = msg.content {
            if let ContentPart::ToolResult { name, .. } = &parts[0] {
                assert_eq!(name.as_deref(), Some("get_price"));
            } else {
                panic!("Wrong part type");
            }
        }
    }

    #[test]
    fn runtime_tool_error_result_sets_structured_metadata() {
        let msg = Message::runtime_tool_error_result("call_1", "web_search", "Runtime tool error");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(
            msg.metadata.get("tool_message_kind"),
            Some(&"error".to_string())
        );
        assert_eq!(msg.metadata.get("tool_error"), Some(&"true".to_string()));
        assert_eq!(
            msg.metadata.get("tool_error_origin"),
            Some(&"runtime".to_string())
        );
        assert_eq!(
            msg.metadata.get("tool_name"),
            Some(&"web_search".to_string())
        );
    }

    #[test]
    fn task_ownership_keeps_prime_visible_owner() {
        let prime = AgentRole::Custom("benshu".to_string());
        let worker = AgentRole::Custom("researcher".to_string());
        let ownership =
            TaskOwnership::prime_owned(prime.clone(), worker.clone(), Some("s1".to_string()));

        assert_eq!(ownership.visible_owner, prime);
        assert_eq!(ownership.execution_owner, worker);
        assert_eq!(ownership.session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn clarification_state_round_trips_through_message_metadata() {
        let state = ClarificationSessionState {
            clarification: "需要哪个文件？".to_string(),
            original_request: "帮我总结".to_string(),
        };
        let mut message = Message::assistant("我需要确认一下");
        state.apply_message_metadata(&mut message, ClarificationSessionEvent::Awaiting, None);

        let recovered = ClarificationSessionState::recover_from_history(&[message])
            .expect("state should recover");
        assert_eq!(recovered, state);
    }
}
