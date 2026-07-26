use serde::{Deserialize, Serialize};

/// Human-Agent Interaction Message (B2H)
///
/// Specialized for client-facing interaction (UI/CLI/WebSocket)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum B2HMessage {
    /// Initial handshake from a human client
    Handshake { user_id: String, platform: String },

    /// User input message
    UserMessage { session_id: String, content: String },

    /// Agent output message (B2H response)
    AgentResponse {
        session_id: String,
        content: String,
        is_final: bool,
    },

    /// System alert or notification for human
    Notification { level: String, message: String },
}
