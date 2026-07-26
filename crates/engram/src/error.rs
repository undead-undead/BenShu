//! Central error handling system for BenShu Engram
//!
//! Provides a unified, thread-safe error type with categorization and
//! seamless conversion from external dependencies.

use std::fmt;
use std::io;
use thiserror::Error;

/// High-level categories for automated error handling and UI feedback
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ErrorCategory {
    Validation, // Client input, bad paths, size limits
    Storage,    // KV database, file persistence
    Model,      // ML inference, embeddings, pool limits
    Security,   // Vault locks, permission denied
    System,     // IO, resource exhaustion, timeouts
    External,   // Library failures (redb, candle)
    NotFound,   // Missing documents, collections
}

#[derive(Error, Debug)]
pub enum EngramError {
    // --- Validation & Input ---
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Content size exceeds limit: {size} > {max}")]
    ContentTooLarge { size: usize, max: usize },

    #[error("Feature is currently disabled: {0}")]
    FeatureDisabled(String),

    // --- Storage & Database ---
    #[error("Storage failure: {0}")]
    Storage(String),

    #[error("Serialization failure: {0}")]
    Serialization(String),

    #[error("Path violation: {0}")]
    InvalidPath(String),

    #[error("Data conflict: {0}")]
    Conflict(String),

    #[error("Integrity error: {0}")]
    Corrupted(String),

    #[error("Resource already exists: {0}")]
    AlreadyExists(String),

    // --- AI & Model ---
    #[error("Inference failure: {0}")]
    Inference(String),

    #[error("Model load failure: {0}")]
    ModelLoad(String),

    #[error("Model pool error: {0}")]
    ModelPool(String),

    // --- Security & Access ---
    #[error("Vault is locked (encryption key required)")]
    VaultLocked,

    #[error("Permission denied: {0}")]
    Permission(String),

    // --- System & IO ---
    #[error("IO failure: {0}")]
    Io(#[from] io::Error),

    #[error("Operation timeout: {0}")]
    Timeout(String),

    #[error("System failure: {0}")]
    Internal(String),

    // --- Identification ---
    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Retrieval failed: {0}")]
    RetrievalError(String),

    #[error("Invalid timestamp detected: {0}")]
    InvalidTimestamp(String),

    // --- External Bridges ---
    #[error("Dependency error: {0}")]
    External(String),
}

impl EngramError {
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::InvalidInput(_)
            | Self::InvalidPath(_)
            | Self::ContentTooLarge { .. }
            | Self::InvalidTimestamp(_) => ErrorCategory::Validation,
            Self::FeatureDisabled(_) => ErrorCategory::Validation,
            Self::Storage(_) | Self::Conflict(_) | Self::Corrupted(_) | Self::AlreadyExists(_) => {
                ErrorCategory::Storage
            }
            Self::Inference(_) | Self::ModelPool(_) | Self::ModelLoad(_) => ErrorCategory::Model,
            Self::VaultLocked | Self::Permission(_) => ErrorCategory::Security,
            Self::Io(_) | Self::Timeout(_) | Self::Internal(_) | Self::Serialization(_) => {
                ErrorCategory::System
            }
            Self::NotFound(_) => ErrorCategory::NotFound,
            Self::RetrievalError(_) => ErrorCategory::External,
            Self::External(_) => ErrorCategory::External,
        }
    }

    /// Whether the operation can be safely retried
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Conflict(_) | Self::Timeout(_) => true,
            Self::Io(e) => matches!(
                e.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ),
            _ => false,
        }
    }
}

// Result alias for the crate
pub type Result<T> = std::result::Result<T, EngramError>;

// Seamless conversions for third-party libraries
impl From<redb::Error> for EngramError {
    fn from(e: redb::Error) -> Self {
        Self::Storage(format!("redb error: {}", e))
    }
}

impl From<redb::DatabaseError> for EngramError {
    fn from(e: redb::DatabaseError) -> Self {
        Self::Storage(format!("redb database error: {}", e))
    }
}

impl From<redb::TransactionError> for EngramError {
    fn from(e: redb::TransactionError) -> Self {
        Self::Storage(format!("redb transaction error: {}", e))
    }
}

impl From<redb::TableError> for EngramError {
    fn from(e: redb::TableError) -> Self {
        Self::Storage(format!("redb table error: {}", e))
    }
}

impl From<redb::StorageError> for EngramError {
    fn from(e: redb::StorageError) -> Self {
        Self::Storage(format!("redb storage error: {}", e))
    }
}

impl From<redb::CommitError> for EngramError {
    fn from(e: redb::CommitError) -> Self {
        Self::Storage(format!("redb commit error: {}", e))
    }
}

impl From<bincode::Error> for EngramError {
    fn from(e: bincode::Error) -> Self {
        Self::Internal(format!("Serialization failure: {}", e))
    }
}

impl From<serde_json::Error> for EngramError {
    fn from(e: serde_json::Error) -> Self {
        Self::InvalidInput(format!("JSON error: {}", e))
    }
}

impl From<anyhow::Error> for EngramError {
    fn from(e: anyhow::Error) -> Self {
        Self::External(e.to_string())
    }
}

#[cfg(feature = "vector")]
impl From<candle_core::Error> for EngramError {
    fn from(e: candle_core::Error) -> Self {
        Self::Inference(e.to_string())
    }
}

// Context extension trait
pub trait Contextualize<T> {
    fn context(self, msg: &str) -> Result<T>;
}

impl<T, E: Into<EngramError>> Contextualize<T> for std::result::Result<T, E> {
    fn context(self, msg: &str) -> Result<T> {
        self.map_err(|e| {
            let base = e.into();
            EngramError::Internal(format!("{}: {}", msg, base))
        })
    }
}
