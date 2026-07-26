use crate::agent::message::Message;
use crate::error::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Constants for the caching system
pub mod cache_constants {
    /// Default Time-To-Live for cached responses (1 hour)
    pub const DEFAULT_CACHE_TTL_SECS: u64 = 3600;
    /// Interval for periodic cache cleanup (10 minutes)
    pub const CLEANUP_INTERVAL_SECS: u64 = 600;
    /// Maximum number of entries in the in-memory cache
    pub const MAX_CACHE_ENTRIES: usize = 1000;
}

#[derive(Clone)]
struct CacheEntry {
    value: String,
    expires_at: Instant,
}

/// Trait for semantic caching
#[async_trait]
pub trait Cache: Send + Sync {
    /// Get a cached response for the given messages
    async fn get(&self, messages: &[Message]) -> Result<Option<String>>;

    /// Store a response in the cache
    async fn set(&self, messages: &[Message], response: String) -> Result<()>;

    /// Clear the cache
    async fn clear(&self) -> Result<()>;

    /// Optional background cleanup task (protected by periodic interval)
    async fn background_cleanup(&self) -> Result<()> {
        Ok(())
    }
}

/// A simple in-memory implementation of the Cache trait
///
/// Note: This is an exact-match cache for now. Truly 'semantic' caching
/// (vector-based) should be implemented using engram.
pub struct InMemoryCache {
    store: DashMap<String, CacheEntry>,
    ttl: Duration,
    max_size: usize,
}

impl InMemoryCache {
    /// Create a new in-memory cache with default limits
    pub fn new() -> Self {
        Self::with_limits(
            Duration::from_secs(cache_constants::DEFAULT_CACHE_TTL_SECS),
            cache_constants::MAX_CACHE_ENTRIES,
        )
    }

    /// Create a new in-memory cache with custom limits
    pub fn with_limits(ttl: Duration, max_size: usize) -> Self {
        Self {
            store: DashMap::new(),
            ttl,
            max_size,
        }
    }

    /// Periodic cleanup of expired entries
    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        let initial_size = self.store.len();
        self.store.retain(|_, entry| entry.expires_at > now);
        let final_size = self.store.len();

        if initial_size > final_size {
            debug!(
                "Cache cleanup: removed {} expired entries. Current size: {}",
                initial_size - final_size,
                final_size
            );
        }
    }

    /// Evict entries if over capacity (simple LRU-ish approach)
    fn enforce_capacity(&self) {
        if self.store.len() > self.max_size {
            // Find an entry to evict (the one that expires soonest)
            let mut soonest_expiration: Option<(String, Instant)> = None;

            // We sample a few entries instead of full scan for performance
            for entry in self.store.iter().take(10) {
                let (key, val) = (entry.key().clone(), entry.value().expires_at);
                if soonest_expiration.as_ref().map_or(true, |(_, e)| val < *e) {
                    soonest_expiration = Some((key, val));
                }
            }

            if let Some((key, _)) = soonest_expiration {
                self.store.remove(&key);
            }
        }
    }

    /// Generate a simple key based on message content
    fn generate_key(&self, messages: &[Message]) -> String {
        let mut key = String::new();
        for msg in messages {
            key.push_str(msg.role.as_str());
            key.push_str(&msg.text());
        }
        // Hash it for a stable fixed-length key if content is huge
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish().to_string()
    }
}

#[async_trait]
impl Cache for InMemoryCache {
    async fn get(&self, messages: &[Message]) -> Result<Option<String>> {
        let key = self.generate_key(messages);
        let now = Instant::now();

        if let Some(entry) = self.store.get(&key) {
            if entry.expires_at > now {
                return Ok(Some(entry.value.clone()));
            } else {
                // Lazy removal of expired entry
                drop(entry);
                self.store.remove(&key);
            }
        }
        Ok(None)
    }

    async fn set(&self, messages: &[Message], response: String) -> Result<()> {
        let key = self.generate_key(messages);
        let expires_at = Instant::now() + self.ttl;

        self.enforce_capacity();
        self.store.insert(
            key,
            CacheEntry {
                value: response,
                expires_at,
            },
        );
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        self.store.clear();
        Ok(())
    }

    async fn background_cleanup(&self) -> Result<()> {
        self.cleanup_expired();
        Ok(())
    }
}

impl Default for InMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}
