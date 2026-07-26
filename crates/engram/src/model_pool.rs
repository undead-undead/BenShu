use crate::error::{EngramError, Result};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, trace, warn};

#[cfg(feature = "vector")]
use crate::embedder::Embedder;
#[cfg(feature = "vector")]
use crate::local_reranker::LocalCandleReranker;

/// Types of models managed by the pool
#[derive(Clone, Debug)]
pub enum ModelResource {
    #[cfg(feature = "vector")]
    Embedder(Arc<Embedder>),
    #[cfg(feature = "vector")]
    Reranker(Arc<LocalCandleReranker>),
    // Future expansion:
    // Whisper(Arc<WhisperModel>),
    // Piper(Arc<PiperModel>),
}

impl ModelResource {
    /// Get memory usage in bytes
    pub fn memory_size(&self) -> usize {
        match self {
            #[cfg(feature = "vector")]
            ModelResource::Embedder(e) => e.memory_size(),
            #[cfg(feature = "vector")]
            ModelResource::Reranker(r) => r.memory_size(),
        }
    }

    /// Check if the model is running on GPU (using VRAM)
    pub fn is_gpu(&self) -> bool {
        match self {
            #[cfg(feature = "vector")]
            ModelResource::Embedder(e) => e.is_gpu(),
            #[cfg(feature = "vector")]
            ModelResource::Reranker(r) => r.is_gpu(),
        }
    }

    /// Get model type name for logging/monitoring
    pub fn type_name(&self) -> &'static str {
        match self {
            #[cfg(feature = "vector")]
            ModelResource::Embedder(_) => "Embedder",
            #[cfg(feature = "vector")]
            ModelResource::Reranker(_) => "Reranker",
        }
    }
}

/// Entry for a model in the pool
#[derive(Debug)]
struct PoolEntry {
    resource: ModelResource,
    last_used: Instant,
    /// Loading timestamp
    loaded_at: Instant,
    /// Frequency counter for smarter eviction
    access_count: u64,
}

impl PoolEntry {
    fn new(resource: ModelResource) -> Self {
        let now = Instant::now();
        Self {
            resource,
            last_used: now,
            loaded_at: now,
            access_count: 0,
        }
    }

    /// Refresh usage timestamp and increment counter
    fn touch(&mut self) {
        self.last_used = Instant::now();
        self.access_count += 1;
    }
}

/// Memory/VRAM usage statistics
#[derive(Debug, Clone, Copy)]
pub struct MemoryStats {
    /// Bytes used
    pub used: usize,
    /// Max budget in bytes
    pub max: usize,
    /// Ratio (0.0 - 1.0)
    pub usage_ratio: f32,
}

/// A centralized pool for managing local AI models with LRU/Frequency eviction.
pub struct ModelPool {
    /// Max RAM budget in bytes
    max_ram: Mutex<usize>,
    /// Max VRAM budget in bytes
    max_vram: Mutex<usize>,
    /// Loaded models with RwLock for high-performance concurrent reads
    entries: RwLock<HashMap<String, PoolEntry>>,
    /// Locks to prevent concurrent loading of the same model
    loading_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl ModelPool {
    /// Create a new model pool with dual memory budgets.
    pub fn new(max_ram: usize, max_vram: usize) -> Self {
        info!(
            "Initializing ModelPool - RAM: {} MB, VRAM: {} MB",
            max_ram / 1024 / 1024,
            max_vram / 1024 / 1024
        );

        Self {
            max_ram: Mutex::new(max_ram),
            max_vram: Mutex::new(max_vram),
            entries: RwLock::new(HashMap::new()),
            loading_locks: Mutex::new(HashMap::new()),
        }
    }

    /// Update budgets at runtime
    pub fn set_budgets(&self, ram_bytes: usize, vram_bytes: usize) {
        let old_ram = *self.max_ram.lock();
        let old_vram = *self.max_vram.lock();

        *self.max_ram.lock() = ram_bytes;
        *self.max_vram.lock() = vram_bytes;

        info!(
            "ModelPool budgets updated - RAM: {} -> {} MB, VRAM: {} -> {} MB",
            old_ram / 1024 / 1024,
            ram_bytes / 1024 / 1024,
            old_vram / 1024 / 1024,
            vram_bytes / 1024 / 1024
        );

        // Enforce new limits immediately
        self.evict_for_space(0, false);
        self.evict_for_space(0, true);
    }

    /// Get combined memory/VRAM usage stats
    pub fn get_memory_stats(&self) -> (MemoryStats, MemoryStats) {
        let entries = self.entries.read();
        let max_ram = *self.max_ram.lock();
        let max_vram = *self.max_vram.lock();

        let mut ram_used = 0;
        let mut vram_used = 0;

        for e in entries.values() {
            let size = e.resource.memory_size();
            if e.resource.is_gpu() {
                vram_used += size;
            } else {
                ram_used += size;
            }
        }

        let ram_stats = MemoryStats {
            used: ram_used,
            max: max_ram,
            usage_ratio: if max_ram > 0 {
                ram_used as f32 / max_ram as f32
            } else {
                0.0
            },
        };

        let vram_stats = MemoryStats {
            used: vram_used,
            max: max_vram,
            usage_ratio: if max_vram > 0 {
                vram_used as f32 / max_vram as f32
            } else {
                0.0
            },
        };

        debug!(
            "ModelPool Stats - RAM: {:.1}% ({} MB), VRAM: {:.1}% ({} MB)",
            ram_stats.usage_ratio * 100.0,
            ram_stats.used / 1024 / 1024,
            vram_stats.usage_ratio * 100.0,
            vram_stats.used / 1024 / 1024
        );

        (ram_stats, vram_stats)
    }

    pub fn current_usage(&self) -> (usize, usize) {
        let (ram, vram) = self.get_memory_stats();
        (ram.used, vram.used)
    }

    pub fn is_whisper_loaded(&self) -> bool {
        self.entries
            .read()
            .keys()
            .any(|k| k.to_lowercase().contains("whisper"))
    }

    pub fn is_piper_loaded(&self) -> bool {
        self.entries
            .read()
            .keys()
            .any(|k| k.to_lowercase().contains("piper"))
    }

    pub fn is_model_loaded(&self, key: &str) -> bool {
        self.entries.read().contains_key(key)
    }

    pub fn loaded_models_count(&self) -> usize {
        self.entries.read().len()
    }

    pub fn list_loaded_models(&self) -> Vec<String> {
        self.entries.read().keys().cloned().collect()
    }

    /// Get detailed model info for dashboard
    pub fn get_model_info(&self, key: &str) -> Option<(String, usize, bool, Duration, u64)> {
        let entries = self.entries.read();
        entries.get(key).map(|entry| {
            (
                entry.resource.type_name().to_string(),
                entry.resource.memory_size(),
                entry.resource.is_gpu(),
                entry.last_used.elapsed(),
                entry.access_count,
            )
        })
    }

    /// Evict models until enough space is available.
    fn evict_for_space(&self, required_size: usize, is_gpu: bool) {
        let limit = if is_gpu {
            *self.max_vram.lock()
        } else {
            *self.max_ram.lock()
        };

        if required_size > limit {
            warn!(
                "Required size ({} MB) exceeds {} budget ({} MB)",
                required_size / 1024 / 1024,
                if is_gpu { "VRAM" } else { "RAM" },
                limit / 1024 / 1024
            );
            return;
        }

        let mut entries = self.entries.write();

        loop {
            let current_usage: usize = entries
                .values()
                .filter(|e| e.resource.is_gpu() == is_gpu)
                .map(|e| e.resource.memory_size())
                .sum();

            if current_usage + required_size <= limit {
                break;
            }

            // Weighted Eviction: Balance LRU time with access frequency
            let evict_candidate = entries
                .iter_mut()
                .filter(|(_, entry)| entry.resource.is_gpu() == is_gpu)
                .min_by_key(|(_, entry)| {
                    let idle_ms = entry.last_used.elapsed().as_millis() as u64;
                    // Boost survival of frequently accessed models
                    let frequency_bonus = entry.access_count / 10;
                    idle_ms.saturating_sub(frequency_bonus)
                })
                .map(|(key, _)| key.clone());

            match evict_candidate {
                Some(key) => {
                    if let Some(removed) = entries.remove(&key) {
                        info!(
                            "Evicted model '{}' ({} MB from {}) to free space",
                            key,
                            removed.resource.memory_size() / 1024 / 1024,
                            if is_gpu { "VRAM" } else { "RAM" }
                        );
                    }
                }
                None => break,
            }
        }
    }

    /// Get or create a lock for loading a specific model to prevent Stampede
    fn get_loading_lock(&self, key: &str) -> Arc<Mutex<()>> {
        let mut loading_locks = self.loading_locks.lock();
        loading_locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    #[cfg(feature = "vector")]
    pub fn get_embedder(
        &self,
        key: &str,
        loader: impl FnOnce() -> Result<Embedder>,
    ) -> Result<Arc<Embedder>> {
        // Fast path: Check existing loaded model
        {
            let entries = self.entries.read();
            if let Some(entry) = entries.get(key) {
                if let ModelResource::Embedder(ref emb) = entry.resource {
                    let arc = Arc::clone(emb);
                    drop(entries);
                    // Update metadata
                    let mut entries = self.entries.write();
                    if let Some(entry) = entries.get_mut(key) {
                        entry.touch();
                    }
                    trace!("Cache hit: embedder '{}'", key);
                    return Ok(arc);
                }
            }
        }

        // Slow path: Load under lock
        let loading_lock = self.get_loading_lock(key);
        let _guard = loading_lock.lock();

        // Double-check after acquiring lock
        {
            let entries = self.entries.read();
            if let Some(entry) = entries.get(key) {
                if let ModelResource::Embedder(ref emb) = entry.resource {
                    return Ok(Arc::clone(emb));
                }
            }
        }

        trace!("Loading embedder '{}' into the pool", key);
        let model = loader()?;
        let size = model.memory_size();
        let is_gpu = model.is_gpu();

        self.evict_for_space(size, is_gpu);

        let mut entries = self.entries.write();
        let arc_model = Arc::new(model);
        entries.insert(
            key.to_string(),
            PoolEntry::new(ModelResource::Embedder(Arc::clone(&arc_model))),
        );

        // Clean up loading lock entry
        self.loading_locks.lock().remove(key);

        Ok(arc_model)
    }

    #[cfg(feature = "vector")]
    pub fn get_reranker(
        &self,
        key: &str,
        loader: impl FnOnce() -> Result<LocalCandleReranker>,
    ) -> Result<Arc<LocalCandleReranker>> {
        // Fast path
        {
            let entries = self.entries.read();
            if let Some(entry) = entries.get(key) {
                if let ModelResource::Reranker(ref rerank) = entry.resource {
                    let arc = Arc::clone(rerank);
                    drop(entries);
                    // Update metadata
                    let mut entries = self.entries.write();
                    if let Some(entry) = entries.get_mut(key) {
                        entry.touch();
                    }
                    trace!("Cache hit: reranker '{}'", key);
                    return Ok(arc);
                }
            }
        }

        // Slow path
        let loading_lock = self.get_loading_lock(key);
        let _guard = loading_lock.lock();

        {
            let entries = self.entries.read();
            if let Some(entry) = entries.get(key) {
                if let ModelResource::Reranker(ref rerank) = entry.resource {
                    return Ok(Arc::clone(rerank));
                }
            }
        }

        trace!("Loading reranker '{}' into the pool", key);
        let model = loader()?;
        let size = model.memory_size();
        let is_gpu = model.is_gpu();

        self.evict_for_space(size, is_gpu);

        let mut entries = self.entries.write();
        let arc_model = Arc::new(model);
        entries.insert(
            key.to_string(),
            PoolEntry::new(ModelResource::Reranker(Arc::clone(&arc_model))),
        );

        self.loading_locks.lock().remove(key);

        Ok(arc_model)
    }

    /// Prune models that haven't been used for `timeout_secs`.
    pub fn prune(&self, timeout_secs: u64) -> usize {
        let timeout = Duration::from_secs(timeout_secs);
        let mut entries = self.entries.write();
        let mut to_remove = Vec::new();

        for (key, entry) in entries.iter() {
            if entry.last_used.elapsed() >= timeout {
                to_remove.push(key.clone());
            }
        }

        let count = to_remove.len();
        for key in to_remove {
            if let Some(removed) = entries.remove(&key) {
                info!(
                    "Pruned idle model '{}' ({} MB, {} type) after {}s idle",
                    key,
                    removed.resource.memory_size() / 1024 / 1024,
                    removed.resource.type_name(),
                    timeout_secs
                );
                // Also remove loading lock
                self.loading_locks.lock().remove(&key);
            }
        }
        count
    }

    /// Forcibly unload a specific model
    pub fn unload_model(&self, key: &str) -> bool {
        let mut entries = self.entries.write();
        if entries.remove(key).is_some() {
            info!("Forcibly unloaded model '{}'", key);
            self.loading_locks.lock().remove(key);
            true
        } else {
            false
        }
    }

    /// Clear all models from the pool
    pub fn clear(&self) -> usize {
        let mut entries = self.entries.write();
        let count = entries.len();
        entries.clear();
        self.loading_locks.lock().clear();
        count
    }
}

impl Drop for ModelPool {
    fn drop(&mut self) {
        let count = self.clear();
        if count > 0 {
            info!("ModelPool dropped, cleared {} models", count);
        }
    }
}
