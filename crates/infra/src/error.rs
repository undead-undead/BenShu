use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Authentication error: {0}")]
    Auth(String),
    #[error("Provider authentication error: {0}")]
    ProviderAuth(String),
    #[error("Provider API error: {0}")]
    ProviderApi(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Stream interrupted: {0}")]
    StreamInterrupted(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Agent error: {0}")]
    Agent(String),
    #[error("Security error: {0}")]
    Security(String),
    #[error("Tool execution error: {tool_name} - {message}")]
    ToolExecution { tool_name: String, message: String },
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("Invalid tool arguments for {tool_name}: {message}")]
    ToolArguments { tool_name: String, message: String },
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unexpected error: {0}")]
    Other(String),
}

impl Error {
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    pub fn tool_execution(tool_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ToolExecution {
            tool_name: tool_name.into(),
            message: message.into(),
        }
    }
}
