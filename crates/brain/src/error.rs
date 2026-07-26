use anyhow::Error as AnyhowError;
use thiserror::Error;

/// Result type alias using benshu's Error
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for the benshu framework
#[derive(Debug, Error)]
pub enum Error {
    // ... (variants) ...
    // ============ Agent Errors ============
    /// Agent is not properly configured
    #[error("Agent configuration error: {0}")]
    AgentConfig(String),

    /// Agent execution failed
    #[error("Agent execution error: {0}")]
    AgentExecution(String),

    /// General Authentication error
    #[error("Authentication error: {0}")]
    Auth(String),

    // ============ Provider Errors ============
    /// Provider API error
    #[error("Provider API error: {0}")]
    ProviderApi(String),

    /// Provider authentication failed
    #[error("Provider authentication error: {0}")]
    ProviderAuth(String),

    /// Provider rate limit exceeded
    #[error("Provider rate limit exceeded: retry after {retry_after_secs}s")]
    ProviderRateLimit {
        /// Seconds to wait before retrying
        retry_after_secs: u64,
    },

    // ============ Tool Errors ============
    /// Tool not found in agent's toolset
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Tool execution failed
    #[error("Tool execution error: {tool_name} - {message}")]
    ToolExecution {
        /// Name of the tool that failed
        tool_name: String,
        /// Error message
        message: String,
    },

    /// Tool approval required
    #[error("Tool execution blocked: {tool_name} requires approval but no handler was available")]
    ToolApprovalRequired {
        /// Name of the tool
        tool_name: String,
    },

    /// Invalid tool arguments
    #[error("Invalid tool arguments for {tool_name}: {message}")]
    ToolArguments {
        /// Name of the tool
        tool_name: String,
        /// Error message
        message: String,
    },

    // ============ Message Errors ============
    /// Message parsing failed
    #[error("Message parse error: {0}")]
    MessageParse(String),

    /// Message serialization failed
    #[error("Message serialization error: {0}")]
    MessageSerialize(#[from] serde_json::Error),

    // ============ Streaming Errors ============
    /// Stream interrupted
    #[error("Stream interrupted: {0}")]
    StreamInterrupted(String),

    /// Stream timeout
    #[error("Stream timeout after {timeout_secs}s")]
    StreamTimeout {
        /// Timeout duration in seconds
        timeout_secs: u64,
    },

    // ============ Memory Errors ============
    /// Memory storage error
    #[error("Memory storage error: {0}")]
    MemoryStorage(String),

    /// Memory retrieval error
    #[error("Memory retrieval error: {0}")]
    MemoryRetrieval(String),

    /// Memory consistency contract was broken or had to be rolled back
    #[error("Memory consistency error: {0}")]
    MemoryConsistency(String),

    /// Memory subsystem is available only in degraded mode
    #[error("Memory degraded: {0}")]
    MemoryDegraded(String),

    // ============ Multi-Agent Errors ============
    /// Agent coordination error
    #[error("Agent coordination error: {0}")]
    AgentCoordination(String),

    /// Agent communication error
    #[error("Agent communication error: {0}")]
    AgentCommunication(String),

    // ============ Network Errors ============
    /// HTTP request failed
    #[cfg(feature = "http")]
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    // ============ System Errors ============
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // ============ Generic Errors ============
    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Security violation
    #[error("Security error: {0}")]
    Security(String),

    /// Resource not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// Any other error
    #[error("{0}")]
    Other(#[from] AnyhowError),
    /// Reasoning phase error
    #[error("Reasoning error: {0}")]
    Reasoning(#[from] crate::agent::protocol::ReasoningError),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Validation failed
    #[error("Validation error: {0}")]
    Validation(String),
}

impl Error {
    /// Create a new security error
    pub fn security(msg: impl Into<String>) -> Self {
        Self::Security(msg.into())
    }

    /// Create a new agent configuration error
    pub fn agent_config(msg: impl Into<String>) -> Self {
        Self::AgentConfig(msg.into())
    }

    /// Create a new agent execution error
    pub fn agent_execution(msg: impl Into<String>) -> Self {
        Self::AgentExecution(msg.into())
    }

    /// Create a new agent configuration error
    pub fn config(msg: impl Into<String>) -> Self {
        Self::AgentConfig(msg.into())
    }

    /// Create a new validation error
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    /// Create a new authentication error
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    /// Create a new provider error
    pub fn provider_error(msg: impl Into<String>) -> Self {
        Self::ProviderApi(msg.into())
    }

    /// Create a new tool execution error
    pub fn tool_execution(tool_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ToolExecution {
            tool_name: tool_name.into(),
            message: message.into(),
        }
    }

    /// Get the tool name associated with this error (if any)
    pub fn tool_name(&self) -> &str {
        match self {
            Self::ToolNotFound(name) => name,
            Self::ToolExecution { tool_name, .. } => tool_name,
            Self::ToolApprovalRequired { tool_name, .. } => tool_name,
            Self::ToolArguments { tool_name, .. } => tool_name,
            _ => "unknown",
        }
    }

    /// Get the tool arguments associated with this error (if any)
    pub fn args(&self) -> &str {
        match self {
            Self::ToolArguments { message, .. } => message, // Using message as placeholder for args if not stored
            _ => "",
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::ProviderRateLimit { .. }
            | Self::StreamInterrupted(_)
            | Self::StreamTimeout { .. } => true,
            #[cfg(feature = "http")]
            Self::Http(_) => true,
            _ => false,
        }
    }
}

impl From<crate::agent::protocol::ToolExecutionError> for Error {
    fn from(e: crate::agent::protocol::ToolExecutionError) -> Self {
        use crate::agent::protocol::ToolExecutionError::*;
        match e {
            NotFound(name) => Error::ToolNotFound(name),
            _ => Error::tool_execution("".to_string(), e.to_string()),
        }
    }
}

impl From<crate::agent::protocol::InterventionError> for Error {
    fn from(e: crate::agent::protocol::InterventionError) -> Self {
        Error::agent_config(e.to_string())
    }
}

impl From<benshu_infra::error::Error> for Error {
    fn from(e: benshu_infra::error::Error) -> Self {
        match e {
            benshu_infra::error::Error::Config(s) => Error::AgentConfig(s),
            benshu_infra::error::Error::Auth(s) => Error::Auth(s),
            benshu_infra::error::Error::ProviderAuth(s) => Error::ProviderAuth(s),
            benshu_infra::error::Error::ProviderApi(s) => Error::ProviderApi(s),
            benshu_infra::error::Error::ProviderError(s) => Error::provider_error(s),
            benshu_infra::error::Error::StreamInterrupted(s) => Error::StreamInterrupted(s),
            benshu_infra::error::Error::Internal(s) => Error::Internal(s),
            benshu_infra::error::Error::NotFound(s) => Error::NotFound(s),
            benshu_infra::error::Error::Agent(s) => Error::AgentExecution(s),
            benshu_infra::error::Error::Security(s) => Error::Security(s),
            benshu_infra::error::Error::ToolExecution { tool_name, message } => {
                Error::ToolExecution { tool_name, message }
            }
            benshu_infra::error::Error::ToolNotFound(name) => Error::ToolNotFound(name),
            benshu_infra::error::Error::ToolArguments { tool_name, message } => {
                Error::ToolArguments { tool_name, message }
            }
            benshu_infra::error::Error::Validation(s) => Error::Validation(s),
            benshu_infra::error::Error::Http(e) => Error::Internal(e.to_string()),
            benshu_infra::error::Error::Json(e) => Error::MessageSerialize(e),
            benshu_infra::error::Error::Io(e) => Error::Io(e),
            benshu_infra::error::Error::Other(s) => Error::Internal(s),
        }
    }
}
