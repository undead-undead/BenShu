use chrono::Utc;
use dashmap::DashMap;
use redb::ReadableTable;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
#[cfg(feature = "cron")]
use std::sync::Weak;

#[cfg(feature = "persistence")]
use redb::{Database, TableDefinition};

use crate::agent::message::Message;
#[cfg(feature = "cron")]
use benshu_scheduler::Scheduler;

#[cfg(feature = "persistence")]
pub(crate) const STM_MESSAGES_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("stm_messages");
#[cfg(feature = "persistence")]
pub(crate) const STM_SESSIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("stm_sessions");
#[cfg(feature = "persistence")]
pub(crate) const STM_FACTS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("stm_facts_v2");
#[cfg(feature = "persistence")]
pub(crate) const STM_FACT_RELATIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("stm_fact_relations");
#[cfg(feature = "persistence")]
pub(crate) const STM_METADATA_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("stm_metadata");

#[cfg(feature = "persistence")]
const STM_ACCESS_META_PREFIX: &str = "stm.access.";

/// Configuration for ShortTermMemory hygiene and limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortTermMemoryConfig {
    /// Max messages to keep in memory (L1 Working Memory) per user session
    pub max_messages: usize,
    /// Max active users to keep in memory (DoS protection)
    pub max_users: usize,
    /// Persistence path (Must be a .redb file)
    pub path: PathBuf,
}

impl Default for ShortTermMemoryConfig {
    fn default() -> Self {
        Self {
            max_messages: 50,
            max_users: 1000,
            path: PathBuf::from("data/short_term_memory.redb"),
        }
    }
}

/// Short-term memory - stores recent conversation history (Episodic Memory).
pub struct ShortTermMemory {
    pub(crate) max_messages: usize,
    pub(crate) max_users: usize,
    pub(crate) store: DashMap<String, VecDeque<Message>>,
    pub(crate) last_access: DashMap<String, std::time::Instant>,
    pub(crate) metadata_cache: DashMap<String, String>,
    pub(crate) path: PathBuf,
    #[cfg(feature = "persistence")]
    pub(crate) db: Option<Arc<Database>>,
    #[cfg(feature = "cron")]
    pub(crate) scheduler: parking_lot::RwLock<Option<Weak<Scheduler>>>,
    pub(crate) last_interaction_ts: AtomicI64,
    pub(crate) security: parking_lot::RwLock<Option<Arc<dyn crate::security::SecurityHandler>>>,
    pub(crate) emitter:
        Arc<parking_lot::RwLock<Option<Arc<dyn benshu_infra::traits::memory::MemoryEmitter>>>>,
}

impl ShortTermMemory {
    fn metadata_cache_capacity(&self) -> usize {
        self.max_users.saturating_mul(16).clamp(256, 16_384)
    }

    fn evict_metadata_cache_if_needed(&self, incoming_key: &str) {
        if self.metadata_cache.contains_key(incoming_key) {
            return;
        }

        let capacity = self.metadata_cache_capacity();
        while self.metadata_cache.len() >= capacity {
            let Some(entry) = self.metadata_cache.iter().next() else {
                break;
            };
            let key = entry.key().clone();
            drop(entry);
            self.metadata_cache.remove(&key);
        }
    }

    #[cfg(feature = "persistence")]
    fn access_meta_key(session_key: &str) -> String {
        format!("{STM_ACCESS_META_PREFIX}{session_key}")
    }

    /// Create with custom capacity and persistence path
    pub async fn new(max_messages: usize, max_users: usize, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let store = DashMap::new();
        let last_access = DashMap::new();
        let metadata_cache = DashMap::new();

        #[cfg(feature = "persistence")]
        let db = {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }

            match Database::create(&path) {
                Ok(database) => {
                    if let Ok(write_txn) = database.begin_write() {
                        let _ = write_txn.open_table(STM_MESSAGES_TABLE);
                        let _ = write_txn.open_table(STM_SESSIONS_TABLE);
                        let _ = write_txn.open_table(STM_FACTS_TABLE);
                        let _ = write_txn.open_table(STM_METADATA_TABLE);
                        let _ = write_txn
                            .open_table(TableDefinition::<&str, &[u8]>::new("stm_fact_relations"));
                        let _ = write_txn.commit();
                    }
                    Some(Arc::new(database))
                }
                Err(e) => {
                    tracing::error!("Failed to initialize redb at {:?}: {}", path, e);
                    // Use a temporary emitter if available (not yet linked to Self via set_emitter)
                    // In HEM new(), we don't have the emitter yet, so we rely on the caller or a later check.
                    None
                }
            }
        };

        let mem = Self {
            max_messages,
            max_users,
            store,
            last_access,
            metadata_cache,
            path,
            #[cfg(feature = "persistence")]
            db,
            #[cfg(feature = "cron")]
            scheduler: parking_lot::RwLock::new(None),
            last_interaction_ts: AtomicI64::new(chrono::Utc::now().timestamp()),
            security: parking_lot::RwLock::new(None),
            emitter: Arc::new(parking_lot::RwLock::new(None)),
        };

        if let Err(e) = mem.load().await {
            tracing::warn!("Failed to warm-up short-term memory L1 cache: {}", e);
        }

        mem
    }

    /// Load state from Redb (Warm up L1 Cache)
    async fn load(&self) -> crate::error::Result<()> {
        #[cfg(feature = "persistence")]
        if let Some(db) = &self.db {
            let db = db.clone();
            let store = self.store.clone();
            let last_access = self.last_access.clone();
            let l1_limit = self.max_messages;
            let max_users = self.max_users;

            tokio::task::spawn_blocking(move || {
                let read_txn = db.begin_read().map_err(|e| {
                    crate::error::Error::Internal(format!("redb read error: {}", e))
                })?;
                let table = read_txn.open_table(STM_MESSAGES_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb table error: {}", e))
                })?;
                let metadata = read_txn.open_table(STM_METADATA_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb metadata table error: {}", e))
                })?;

                let mut ranked_sessions = Vec::new();
                for entry in metadata.iter().map_err(|e| {
                    crate::error::Error::Internal(format!("redb metadata iter error: {}", e))
                })? {
                    let (key, value) = entry.map_err(|e| {
                        crate::error::Error::Internal(format!("redb metadata entry error: {}", e))
                    })?;
                    let meta_key = key.value();
                    if let Some(session_key) = meta_key.strip_prefix(STM_ACCESS_META_PREFIX) {
                        let touched_at = value.value().parse::<i64>().unwrap_or_default();
                        ranked_sessions.push((touched_at, session_key.to_string()));
                    }
                }

                ranked_sessions.sort_by(|a, b| b.0.cmp(&a.0));
                ranked_sessions.truncate(max_users);

                if ranked_sessions.is_empty() {
                    for entry in table.iter().map_err(|e| {
                        crate::error::Error::Internal(format!("redb iter error: {}", e))
                    })? {
                        let (key, _) = entry.map_err(|e| {
                            crate::error::Error::Internal(format!("redb entry error: {}", e))
                        })?;
                        ranked_sessions.push((0, key.value().to_string()));
                        if ranked_sessions.len() >= max_users {
                            break;
                        }
                    }
                }

                let mut count = 0usize;
                for (_, key_str) in ranked_sessions {
                    let Some(value) = table.get(key_str.as_str()).map_err(|e| {
                        crate::error::Error::Internal(format!("redb get error: {}", e))
                    })?
                    else {
                        continue;
                    };

                    let full_history: VecDeque<Message> = serde_json::from_slice(value.value())
                        .map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "Failed to parse memory for user {}: {}",
                                key_str, e
                            ))
                        })?;

                    let l1_slice: VecDeque<Message> = full_history
                        .iter()
                        .skip(full_history.len().saturating_sub(l1_limit))
                        .cloned()
                        .collect();

                    store.insert(key_str.clone(), l1_slice);
                    last_access.insert(key_str, std::time::Instant::now());
                    count += 1;
                }

                tracing::info!(
                    "HEM: L1 Cache warmed for {} active users from Redb metadata ranking",
                    count
                );
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| crate::error::Error::Internal(format!("Load task panicked: {}", e)))??;

            return Ok(());
        }

        Ok(())
    }

    pub fn key(&self, user_id: &str, agent_id: Option<&str>) -> String {
        match agent_id {
            Some(aid) => format!("{}:{}", user_id, aid),
            None => user_id.to_string(),
        }
    }

    #[cfg(feature = "persistence")]
    pub async fn update_l2_history(
        &self,
        key: String,
        new_messages: Vec<Message>,
        is_undo: bool,
    ) -> crate::error::Result<()> {
        if let Some(db) = &self.db {
            let db = db.clone();
            let session_id = key.clone();
            let persisted_count = if is_undo { 0 } else { new_messages.len() };
            let access_meta_key = Self::access_meta_key(&key);
            let touched_at = Utc::now().timestamp_millis().to_string();

            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn.open_table(STM_MESSAGES_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;

                    let mut history: VecDeque<Message> = match table.get(key.as_str()) {
                        Ok(Some(v)) => serde_json::from_slice(v.value()).unwrap_or_default(),
                        _ => VecDeque::new(),
                    };

                    if is_undo {
                        history.pop_back();
                    } else if !new_messages.is_empty() {
                        for msg in new_messages {
                            if history.len() >= 3000 {
                                history.pop_front();
                            }
                            history.push_back(msg);
                        }
                    }

                    let data = serde_json::to_vec(&history)
                        .map_err(|e| crate::error::Error::Internal(e.to_string()))?;
                    table.insert(key.as_str(), data.as_slice()).map_err(|e| {
                        crate::error::Error::Internal(format!("redb insert error: {}", e))
                    })?;

                    let mut metadata_table =
                        write_txn.open_table(STM_METADATA_TABLE).map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "redb metadata table error: {}",
                                e
                            ))
                        })?;
                    metadata_table
                        .insert(access_meta_key.as_str(), touched_at.as_str())
                        .map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "redb metadata insert error: {}",
                                e
                            ))
                        })?;
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("L2 background task panicked: {}", e))
            })??;

            // Emit a durable L2 persistence signal only for newly written batches.
            if persisted_count > 0 {
                if let Some(emitter_opt) = self.emitter.read().as_ref() {
                    emitter_opt.emit(
                        benshu_infra::traits::memory::MemoryEvent::L2Persisted {
                            session_id,
                            count: persisted_count,
                        },
                        benshu_infra::traits::memory::EventLevel::Info,
                    );
                }
            }
        }
        Ok(())
    }

    pub fn enforce_user_capacity(&self) {
        if self.store.len() < self.max_users {
            return;
        }

        let mut oldest_key = None;
        let mut oldest_time = std::time::Instant::now();

        for r in self.last_access.iter() {
            if *r.value() < oldest_time {
                oldest_time = *r.value();
                oldest_key = Some(r.key().clone());
            }
        }

        if let Some(key) = oldest_key {
            self.store.remove(&key);
            self.last_access.remove(&key);
        }
    }

    pub fn record_interaction_inner(&self) {
        self.last_interaction_ts
            .store(Utc::now().timestamp(), std::sync::atomic::Ordering::Relaxed);
    }

    pub fn last_interaction_elapsed_inner(&self) -> std::time::Duration {
        let ts = self
            .last_interaction_ts
            .load(std::sync::atomic::Ordering::Relaxed);
        let now = Utc::now().timestamp();
        if now > ts {
            std::time::Duration::from_secs((now - ts) as u64)
        } else {
            std::time::Duration::from_secs(0)
        }
    }

    pub async fn retrieve_full_history_inner(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Message>> {
        #[cfg(feature = "persistence")]
        if let Some(db) = &self.db {
            let key = self.key(user_id, agent_id);
            let db = db.clone();
            let history = tokio::task::spawn_blocking(move || {
                let read_txn = db.begin_read().map_err(|e| {
                    crate::error::Error::Internal(format!("redb read error: {}", e))
                })?;
                let table = read_txn.open_table(STM_MESSAGES_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb table error: {}", e))
                })?;

                let value = table
                    .get(key.as_str())
                    .map_err(|e| crate::error::Error::Internal(format!("redb get error: {}", e)))?;
                if let Some(v) = value {
                    let history: std::collections::VecDeque<Message> =
                        serde_json::from_slice(v.value()).map_err(|e| {
                            crate::error::Error::Internal(format!("Failed to parse history: {}", e))
                        })?;
                    Ok::<Option<Vec<Message>>, crate::error::Error>(Some(
                        history.into_iter().collect(),
                    ))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("History read panicked: {}", e))
            })??;

            if let Some(history) = history {
                return Ok(history);
            }
        }

        // Return L1 if L2 not available or empty
        Ok(self.retrieve_inner(user_id, agent_id, 3000))
    }

    pub fn retrieve_inner(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        limit: usize,
    ) -> Vec<Message> {
        let key = self.key(user_id, agent_id);
        self.store
            .get(&key)
            .map(|v| v.iter().rev().take(limit).cloned().rev().collect())
            .unwrap_or_default()
    }

    #[cfg(feature = "persistence")]
    pub async fn sync_l2_snapshot(
        &self,
        key: String,
        messages: VecDeque<Message>,
    ) -> crate::error::Result<()> {
        if let Some(db) = &self.db {
            let db = db.clone();
            let data = serde_json::to_vec(&messages)
                .map_err(|e| crate::error::Error::Internal(e.to_string()))?;
            let access_meta_key = Self::access_meta_key(&key);
            let touched_at = Utc::now().timestamp_millis().to_string();

            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn.open_table(STM_MESSAGES_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    table.insert(key.as_str(), data.as_slice()).map_err(|e| {
                        crate::error::Error::Internal(format!("redb insert snapshot error: {}", e))
                    })?;

                    let mut metadata_table =
                        write_txn.open_table(STM_METADATA_TABLE).map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "redb metadata table error: {}",
                                e
                            ))
                        })?;
                    metadata_table
                        .insert(access_meta_key.as_str(), touched_at.as_str())
                        .map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "redb metadata insert error: {}",
                                e
                            ))
                        })?;
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit snapshot error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| crate::error::Error::Internal(format!("L2 sync panicked: {}", e)))??;
        }
        Ok(())
    }
    pub async fn undo_inner(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Option<Message>> {
        let key = self.key(user_id, agent_id);
        let popped = if let Some(mut entry) = self.store.get_mut(&key) {
            entry.pop_back()
        } else {
            None
        };

        if popped.is_some() {
            #[cfg(feature = "persistence")]
            self.update_l2_history(key, Vec::new(), true).await?;
        }

        Ok(popped)
    }

    pub async fn get_global_cognitive_status(&self) -> crate::error::Result<String> {
        self.get_global_cognitive_status_inner().await
    }

    #[cfg(not(feature = "persistence"))]
    pub async fn get_metadata_inner(&self, key: &str) -> crate::error::Result<Option<String>> {
        Ok(self
            .metadata_cache
            .get(key)
            .map(|value| value.value().clone()))
    }

    #[cfg(feature = "persistence")]
    pub async fn get_metadata_inner(&self, key: &str) -> crate::error::Result<Option<String>> {
        if let Some(value) = self.metadata_cache.get(key) {
            return Ok(Some(value.value().clone()));
        }

        let Some(db) = &self.db else {
            return Ok(self
                .metadata_cache
                .get(key)
                .map(|value| value.value().clone()));
        };

        let db = db.clone();
        let key_owned = key.to_string();
        let value = tokio::task::spawn_blocking(move || {
            let read_txn = db
                .begin_read()
                .map_err(|e| crate::error::Error::Internal(format!("redb read error: {}", e)))?;
            let table = read_txn.open_table(STM_METADATA_TABLE).map_err(|e| {
                crate::error::Error::Internal(format!("redb metadata table error: {}", e))
            })?;
            table
                .get(key_owned.as_str())
                .map_err(|e| {
                    crate::error::Error::Internal(format!("redb metadata get error: {}", e))
                })
                .map(|opt| opt.map(|value| value.value().to_string()))
        })
        .await
        .map_err(|e| crate::error::Error::Internal(format!("Get metadata panicked: {}", e)))??;

        if let Some(ref value) = value {
            self.evict_metadata_cache_if_needed(key);
            self.metadata_cache.insert(key.to_string(), value.clone());
        }

        Ok(value)
    }

    #[cfg(not(feature = "persistence"))]
    pub async fn set_metadata_inner(&self, key: &str, value: &str) -> crate::error::Result<()> {
        self.evict_metadata_cache_if_needed(key);
        self.metadata_cache
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    #[cfg(feature = "persistence")]
    pub async fn set_metadata_inner(&self, key: &str, value: &str) -> crate::error::Result<()> {
        self.evict_metadata_cache_if_needed(key);
        self.metadata_cache
            .insert(key.to_string(), value.to_string());

        let Some(db) = &self.db else {
            return Ok(());
        };

        let db = db.clone();
        let key_owned = key.to_string();
        let value_owned = value.to_string();
        tokio::task::spawn_blocking(move || {
            let write_txn = db
                .begin_write()
                .map_err(|e| crate::error::Error::Internal(format!("redb write error: {}", e)))?;
            {
                let mut table = write_txn.open_table(STM_METADATA_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb metadata table error: {}", e))
                })?;
                table
                    .insert(key_owned.as_str(), value_owned.as_str())
                    .map_err(|e| {
                        crate::error::Error::Internal(format!("redb metadata insert error: {}", e))
                    })?;
            }
            write_txn.commit().map_err(|e| {
                crate::error::Error::Internal(format!("redb metadata commit error: {}", e))
            })?;
            Ok::<(), crate::error::Error>(())
        })
        .await
        .map_err(|e| crate::error::Error::Internal(format!("Set metadata panicked: {}", e)))??;
        Ok(())
    }

    pub async fn clear_inner(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        #[cfg(feature = "persistence")]
        let access_meta_key = Self::access_meta_key(&key);
        self.store.remove(&key);
        self.last_access.remove(&key);
        self.metadata_cache.remove(&key);
        #[cfg(feature = "persistence")]
        self.metadata_cache.remove(&access_meta_key);

        #[cfg(feature = "persistence")]
        if let Some(db) = &self.db {
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn.open_table(STM_MESSAGES_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    table.remove(key.as_str()).map_err(|e| {
                        crate::error::Error::Internal(format!("redb remove error: {}", e))
                    })?;

                    let mut metadata_table =
                        write_txn.open_table(STM_METADATA_TABLE).map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "redb metadata table error: {}",
                                e
                            ))
                        })?;
                    let _ = metadata_table.remove(access_meta_key.as_str());
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| crate::error::Error::Internal(format!("Clear task panicked: {}", e)))??;
        }

        Ok(())
    }

    pub async fn store_session_inner(
        &self,
        session: crate::agent::session::AgentSession,
    ) -> crate::error::Result<()> {
        #[cfg(feature = "persistence")]
        if let Some(db) = &self.db {
            let id = session.id.clone();
            let data = serde_json::to_vec(&session).map_err(|e| {
                crate::error::Error::Internal(format!("Failed to serialize session: {}", e))
            })?;
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn.open_table(STM_SESSIONS_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    table.insert(id.as_str(), data.as_slice()).map_err(|e| {
                        crate::error::Error::Internal(format!("redb insert error: {}", e))
                    })?;
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("Store session panicked: {}", e))
            })??;
            return Ok(());
        }
        Ok(())
    }

    pub async fn retrieve_session_inner(
        &self,
        session_id: &str,
    ) -> crate::error::Result<Option<crate::agent::session::AgentSession>> {
        #[cfg(feature = "persistence")]
        if let Some(db) = &self.db {
            let db = db.clone();
            let session_id = session_id.to_string();
            let session = tokio::task::spawn_blocking(move || {
                let read_txn = db.begin_read().map_err(|e| {
                    crate::error::Error::Internal(format!("redb read error: {}", e))
                })?;
                let table = read_txn.open_table(STM_SESSIONS_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb table error: {}", e))
                })?;

                let value = table
                    .get(session_id.as_str())
                    .map_err(|e| crate::error::Error::Internal(format!("redb get error: {}", e)))?;
                if let Some(v) = value {
                    let session = serde_json::from_slice(v.value()).map_err(|e| {
                        crate::error::Error::Internal(format!(
                            "Failed to parse session {}: {}",
                            session_id, e
                        ))
                    })?;
                    Ok::<Option<crate::agent::session::AgentSession>, crate::error::Error>(Some(
                        session,
                    ))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("Retrieve session panicked: {}", e))
            })??;
            return Ok(session);
        }
        Ok(None)
    }

    pub async fn delete_session_inner(&self, session_id: &str) -> crate::error::Result<()> {
        self.metadata_cache.remove(session_id);
        #[cfg(feature = "persistence")]
        if let Some(db) = &self.db {
            let db = db.clone();
            let session_id = session_id.to_string();
            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn.open_table(STM_SESSIONS_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    table.remove(session_id.as_str()).map_err(|e| {
                        crate::error::Error::Internal(format!("redb remove session error: {}", e))
                    })?;
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit session error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("Delete session panicked: {}", e))
            })??;
        }
        Ok(())
    }

    pub async fn maintenance_inner(&self) -> crate::error::Result<()> {
        self.prune_inactive(std::time::Duration::from_secs(7 * 24 * 3600));
        self.enforce_user_capacity();

        let _ = self
            .prune_messages_inner(std::time::Duration::from_secs(90 * 24 * 3600))
            .await;

        let mut dirty_snapshots = Vec::new();
        for mut entry in self.store.iter_mut() {
            let original_len = entry.value().len();
            entry.value_mut().retain(|msg| {
                let hours_since = (Utc::now() - msg.last_accessed).num_minutes() as f32 / 60.0;
                let recency = (-0.693 * hours_since / 720.0).exp();
                let score = 0.4 * recency + 0.6 * msg.utility_score;
                score > 0.1
            });

            if entry.value().len() != original_len {
                #[cfg(feature = "persistence")]
                if self.db.is_some() {
                    dirty_snapshots.push((entry.key().clone(), entry.value().clone()));
                }
            }
        }

        #[cfg(feature = "persistence")]
        for (key, snapshot) in dirty_snapshots {
            let _ = self.sync_l2_snapshot(key, snapshot).await;
        }

        tracing::debug!("ShortTermMemory maintenance cycle completed (ACID backed)");
        Ok(())
    }

    pub fn prune_inactive(&self, duration: std::time::Duration) {
        let now = std::time::Instant::now();
        self.last_access.retain(|key, last_time| {
            let keep = now.duration_since(*last_time) < duration;
            if !keep {
                self.store.remove(key);
                self.metadata_cache.remove(key);
                #[cfg(feature = "persistence")]
                self.metadata_cache.remove(&Self::access_meta_key(key));
            }
            keep
        });
    }

    pub async fn prune_messages_inner(
        &self,
        older_than: std::time::Duration,
    ) -> crate::error::Result<usize> {
        #[cfg(feature = "persistence")]
        if let Some(db) = &self.db {
            let now = Utc::now();
            let threshold = now
                - chrono::Duration::from_std(older_than).map_err(|_| {
                    crate::error::Error::Internal(
                        "Invalid duration for message pruning ".to_string(),
                    )
                })?;

            let mut keys_to_delete = Vec::new();
            {
                let read_txn = db.begin_read().map_err(|e| {
                    crate::error::Error::Internal(format!("redb read error: {}", e))
                })?;
                let table = read_txn.open_table(STM_MESSAGES_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb table error: {}", e))
                })?;

                for res in table
                    .iter()
                    .map_err(|e| crate::error::Error::Internal(format!("redb iter error: {}", e)))?
                {
                    let (key, value) = res.map_err(|e| {
                        crate::error::Error::Internal(format!("redb next error: {}", e))
                    })?;
                    let msgs: Vec<Message> =
                        serde_json::from_slice(value.value()).unwrap_or_default();
                    if let Some(last) = msgs.last() {
                        if last.created_at < threshold {
                            keys_to_delete.push(key.value().to_string());
                        }
                    }
                }
            }

            let count = keys_to_delete.len();
            if count > 0 {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut msg_table = write_txn.open_table(STM_MESSAGES_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    let mut sess_table = write_txn.open_table(STM_SESSIONS_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    let mut metadata_table =
                        write_txn.open_table(STM_METADATA_TABLE).map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "redb metadata table error: {}",
                                e
                            ))
                        })?;
                    for key in keys_to_delete {
                        let _ = msg_table.remove(key.as_str());
                        let _ = sess_table.remove(key.as_str());
                        let access_meta_key = Self::access_meta_key(&key);
                        let _ = metadata_table.remove(access_meta_key.as_str());
                    }
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit error: {}", e))
                })?;
            }
            return Ok(count);
        }
        Ok(0)
    }

    pub async fn list_sessions_inner(
        &self,
    ) -> crate::error::Result<Vec<crate::agent::session::AgentSession>> {
        #[cfg(feature = "persistence")]
        if let Some(db) = &self.db {
            let db = db.clone();
            let sessions = tokio::task::spawn_blocking(move || {
                let read_txn = db.begin_read().map_err(|e| {
                    crate::error::Error::Internal(format!("redb read error: {}", e))
                })?;
                let table = read_txn.open_table(STM_SESSIONS_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb table error: {}", e))
                })?;

                let mut sessions = Vec::new();
                for res in table
                    .iter()
                    .map_err(|e| crate::error::Error::Internal(format!("redb iter error: {}", e)))?
                {
                    let (_, value) = res.map_err(|e| {
                        crate::error::Error::Internal(format!("redb next error: {}", e))
                    })?;
                    let session: crate::agent::session::AgentSession =
                        serde_json::from_slice(value.value())
                            .map_err(|e| crate::error::Error::Internal(e.to_string()))?;
                    sessions.push(session);
                }
                Ok::<Vec<crate::agent::session::AgentSession>, crate::error::Error>(sessions)
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("List sessions panicked: {}", e))
            })??;
            return Ok(sessions);
        }
        Ok(Vec::new())
    }

    pub async fn mark_cancelled_inner(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        reason: &str,
    ) -> crate::error::Result<()> {
        <Self as crate::agent::memory::Memory>::store(
            self,
            user_id,
            agent_id,
            Message::assistant(format!("Task cancelled: {}", reason)),
        )
        .await
    }

    pub async fn get_global_cognitive_status_inner(&self) -> crate::error::Result<String> {
        #[cfg(feature = "persistence")]
        if self.db.is_none() {
            return Ok("Degraded (L2 Persistence Disabled/Failed)".into());
        }
        Ok("Stable".into())
    }
}
