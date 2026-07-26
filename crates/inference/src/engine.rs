//! Inference Engine: lightweight KV page bookkeeping.

use std::collections::HashMap;
use std::sync::TryLockError;
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Configuration for the Inference Engine
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    pub page_size: usize,    // Number of tokens per page
    pub num_pages: usize,    // Total pages in the pool
    pub head_dim: usize,     // Dimension per head
    pub num_heads: usize,    // Number of query attention heads
    pub num_kv_heads: usize, // Number of KV heads (GQA Support)
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            page_size: 16,
            num_pages: 1024,
            head_dim: 128,
            num_heads: 32,
            num_kv_heads: 8,
        }
    }
}

/// A single cache page in the inference engine
#[derive(Clone)]
pub struct CachePage {
    pub id: usize,
    pub k_data: Vec<u8>,
    pub v_data: Vec<u8>,
    pub last_access: u64,
    /// Scaled metadata (Memory optimized)
    pub k_min: f32,
    pub k_max: f32,
    pub v_min: f32,
    pub v_max: f32,
}

impl CachePage {
    pub fn reset(&mut self, config: &InferenceConfig) {
        let size = config.num_kv_heads * config.page_size * config.head_dim * 2;
        self.k_data.resize(size, 0);
        self.v_data.resize(size, 0);
        self.k_data.fill(0);
        self.v_data.fill(0);
        self.last_access = 0;
        self.k_min = 0.0;
        self.k_max = 0.0;
        self.v_min = 0.0;
        self.v_max = 0.0;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EngineLoad {
    pub avg_cpu_usage: f32,
    pub last_updated: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EngineError {
    #[error("No free KV pages available")]
    OutOfMemory,
    #[error("No free KV pages available and no reusable page could be found")]
    NoReusablePage,
    #[error("KV page allocation is temporarily blocked by page contention")]
    Contention,
    #[error("Request '{0}' does not exist in the KV engine")]
    UnknownRequest(String),
}

/// KvEngine: The unified memory engine for model inference
pub struct KvEngine {
    config: InferenceConfig,
    pages: Vec<Arc<RwLock<CachePage>>>,
    request_map: HashMap<String, Vec<usize>>,
    free_pages: Vec<usize>,
    sys: sysinfo::System,
    load_snapshot: EngineLoad,
}

impl KvEngine {
    pub fn new(config: InferenceConfig) -> Self {
        let mut pages = Vec::with_capacity(config.num_pages);
        let mut free_pages = Vec::with_capacity(config.num_pages);
        let kv_size = config.num_kv_heads * config.page_size * config.head_dim * 2;

        for i in 0..config.num_pages {
            let page = CachePage {
                id: i,
                k_data: vec![0u8; kv_size],
                v_data: vec![0u8; kv_size],
                last_access: 0,
                k_min: 0.0,
                k_max: 0.0,
                v_min: 0.0,
                v_max: 0.0,
            };
            pages.push(Arc::new(RwLock::new(page)));
            free_pages.push(i);
        }

        Self {
            config,
            pages,
            request_map: HashMap::new(),
            free_pages,
            sys: sysinfo::System::new_all(),
            load_snapshot: EngineLoad {
                avg_cpu_usage: 0.0,
                last_updated: 0,
            },
        }
    }

    pub fn update_load(&mut self) {
        self.sys.refresh_cpu_usage();
        let cpus = self.sys.cpus();
        let avg_usage = if cpus.is_empty() {
            0.0
        } else {
            cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpus.len() as f32
        };
        self.load_snapshot = EngineLoad {
            avg_cpu_usage: avg_usage,
            last_updated: self.get_now_ms(),
        };
    }

    pub fn try_allocate_page(&mut self, request_id: &str) -> Result<usize, EngineError> {
        let now = self.get_now_ms();

        if let Some(page_id) = self.free_pages.pop() {
            let mut page = self.pages[page_id]
                .write()
                .map_err(|_| EngineError::Contention)?;
            page.reset(&self.config);
            page.last_access = now;
            self.request_map
                .entry(request_id.to_string())
                .or_default()
                .push(page_id);
            return Ok(page_id);
        }

        let (best_to_evict, saw_contention) = self.find_evictable_page();

        if let Some(evict_id) = best_to_evict {
            let mut empty_req = None;
            for (req, ids) in self.request_map.iter_mut() {
                if let Some(pos) = ids.iter().position(|&x| x == evict_id) {
                    ids.remove(pos);
                    if ids.is_empty() {
                        empty_req = Some(req.clone());
                    }
                    break;
                }
            }
            if let Some(r) = empty_req {
                self.request_map.remove(&r);
            }

            let mut page = self.pages[evict_id]
                .write()
                .map_err(|_| EngineError::Contention)?;
            page.reset(&self.config);
            page.last_access = now;
            self.request_map
                .entry(request_id.to_string())
                .or_default()
                .push(evict_id);
            return Ok(evict_id);
        }

        if saw_contention {
            Err(EngineError::Contention)
        } else {
            Err(EngineError::NoReusablePage)
        }
    }

    pub fn allocate_page(&mut self, request_id: &str) -> Option<usize> {
        self.try_allocate_page(request_id).ok()
    }

    fn get_now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn release_request(&mut self, request_id: &str) {
        if let Some(ids) = self.request_map.remove(request_id) {
            for id in ids {
                if let Ok(mut page) = self.pages[id].write() {
                    page.reset(&self.config);
                }
                self.free_pages.push(id);
            }
        }
    }

    pub fn get_request_pages(&self, request_id: &str) -> Vec<Arc<RwLock<CachePage>>> {
        let now = self.get_now_ms();
        self.request_map
            .get(request_id)
            .map(|ids| {
                ids.iter()
                    .map(|&id| {
                        if let Ok(mut page) = self.pages[id].write() {
                            page.last_access = now;
                        }
                        Arc::clone(&self.pages[id])
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn find_evictable_page(&self) -> (Option<usize>, bool) {
        let mut best = None;
        let mut best_ts = u64::MAX;
        let mut saw_contention = false;

        for (idx, page_arc) in self.pages.iter().enumerate() {
            match page_arc.try_read() {
                Ok(page) => {
                    if page.last_access < best_ts {
                        best_ts = page.last_access;
                        best = Some(idx);
                    }
                }
                Err(TryLockError::WouldBlock) => {
                    saw_contention = true;
                }
                Err(TryLockError::Poisoned(_)) => {
                    saw_contention = true;
                }
            }
        }

        (best, saw_contention)
    }
}

#[async_trait::async_trait]
impl benshu_infra::HealthCheck for KvEngine {
    async fn check_health(&self) -> benshu_infra::HealthStatus {
        let load = self.load_snapshot.avg_cpu_usage;
        if load > 95.0 {
            benshu_infra::HealthStatus::Unhealthy(format!("High CPU Load: {:.1}%", load))
        } else {
            benshu_infra::HealthStatus::Healthy
        }
    }
    fn module_name(&self) -> &'static str {
        "benshu-inference::kv-engine"
    }
}

impl InferenceConfig {
    pub fn for_llama3_8b() -> Self {
        Self {
            num_heads: 32,
            num_kv_heads: 8,
            ..Default::default()
        }
    }
    pub fn for_qwen7b() -> Self {
        Self {
            num_heads: 24,
            num_kv_heads: 1,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_pages_are_reset_before_reuse() {
        let mut engine = KvEngine::new(InferenceConfig::default());
        let page_id = engine.try_allocate_page("req-a").expect("page alloc");
        {
            let mut page = engine.pages[page_id].write().expect("page lock");
            page.k_data.fill(7);
            page.v_data.fill(9);
            page.last_access = 1234;
            page.k_min = -1.0;
            page.k_max = 1.0;
            page.v_min = -2.0;
            page.v_max = 2.0;
        }

        engine.release_request("req-a");
        let reused = engine
            .try_allocate_page("req-b")
            .expect("reused page alloc");
        assert_eq!(page_id, reused);

        let page = engine.pages[reused].read().expect("page lock");
        assert!(page.k_data.iter().all(|&v| v == 0));
        assert!(page.v_data.iter().all(|&v| v == 0));
        assert_eq!(page.k_min, 0.0);
        assert_eq!(page.k_max, 0.0);
        assert_eq!(page.v_min, 0.0);
        assert_eq!(page.v_max, 0.0);
        assert!(page.last_access > 0);
    }

    #[test]
    fn get_request_pages_refreshes_access_time() {
        let mut engine = KvEngine::new(InferenceConfig::default());
        let page_id = engine.try_allocate_page("req-a").expect("page alloc");
        {
            let mut page = engine.pages[page_id].write().expect("page lock");
            page.last_access = 1;
        }

        let _ = engine.get_request_pages("req-a");
        let page = engine.pages[page_id].read().expect("page lock");
        assert!(page.last_access > 1);
    }

    #[test]
    fn try_allocate_page_returns_no_evictable_page_when_only_uncompressed_pages_exist() {
        let config = InferenceConfig {
            num_pages: 1,
            ..Default::default()
        };
        let mut engine = KvEngine::new(config);
        let page_id = engine.try_allocate_page("req-a").expect("page alloc");
        let _ = page_id;

        let err = engine
            .try_allocate_page("req-b")
            .expect_err("expected no-evictable-page");
        assert_eq!(err, EngineError::NoReusablePage);
    }

    #[test]
    fn try_allocate_page_returns_contention_when_candidate_page_is_locked() {
        let config = InferenceConfig {
            num_pages: 1,
            ..Default::default()
        };
        let mut engine = KvEngine::new(config);
        let page_id = engine.try_allocate_page("req-a").expect("page alloc");

        let page_arc = Arc::clone(&engine.pages[page_id]);
        let _guard = page_arc.write().expect("contention guard");
        let err = engine
            .try_allocate_page("req-b")
            .expect_err("expected contention");
        assert_eq!(err, EngineError::Contention);
    }
}
