use thiserror::Error;

/// Result type for Communication Core
pub type Result<T> = std::result::Result<T, CommError>;

/// Communication Core Error - Unified Hierarchy
#[derive(Debug, Error)]
pub enum CommError {
    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("Routing error: {0}")]
    Routing(#[from] RoutingError),

    #[error("Scheduler error: {0}")]
    Scheduler(#[from] SchedulerError),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Protocol Layer Errors
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Serialization failed: {0}")]
    Serialization(String),

    #[error("Invalid address format: {0}")]
    InvalidAddress(String),

    #[error("Unsupported protocol version: {0}")]
    UnsupportedVersion(u32),

    #[error("Validation failed: {0}")]
    Validation(String),
}

/// Transport Layer Errors
#[derive(Debug, Error)]
pub enum TransportError {
    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Routing error: {0}")]
    Routing(String),

    #[error("Internal transport error: {0}")]
    Internal(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Channel closed")]
    ChannelClosed,
}

/// Routing & Addressing Errors
#[derive(Debug, Error)]
pub enum RoutingError {
    #[error("Destination unreachable: {0}")]
    Unreachable(String),

    #[error("No route found for address: {0}")]
    NoRoute(String),

    #[error("Address lookup failed: {0}")]
    LookupFailed(String),
}

/// Scheduler & Rate Limiting Errors
#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Capacity exceeded: {0}")]
    CapacityExceeded(String),

    #[error("Rate limit throttled: agent_id={agent_id}, limit={limit}")]
    Throttled { agent_id: String, limit: u32 },

    #[error("Task priority mismatch: {0}")]
    PriorityMismatch(String),
}
