use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Constants for the attempt and retry system.
pub mod attempt_constants {
    /// Default maximum allowed retries for recoverable errors.
    pub const DEFAULT_MAX_RETRIES: u32 = 3;
    /// Ratio of history to keep in compressed mode.
    pub const COMPRESSED_HISTORY_RATIO: f32 = 0.5;
    /// Base backoff duration in milliseconds.
    pub const BACKOFF_BASE_MS: u64 = 1000;
    /// Maximum backoff duration in milliseconds.
    pub const BACKOFF_MAX_MS: u64 = 30000;
}

/// Configuration for a context construction strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Ratio to multiply max_history_messages by.
    pub max_history_ratio: f32,
    /// Whether to enable additional context pruning.
    pub enable_smart_pruning: bool,
    /// Whether to explicitly request conciseness in the prompt.
    pub add_concise_directive: bool,
}

/// Context construction strategy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Strategy {
    /// Standard mode: use default configuration.
    #[default]
    Standard,
    /// Compressed mode: reduce history, enable smart pruning, add concise directive.
    Compressed,
    /// Fallback mode: minimal context.
    Fallback,
}

impl Strategy {
    /// Returns the execution configuration for this strategy.
    pub fn config(&self) -> StrategyConfig {
        match self {
            Strategy::Standard => StrategyConfig {
                max_history_ratio: 1.0,
                enable_smart_pruning: false,
                add_concise_directive: false,
            },
            Strategy::Compressed => StrategyConfig {
                max_history_ratio: attempt_constants::COMPRESSED_HISTORY_RATIO,
                enable_smart_pruning: true,
                add_concise_directive: true,
            },
            Strategy::Fallback => StrategyConfig {
                max_history_ratio: 0.0,
                enable_smart_pruning: true,
                add_concise_directive: true,
            },
        }
    }
}

/// Represents a single attempt to generate a response.
#[derive(Debug, Clone)]
pub struct Attempt {
    /// Unique ID for this attempt chain.
    pub id: Uuid,
    /// Current strategy being used.
    pub strategy: Strategy,
    /// Current retry count.
    pub retry_count: u32,
    /// Maximum allowed retries for network/server errors.
    pub max_retries: u32,
}

impl Attempt {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            strategy: Strategy::Standard,
            retry_count: 0,
            max_retries: attempt_constants::DEFAULT_MAX_RETRIES,
        }
    }

    /// Calculate exponential backoff duration for the current retry.
    pub fn backoff_duration(&self) -> Duration {
        if self.retry_count == 0 {
            return Duration::ZERO;
        }

        let backoff_ms = attempt_constants::BACKOFF_BASE_MS * 2u64.pow(self.retry_count - 1);
        Duration::from_millis(backoff_ms.min(attempt_constants::BACKOFF_MAX_MS))
    }

    /// Check if we can retry based on current count.
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }

    /// Increment retry count.
    pub fn next(&mut self) {
        self.retry_count += 1;
    }

    /// Downgrade strategy for context overflow recovery.
    pub fn downgrade(&mut self) -> bool {
        match self.strategy {
            Strategy::Standard => {
                self.strategy = Strategy::Compressed;
                true
            }
            Strategy::Compressed => {
                self.strategy = Strategy::Fallback;
                true
            }
            Strategy::Fallback => false,
        }
    }
}

impl Default for Attempt {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_configs_match_expected_downgrade_shape() {
        assert_eq!(Strategy::Standard.config().max_history_ratio, 1.0);
        assert_eq!(
            Strategy::Compressed.config().max_history_ratio,
            attempt_constants::COMPRESSED_HISTORY_RATIO
        );
        assert_eq!(Strategy::Fallback.config().max_history_ratio, 0.0);
        assert!(Strategy::Fallback.config().add_concise_directive);
    }

    #[test]
    fn attempt_backoff_and_downgrade_are_bounded() {
        let mut attempt = Attempt::new();

        assert_eq!(attempt.backoff_duration(), Duration::ZERO);
        assert!(attempt.can_retry());
        assert!(attempt.downgrade());
        assert_eq!(attempt.strategy, Strategy::Compressed);
        assert!(attempt.downgrade());
        assert_eq!(attempt.strategy, Strategy::Fallback);
        assert!(!attempt.downgrade());

        attempt.next();
        assert_eq!(attempt.backoff_duration(), Duration::from_millis(1000));
    }
}
