//! Hybrid search engine combining BM25 and vector similarity search
//!
//! Integrates keyword-based (BM25) and semantic (vector) search using RRF fusion.

#[cfg(feature = "vector")]
use crate::embedder::Embedder;
use crate::error::{EngramError, Result};
#[cfg(feature = "vector")]
use crate::local_reranker::LocalCandleReranker;
use crate::metadata_contract::{
    tier_promotion_metadata, PROMOTION_MODE_POLICY_DRIVEN, PROMOTION_MODE_UTILITY_DRIVEN,
};
use crate::model_pool::ModelPool;
use crate::reranker::Reranker;
use crate::rrf::{FusedResult, RrfConfig, RrfFusion};
#[cfg(feature = "vector")]
use crate::runtime_bridge::block_on_sync;
use crate::store::{AntiPattern, Document, EngramStore, Experience};
#[cfg(feature = "vector")]
use crate::{
    projection::TreeProjector,
    vector_store::{VectorMetric, VectorStore},
};
#[cfg(feature = "vector")]
use benshu_inference::{
    describe_local_model_contract, detect_windows_native_runtime_status,
    diagnose_windows_native_small_model_error, QuantLevel,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{info, warn};

/// Configuration for hybrid search
#[derive(Debug, Clone)]
pub struct HybridSearchConfig {
    pub db_path: PathBuf,
    pub vector_dimension: usize,
    pub max_vectors: usize,
    pub rrf_k: f64,
    pub bm25_weight: f64,
    pub vector_weight: f64,
    pub dedup_threshold: f32,
    pub enable_semantic_dedup: bool,
    pub vector_metric: crate::vector_store::VectorMetric,
    pub use_vector: bool,
    pub use_hierarchy_projection: bool,
    pub use_reranker: bool,
    pub embed_model: String,
    pub rerank_model: String,
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("engram.db"),
            vector_dimension: 384,
            max_vectors: 100_000,
            rrf_k: 60.0,
            bm25_weight: 0.4,
            vector_weight: 0.6,
            dedup_threshold: 0.85,
            enable_semantic_dedup: true,
            vector_metric: crate::vector_store::VectorMetric::Cosine,
            use_vector: true,
            use_hierarchy_projection: true,
            use_reranker: true,
            embed_model: String::new(),
            rerank_model: String::new(),
        }
    }
}

/// Hybrid search result combining BM25 and vector search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    pub document: Document,
    pub rrf_score: f64,
    pub bm25_score: Option<f64>,
    pub vector_score: Option<f32>,
    pub causal_efficiency: f32,
    pub latency_ms: f32,
    pub rank: usize,
}

/// Hybrid search statistics for UI monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchStats {
    pub total_documents: u64,
    pub total_unverified: u64,
    pub total_vectors: usize,
    pub total_collections: usize,
    pub database_path: String,
    // Detailed vector stats
    pub fp32_count: usize,
    pub warm_count: usize,
    pub cold_count: usize,
    pub background_count: usize,
    pub last_latency_ms: f32,
    pub acceleration_target: String, // e.g., "AVX2+FMA"
    // New Metrics (Phase 16)
    pub rrf_fusion_count: u64,
    pub semantic_dedup_count: u64,
    pub reranker_used_count: u64,
    pub vector_search_latency_avg: f32,
    pub bm25_search_latency_avg: f32,
    pub vector_execution_profile: String,
    pub vector_last_execution_mode: String,
    pub vector_snapshot_load_count: u64,
    pub vector_rebuild_count: u64,
    pub vector_ann_search_count: u64,
    pub vector_exact_scan_count: u64,
    pub vector_exact_backfill_count: u64,
    pub vector_exact_scan_fallback_rate: f32,
    pub vector_quantized_decode_fallback_count: u64,
    pub vector_quantized_decode_fallback_rate: f32,
    pub vector_tombstone_count: usize,
    pub vector_tombstone_ratio: f32,
    pub vector_search_latency_by_metric_and_collection_json: String,
    pub session_archive_count: u64,
    pub session_recovery_count: u64,
    pub session_background_archive_count: u64,
    pub session_background_recovery_count: u64,
    pub prune_count_by_reason_json: String,
    pub retention_policy_json: String,
    pub retention_last_run_json: String,
    pub promotion_operation_count: u64,
    pub promotion_document_count: u64,
    pub promotion_last_source: String,
    pub promotion_last_target: String,
    pub promotion_last_mode: String,
    pub promotion_last_policy_owner: String,
    pub promotion_counts_by_source_target_json: String,
    pub windows_native_embed_outcome: String,
    pub windows_native_embed_class: String,
    pub windows_native_embed_provider: String,
    pub windows_native_embed_device_target: String,
    pub windows_native_embed_fallback_mode: String,
    pub windows_native_embed_strategy: String,
    pub windows_native_embed_note: String,
    pub windows_native_rerank_outcome: String,
    pub windows_native_rerank_class: String,
    pub windows_native_rerank_provider: String,
    pub windows_native_rerank_device_target: String,
    pub windows_native_rerank_fallback_mode: String,
    pub windows_native_rerank_strategy: String,
    pub windows_native_rerank_note: String,
}

#[cfg(feature = "vector")]
fn hierarchy_db_path(db_path: &Path) -> PathBuf {
    let mut hierarchy_path = db_path.to_path_buf();
    let stem = db_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("engram");
    hierarchy_path.set_file_name(format!("{stem}.hierarchy.redb"));
    hierarchy_path
}

use parking_lot::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PromotionMode {
    PolicyDriven,
    UtilityDriven,
}

impl PromotionMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::PolicyDriven => PROMOTION_MODE_POLICY_DRIVEN,
            Self::UtilityDriven => PROMOTION_MODE_UTILITY_DRIVEN,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PromotionTelemetry {
    operation_count: u64,
    document_count: u64,
    last_source: String,
    last_target: String,
    last_mode: String,
    last_policy_owner: String,
    counts_by_source_target: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WindowsNativeExecutionTelemetry {
    embed_outcome: String,
    embed_provider: String,
    embed_device_target: String,
    embed_fallback_mode: String,
    embed_strategy: String,
    embed_note: String,
    rerank_outcome: String,
    rerank_provider: String,
    rerank_device_target: String,
    rerank_fallback_mode: String,
    rerank_strategy: String,
    rerank_note: String,
}

fn windows_native_outcome_class(outcome: &str) -> &'static str {
    match outcome {
        "windows_native_active" | "active" => "active",
        "cpu_fallback_provider_downgrade" => "provider_downgrade",
        "cpu_fallback_no_accelerator_route" => "no_accelerator_route",
        "cpu_fallback_active" => "cpu_fallback",
        "windows_native_provider_execution_failed" => "provider_failure",
        "windows_native_execution_failed" => "runtime_failure",
        "fallback_runtime_active" | "migrate_to_windows_native_runtime" => "fallback_runtime",
        "backend_unlinked" | "runtime_missing" | "validation_only" => "pending_runtime",
        "model_contract_incompatible" => "contract_incompatible",
        "accelerator_resource_exhausted" => "resource_exhausted",
        "accelerator_unavailable" => "accelerator_unavailable",
        "not_observed" | "not_reported" => "not_observed",
        _ => "other",
    }
}

/// Hybrid search engine
pub struct HybridSearchEngine {
    store: Arc<EngramStore>,
    #[cfg(feature = "vector")]
    vector_store: Option<Arc<VectorStore>>,
    #[cfg(feature = "vector")]
    hierarchy_vector_store: Option<Arc<VectorStore>>,
    #[cfg(feature = "vector")]
    model_pool: Option<Arc<ModelPool>>,
    #[cfg(feature = "vector")]
    reranker_override: Option<Arc<dyn Reranker>>,
    config: Arc<RwLock<HybridSearchConfig>>,
    /// Concurrency control
    search_semaphore: Arc<Semaphore>,
    index_semaphore: Arc<Semaphore>,
    /// Metrics persistence
    rrf_count: AtomicU64,
    dedup_count: AtomicU64,
    reranker_count: AtomicU64,
    v_latency_sum: AtomicU64, // microseconds
    b_latency_sum: AtomicU64, // microseconds
    promotion_telemetry: Arc<RwLock<PromotionTelemetry>>,
    windows_native_telemetry: Arc<RwLock<WindowsNativeExecutionTelemetry>>,
}

impl HybridSearchEngine {
    /// Create a new hybrid search engine
    pub fn new(config: HybridSearchConfig, model_pool: Option<Arc<ModelPool>>) -> Result<Self> {
        let store = Arc::new(EngramStore::new(&config.db_path)?);

        #[cfg(feature = "vector")]
        let vector_store = {
            let vs_path = config.db_path.with_extension("vectors.bin");
            let kv = store.kv_arc();
            let has_persisted_vector_state =
                vs_path.exists() || kv.get_collection("meta:vector:store_config")?.is_some();
            let vs = if has_persisted_vector_state {
                Some(VectorStore::load(
                    kv.clone(),
                    &vs_path,
                    config.vector_dimension,
                    config.vector_metric,
                )?)
            } else {
                Some(VectorStore::new(
                    kv.clone(),
                    config.vector_dimension,
                    config.max_vectors,
                    config.vector_metric,
                ))
            };
            vs.map(Arc::new)
        };

        #[cfg(feature = "vector")]
        let hierarchy_vector_store = if config.use_vector && config.use_hierarchy_projection {
            let hierarchy_db_path = hierarchy_db_path(&config.db_path);
            let hierarchy_store = EngramStore::new(&hierarchy_db_path)?;
            let hierarchy_vs_path = hierarchy_db_path.with_extension("vectors.bin");
            let kv = hierarchy_store.kv_arc();
            let has_persisted_hierarchy_state = hierarchy_vs_path.exists()
                || kv.get_collection("meta:vector:store_config")?.is_some();
            let vs = if has_persisted_hierarchy_state {
                VectorStore::load(
                    kv.clone(),
                    &hierarchy_vs_path,
                    config.vector_dimension,
                    VectorMetric::Poincare,
                )?
            } else {
                VectorStore::new(
                    kv.clone(),
                    config.vector_dimension,
                    config.max_vectors,
                    VectorMetric::Poincare,
                )
            };
            Some(Arc::new(vs))
        } else {
            None
        };

        Ok(Self {
            store,
            #[cfg(feature = "vector")]
            vector_store,
            #[cfg(feature = "vector")]
            hierarchy_vector_store,
            #[cfg(feature = "vector")]
            model_pool,
            #[cfg(feature = "vector")]
            reranker_override: None,
            config: Arc::new(RwLock::new(config)),
            search_semaphore: Arc::new(Semaphore::new(32)),
            index_semaphore: Arc::new(Semaphore::new(8)),
            rrf_count: AtomicU64::new(0),
            dedup_count: AtomicU64::new(0),
            reranker_count: AtomicU64::new(0),
            v_latency_sum: AtomicU64::new(0),
            b_latency_sum: AtomicU64::new(0),
            promotion_telemetry: Arc::new(RwLock::new(PromotionTelemetry::default())),
            windows_native_telemetry: Arc::new(RwLock::new(WindowsNativeExecutionTelemetry {
                embed_outcome: "not_observed".to_string(),
                embed_provider: "not_reported".to_string(),
                embed_device_target: "not_reported".to_string(),
                embed_fallback_mode: "not_reported".to_string(),
                embed_strategy: "inspect_runtime_path".to_string(),
                embed_note: "No embedding runtime event has been observed yet.".to_string(),
                rerank_outcome: "not_observed".to_string(),
                rerank_provider: "not_reported".to_string(),
                rerank_device_target: "not_reported".to_string(),
                rerank_fallback_mode: "not_reported".to_string(),
                rerank_strategy: "inspect_runtime_path".to_string(),
                rerank_note: "No rerank runtime event has been observed yet.".to_string(),
            })),
        })
    }

    #[cfg(feature = "vector")]
    fn record_windows_native_embed_event(
        &self,
        outcome: impl Into<String>,
        strategy: impl Into<String>,
        note: impl Into<String>,
    ) {
        let runtime = detect_windows_native_runtime_status();
        let mut telemetry = self.windows_native_telemetry.write();
        telemetry.embed_outcome = outcome.into();
        telemetry.embed_provider = runtime.small_model_execution_provider;
        telemetry.embed_device_target = runtime.small_model_device_target;
        telemetry.embed_fallback_mode = runtime.small_model_fallback_mode;
        telemetry.embed_strategy = strategy.into();
        telemetry.embed_note = note.into();
    }

    #[cfg(feature = "vector")]
    fn record_windows_native_rerank_event(
        &self,
        outcome: impl Into<String>,
        strategy: impl Into<String>,
        note: impl Into<String>,
    ) {
        let runtime = detect_windows_native_runtime_status();
        let mut telemetry = self.windows_native_telemetry.write();
        telemetry.rerank_outcome = outcome.into();
        telemetry.rerank_provider = runtime.small_model_execution_provider;
        telemetry.rerank_device_target = runtime.small_model_device_target;
        telemetry.rerank_fallback_mode = runtime.small_model_fallback_mode;
        telemetry.rerank_strategy = strategy.into();
        telemetry.rerank_note = note.into();
    }

    pub fn reconfigure(&self, new_config: HybridSearchConfig) {
        let mut config = self.config.write();
        *config = new_config;
    }

    pub fn engram_store(&self) -> Arc<EngramStore> {
        Arc::clone(&self.store)
    }

    /// Project a hierarchical filesystem tree into the hidden Poincare auxiliary index.
    ///
    /// This is intentionally not exposed as a user-facing metric selector: semantic RAG keeps
    /// using cosine, while folder/code/document trees automatically get a hierarchy index.
    #[cfg(feature = "vector")]
    pub async fn project_hierarchy_path(&self, root: &Path) -> Result<usize> {
        if !root.is_dir() {
            return Ok(0);
        }

        let Some(vs) = &self.hierarchy_vector_store else {
            return Ok(0);
        };

        let dimension = self.config.read().vector_dimension;
        TreeProjector::new(dimension)
            .project_filesystem_tree(root, vs)
            .await
    }

    #[cfg(not(feature = "vector"))]
    pub async fn project_hierarchy_path(&self, _root: &Path) -> Result<usize> {
        Ok(0)
    }

    #[cfg(feature = "vector")]
    pub fn hierarchy_vector_count(&self) -> usize {
        self.hierarchy_vector_store
            .as_ref()
            .map(|vs| vs.len())
            .unwrap_or(0)
    }

    #[cfg(not(feature = "vector"))]
    pub fn hierarchy_vector_count(&self) -> usize {
        0
    }

    /// Access the underlying model pool for resource management
    pub fn model_pool(&self) -> Option<Arc<ModelPool>> {
        #[cfg(feature = "vector")]
        {
            self.model_pool.as_ref().map(Arc::clone)
        }
        #[cfg(not(feature = "vector"))]
        {
            None
        }
    }

    /// Set a custom reranker override
    pub fn with_reranker(mut self, reranker: Arc<dyn Reranker>) -> Self {
        #[cfg(feature = "vector")]
        {
            self.reranker_override = Some(reranker);
        }
        self
    }

    /// Index a document with differentiated quantization (Agent vs Background)
    pub fn index_at_level(
        &self,
        collection: &str,
        path: &str,
        title: &str,
        content: &str,
        level: QuantLevel,
        unverified: bool,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        let _permit = self
            .index_semaphore
            .try_acquire()
            .map_err(|_| EngramError::Internal("Index concurrency limit reached".into()))?;

        // 1. Vector indexing
        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            if let Ok(emb) = self.embed(content) {
                // 1.1 Semantic Deduplication
                let is_duplicate = self
                    .is_semantic_duplicate(collection, &emb, content)
                    .unwrap_or(false);
                if is_duplicate {
                    // Dedup only suppresses the ANN vector entry. The document body still
                    // belongs in the requested collection/path so imports and updates are
                    // visible to knowledge management tools.
                    tracing::info!(collection = %collection, path = %path, "Skipping vector indexing: semantic/hash duplicate detected");
                    self.dedup_count.fetch_add(1, Ordering::Relaxed);
                } else {
                    vs.add_at_level(collection, path, title, 0, emb, level)?;
                }
            }
        }

        // 2. Text indexing (FTS)
        self.store
            .store_document(collection, path, title, content, unverified, metadata)?;

        Ok(())
    }

    /// Generate embeddings for the given text using the default model
    #[cfg(feature = "vector")]
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let config = self.config.read();
        if !config.use_vector {
            return Err(EngramError::InvalidInput(
                "Vector search is disabled in config".into(),
            ));
        }

        if let Some(pool) = &self.model_pool {
            let model_dir = config
                .db_path
                .parent()
                .unwrap_or(&config.db_path)
                .join("models")
                .join(&config.embed_model);
            let contract = describe_local_model_contract(&model_dir);
            let emb_getter = pool
                .get_embedder(&config.embed_model, || {
                    let kv = self.store.kv_arc();
                    Embedder::load(&model_dir, Some(kv))
                })
                .map_err(|err| {
                    let diagnosis = diagnose_windows_native_small_model_error(
                        Some(&model_dir),
                        &benshu_inference::backend::InferenceError::LoadFailed(err.to_string()),
                    );
                    self.record_windows_native_embed_event(
                        diagnosis.outcome,
                        diagnosis.strategy,
                        diagnosis.note.clone(),
                    );
                    err
                })?;

            // Sync wrapper for the async embedder
            match block_on_sync(async move { emb_getter.embed(text).await }) {
                Ok(embedding) => {
                    if contract.ready_for_windows_native_small_model_runtime
                        && detect_windows_native_runtime_status().small_model_runtime_readiness
                            == "windows_native_ready"
                    {
                        self.record_windows_native_embed_event(
                            "windows_native_active",
                            "active",
                            "Embedding executed through the Windows-native small-model runtime."
                                .to_string(),
                        );
                    }
                    Ok(embedding)
                }
                Err(err) => {
                    let diagnosis = diagnose_windows_native_small_model_error(
                        Some(&model_dir),
                        &benshu_inference::backend::InferenceError::Execution(
                            err.to_string(),
                            "engram-embed".to_string(),
                        ),
                    );
                    self.record_windows_native_embed_event(
                        diagnosis.outcome,
                        diagnosis.strategy,
                        diagnosis.note.clone(),
                    );
                    Err(err)
                }
            }
        } else {
            Err(EngramError::InvalidInput(
                "Model pool not initialized for embedding".into(),
            ))
        }
    }

    /// Check if the content is a semantic duplicate in the given collection
    #[cfg(feature = "vector")]
    pub fn is_semantic_duplicate(
        &self,
        collection: &str,
        embedding: &[f32],
        content: &str,
    ) -> Result<bool> {
        let config = self.config.read();
        if !config.enable_semantic_dedup {
            return Ok(false);
        }

        // 1. O(1) Hash-based pre-check
        let hash = crate::content_hash::hash_content(content);
        if self.store.kv().get_content(&hash)?.is_some() {
            return Ok(true);
        }

        // 2. Vector search check
        if let Some(vs) = &self.vector_store {
            let results = vs.search(collection, embedding, 3)?;
            for res in results {
                let similarity = res.score;
                if similarity >= config.dedup_threshold {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Hybrid search combining BM25 and vector search
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<HybridSearchResult>> {
        let config = self.config.read();
        let _permit = self
            .search_semaphore
            .try_acquire()
            .map_err(|_| EngramError::Internal("Search concurrency limit reached".into()))?;

        // 1. BM25 search
        let start_b = std::time::Instant::now();
        let bm25_results = self.store.search_fts(query, limit * 2)?;
        self.b_latency_sum
            .fetch_add(start_b.elapsed().as_micros() as u64, Ordering::Relaxed);

        // 2. Vector search
        let mut vector_input: Vec<(String, f64)> = Vec::new();

        #[cfg(feature = "vector")]
        if config.use_vector {
            if let Ok(emb) = self.embed(query) {
                let start_v = std::time::Instant::now();
                if let Some(vs) = &self.vector_store {
                    if let Ok(v_res) = vs.search("", &emb, limit * 2) {
                        self.v_latency_sum
                            .fetch_add(start_v.elapsed().as_micros() as u64, Ordering::Relaxed);
                        vector_input = v_res
                            .iter()
                            .map(|r| (format!("{}:{}", r.collection, r.path), r.score as f64))
                            .collect();
                    }
                }
            }
        }

        // 3. Fusion Logic
        let fusion = RrfFusion::with_config(RrfConfig {
            k: config.rrf_k as usize,
            bm25_weight: config.bm25_weight,
            vector_weight: config.vector_weight,
        });

        let bm25_input: Vec<(String, f64)> = bm25_results
            .iter()
            .map(|r| {
                (
                    format!("{}:{}", r.document.collection, r.document.path),
                    r.score,
                )
            })
            .collect();

        let mut results: Vec<HybridSearchResult> = if !vector_input.is_empty() {
            let fused_results = fusion.fuse_hybrid(
                &bm25_input,
                &vector_input,
                config.bm25_weight,
                config.vector_weight,
            );

            self.rrf_count.fetch_add(1, Ordering::Relaxed);

            fused_results
                .into_iter()
                .filter_map(|f| {
                    if let Some(r) = bm25_results.iter().find(|r| {
                        format!("{}:{}", r.document.collection, r.document.path) == f.docid
                    }) {
                        Some(HybridSearchResult {
                            document: r.document.clone(),
                            rrf_score: f.rrf_score,
                            bm25_score: f.source_metadata.get("bm25").map(|(_, s)| *s),
                            vector_score: f.source_metadata.get("vector").map(|(_, s)| *s as f32),
                            causal_efficiency: 1.0,
                            latency_ms: 0.0,
                            rank: 0,
                        })
                    } else {
                        let parts: Vec<&str> = f.docid.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            self.store
                                .get_by_path(parts[0], parts[1])
                                .ok()
                                .flatten()
                                .map(|doc| HybridSearchResult {
                                    document: doc,
                                    rrf_score: f.rrf_score,
                                    bm25_score: f.source_metadata.get("bm25").map(|(_, s)| *s),
                                    vector_score: f
                                        .source_metadata
                                        .get("vector")
                                        .map(|(_, s)| *s as f32),
                                    causal_efficiency: 1.0,
                                    latency_ms: 0.0,
                                    rank: 0,
                                })
                        } else {
                            None
                        }
                    }
                })
                .collect()
        } else {
            bm25_results
                .into_iter()
                .map(|r| HybridSearchResult {
                    document: r.document,
                    rrf_score: r.score,
                    bm25_score: Some(r.score),
                    vector_score: None,
                    causal_efficiency: 1.0,
                    latency_ms: 0.0,
                    rank: 0,
                })
                .collect()
        };

        results.sort_by(|a, b| {
            b.rrf_score
                .partial_cmp(&a.rrf_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);

        // 4. Reranking
        #[cfg(feature = "vector")]
        {
            if config.use_reranker {
                if let Some(reranker) = &self.reranker_override {
                    results = reranker.rerank(query, results.clone()).unwrap_or(results);
                } else if let Some(pool) = &self.model_pool {
                    let model_dir = config
                        .db_path
                        .parent()
                        .unwrap_or(&config.db_path)
                        .join("models")
                        .join(&config.rerank_model);
                    match pool.get_reranker(&config.rerank_model, || {
                        LocalCandleReranker::load(&model_dir, Some(self.store.kv_arc()))
                    }) {
                        Ok(reranker) => {
                            self.reranker_count.fetch_add(1, Ordering::Relaxed);
                            match reranker.rerank(query, results.clone()) {
                                Ok(reranked) => {
                                    let contract = describe_local_model_contract(&model_dir);
                                    if contract.ready_for_windows_native_small_model_runtime
                                        && detect_windows_native_runtime_status()
                                            .small_model_runtime_readiness
                                            == "windows_native_ready"
                                    {
                                        self.record_windows_native_rerank_event(
                                            "windows_native_active",
                                            "active",
                                            "Rerank executed through the Windows-native small-model runtime."
                                                .to_string(),
                                        );
                                    }
                                    results = reranked;
                                }
                                Err(err) => {
                                    let diagnosis = diagnose_windows_native_small_model_error(
                                        Some(&model_dir),
                                        &benshu_inference::backend::InferenceError::Execution(
                                            err.to_string(),
                                            "engram-rerank".to_string(),
                                        ),
                                    );
                                    self.record_windows_native_rerank_event(
                                        diagnosis.outcome.clone(),
                                        diagnosis.strategy.clone(),
                                        diagnosis.note.clone(),
                                    );
                                    warn!(
                                        model = %config.rerank_model,
                                        path = %model_dir.display(),
                                        error = %err,
                                        windows_native_outcome = %diagnosis.outcome,
                                        windows_native_strategy = %diagnosis.strategy,
                                        "Hybrid search rerank execution failed; falling back to fused results"
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            let diagnosis = diagnose_windows_native_small_model_error(
                                Some(&model_dir),
                                &benshu_inference::backend::InferenceError::LoadFailed(
                                    err.to_string(),
                                ),
                            );
                            self.record_windows_native_rerank_event(
                                diagnosis.outcome.clone(),
                                diagnosis.strategy.clone(),
                                diagnosis.note.clone(),
                            );
                            warn!(
                                model = %config.rerank_model,
                                path = %model_dir.display(),
                                error = %err,
                                windows_native_outcome = %diagnosis.outcome,
                                windows_native_strategy = %diagnosis.strategy,
                                "Hybrid search reranker unavailable; falling back to fused results"
                            );
                        }
                    }
                }
            }
        }

        for (i, r) in results.iter_mut().enumerate() {
            r.rank = i + 1;
        }

        Ok(results)
    }

    /// Search within a specific collection
    pub fn search_in_collection(
        &self,
        query: &str,
        collection: &str,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        let config = self.config.read();
        let bm25_results = self
            .store
            .search_fts_in_collection(query, collection, limit)?;

        let results: Vec<HybridSearchResult> = bm25_results
            .into_iter()
            .enumerate()
            .map(|(i, r)| HybridSearchResult {
                document: r.document,
                rrf_score: config.bm25_weight / (config.rrf_k + i as f64 + 1.0),
                bm25_score: Some(r.score),
                vector_score: None,
                causal_efficiency: 1.0,
                latency_ms: 0.0,
                rank: i + 1,
            })
            .collect();

        Ok(results)
    }

    /// Search with path prefix filter
    pub fn search_with_path(
        &self,
        query: &str,
        collection: &str,
        path_prefix: &str,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        let config = self.config.read();
        let bm25_results =
            self.store
                .search_fts_with_path(query, collection, path_prefix, limit)?;

        let results: Vec<HybridSearchResult> = bm25_results
            .into_iter()
            .enumerate()
            .map(|(i, r)| HybridSearchResult {
                document: r.document,
                rrf_score: config.bm25_weight / (config.rrf_k + i as f64 + 1.0),
                bm25_score: Some(r.score),
                vector_score: None,
                causal_efficiency: 1.0,
                latency_ms: 0.0,
                rank: i + 1,
            })
            .collect();

        Ok(results)
    }

    pub fn stats(&self) -> HybridSearchStats {
        let config = self.config.read();
        let store_stats = self.store.stats().unwrap_or_default();
        let (
            session_archive_count,
            session_recovery_count,
            session_background_archive_count,
            session_background_recovery_count,
            prune_counts,
        ) = self
            .store
            .lifecycle_metrics()
            .unwrap_or_else(|_| (0, 0, 0, 0, BTreeMap::new()));
        let retention_policy_json = serde_json::to_string(&self.store.retention_policy())
            .unwrap_or_else(|_| "{}".to_string());
        let retention_last_run_json =
            serde_json::to_string(&self.store.last_retention_report().unwrap_or_default())
                .unwrap_or_else(|_| "{}".to_string());

        #[cfg(feature = "vector")]
        let (total_vectors, fp32, warm, cold, back, accel, latency, runtime) =
            if let Some(vs) = &self.vector_store {
                let counts = vs.get_level_counts();
                (
                    vs.len(),
                    counts.0,
                    counts.1,
                    counts.2,
                    counts.3,
                    vs.get_acceleration_info(),
                    vs.get_last_latency_ms(),
                    Some(vs.runtime_stats()),
                )
            } else {
                (0, 0, 0, 0, 0, "N/A".to_string(), 0.0, None)
            };
        #[cfg(not(feature = "vector"))]
        let (total_vectors, fp32, warm, cold, back, accel, latency, runtime) = (
            0,
            0,
            0,
            0,
            0,
            "N/A".to_string(),
            0.0,
            None::<crate::vector_store::VectorRuntimeStats>,
        );

        let rrf_c = self.rrf_count.load(Ordering::Relaxed);
        let b_lat = if rrf_c == 0 {
            0.0
        } else {
            self.b_latency_sum.load(Ordering::Relaxed) as f32 / 1000.0 / rrf_c as f32
        };
        let v_lat = if rrf_c == 0 {
            0.0
        } else {
            self.v_latency_sum.load(Ordering::Relaxed) as f32 / 1000.0 / rrf_c as f32
        };
        let runtime = runtime.unwrap_or_else(|| crate::vector_store::VectorRuntimeStats {
            execution_profile: "disabled".to_string(),
            last_execution_mode: "disabled".to_string(),
            snapshot_load_count: 0,
            rebuild_count: 0,
            ann_search_count: 0,
            exact_scan_count: 0,
            exact_backfill_count: 0,
            exact_scan_fallback_rate: 0.0,
            quantized_decode_fallback_count: 0,
            quantized_decode_fallback_rate: 0.0,
            tombstone_count: 0,
            tombstone_ratio: 0.0,
            search_latency_by_metric_and_collection_json: "{}".to_string(),
        });
        let promotion = self.promotion_telemetry.read().clone();
        let windows_native = self.windows_native_telemetry.read().clone();
        let embed_outcome = windows_native.embed_outcome;
        let embed_class = windows_native_outcome_class(&embed_outcome).to_string();
        let embed_provider = windows_native.embed_provider;
        let embed_device_target = windows_native.embed_device_target;
        let embed_fallback_mode = windows_native.embed_fallback_mode;
        let embed_strategy = windows_native.embed_strategy;
        let embed_note = windows_native.embed_note;
        let rerank_outcome = windows_native.rerank_outcome;
        let rerank_class = windows_native_outcome_class(&rerank_outcome).to_string();
        let rerank_provider = windows_native.rerank_provider;
        let rerank_device_target = windows_native.rerank_device_target;
        let rerank_fallback_mode = windows_native.rerank_fallback_mode;
        let rerank_strategy = windows_native.rerank_strategy;
        let rerank_note = windows_native.rerank_note;

        HybridSearchStats {
            total_documents: store_stats.total_documents,
            total_unverified: store_stats.total_unverified,
            total_vectors,
            total_collections: store_stats.total_collections,
            database_path: config.db_path.display().to_string(),
            fp32_count: fp32,
            warm_count: warm,
            cold_count: cold,
            background_count: back,
            last_latency_ms: latency,
            acceleration_target: accel,
            rrf_fusion_count: rrf_c,
            semantic_dedup_count: self.dedup_count.load(Ordering::Relaxed),
            reranker_used_count: self.reranker_count.load(Ordering::Relaxed),
            bm25_search_latency_avg: b_lat,
            vector_search_latency_avg: v_lat,
            vector_execution_profile: runtime.execution_profile,
            vector_last_execution_mode: runtime.last_execution_mode,
            vector_snapshot_load_count: runtime.snapshot_load_count,
            vector_rebuild_count: runtime.rebuild_count,
            vector_ann_search_count: runtime.ann_search_count,
            vector_exact_scan_count: runtime.exact_scan_count,
            vector_exact_backfill_count: runtime.exact_backfill_count,
            vector_exact_scan_fallback_rate: runtime.exact_scan_fallback_rate,
            vector_quantized_decode_fallback_count: runtime.quantized_decode_fallback_count,
            vector_quantized_decode_fallback_rate: runtime.quantized_decode_fallback_rate,
            vector_tombstone_count: runtime.tombstone_count,
            vector_tombstone_ratio: runtime.tombstone_ratio,
            vector_search_latency_by_metric_and_collection_json: runtime
                .search_latency_by_metric_and_collection_json,
            session_archive_count,
            session_recovery_count,
            session_background_archive_count,
            session_background_recovery_count,
            prune_count_by_reason_json: serde_json::to_string(&prune_counts)
                .unwrap_or_else(|_| "{}".to_string()),
            retention_policy_json,
            retention_last_run_json,
            promotion_operation_count: promotion.operation_count,
            promotion_document_count: promotion.document_count,
            promotion_last_source: promotion.last_source,
            promotion_last_target: promotion.last_target,
            promotion_last_mode: promotion.last_mode,
            promotion_last_policy_owner: promotion.last_policy_owner,
            promotion_counts_by_source_target_json: serde_json::to_string(
                &promotion.counts_by_source_target,
            )
            .unwrap_or_else(|_| "{}".to_string()),
            windows_native_embed_outcome: embed_outcome,
            windows_native_embed_class: embed_class,
            windows_native_embed_provider: embed_provider,
            windows_native_embed_device_target: embed_device_target,
            windows_native_embed_fallback_mode: embed_fallback_mode,
            windows_native_embed_strategy: embed_strategy,
            windows_native_embed_note: embed_note,
            windows_native_rerank_outcome: rerank_outcome,
            windows_native_rerank_class: rerank_class,
            windows_native_rerank_provider: rerank_provider,
            windows_native_rerank_device_target: rerank_device_target,
            windows_native_rerank_fallback_mode: rerank_fallback_mode,
            windows_native_rerank_strategy: rerank_strategy,
            windows_native_rerank_note: rerank_note,
        }
    }

    pub fn delete_stale_sessions(&self, days: u32) -> Result<usize> {
        self.store.delete_stale_sessions(days.into())
    }

    pub fn list_sessions(&self) -> Result<Vec<(String, String)>> {
        self.store.list_sessions()
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.store.delete_session(id)
    }

    pub fn list_unverified(&self, limit: usize) -> Result<Vec<Document>> {
        self.store.list_unverified(limit)
    }

    pub fn mark_verified(&self, collection: &str, path: &str) -> Result<()> {
        self.store.mark_verified(collection, path)
    }

    pub fn mark_pending_review(
        &self,
        collection: &str,
        path: &str,
        summary: Option<&str>,
    ) -> Result<()> {
        self.store.mark_pending_review(collection, path, summary)
    }

    pub fn delete_document(&self, collection: &str, path: &str) -> Result<()> {
        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            vs.remove(collection, path)?;
        }
        self.store.delete_document(collection, path)
    }

    pub fn replace_document_content(
        &self,
        collection: &str,
        path: &str,
        title: &str,
        content: &str,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<Document> {
        #[cfg(feature = "vector")]
        {
            if let Some(vs) = &self.vector_store {
                let _ = vs.remove(collection, path);
            }
            self.index_at_level(
                collection,
                path,
                title,
                content,
                QuantLevel::Warm,
                false,
                metadata,
            )?;
        }

        #[cfg(not(feature = "vector"))]
        {
            self.store
                .store_document(collection, path, title, content, false, metadata)?;
        }

        self.store
            .get_by_path(collection, path)?
            .ok_or_else(|| EngramError::NotFound(format!("{}:{}", collection, path)))
    }

    pub fn get_by_path(&self, collection: &str, path: &str) -> Result<Option<Document>> {
        self.store.get_by_path(collection, path)
    }

    pub fn list_documents_in_collection(&self, collection: &str) -> Result<Vec<Document>> {
        self.store.list_documents_in_collection(collection)
    }

    pub fn list_documents(&self) -> Result<Vec<Document>> {
        self.store.list_documents()
    }

    pub fn update_summary(&self, collection: &str, path: &str, summary: &str) -> Result<()> {
        self.store
            .update_summary(collection, path, summary.to_string())
    }

    pub fn merge_document_metadata(
        &self,
        collection: &str,
        path: &str,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        self.store.merge_metadata(collection, path, metadata)
    }

    pub fn promote_collection(
        &self,
        collection: &str,
        target_level: QuantLevel,
        source: &str,
        mode: PromotionMode,
        policy_owner: &str,
    ) -> Result<usize> {
        let docs = self.store.list_documents_in_collection(collection)?;
        let promoted_at = Utc::now().timestamp_millis().to_string();
        let mut promoted = 0usize;
        let target_level_str = format!("{:?}", target_level).to_lowercase();

        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            for doc in &docs {
                let _ = vs.change_level(&doc.collection, &doc.path, target_level);
            }
        }

        for doc in docs {
            let utility_boost = match target_level {
                QuantLevel::Full => 0.25,
                QuantLevel::Warm => 0.15,
                QuantLevel::Cold | QuantLevel::Background => 0.05,
            };
            let _ = self.store.update_utility(&doc.docid, utility_boost);

            let metadata = tier_promotion_metadata(
                &target_level_str,
                &promoted_at,
                source,
                mode.as_str(),
                policy_owner,
            );
            self.store
                .merge_metadata(&doc.collection, &doc.path, metadata)?;
            promoted += 1;
        }

        let mut telemetry = self.promotion_telemetry.write();
        telemetry.operation_count += 1;
        telemetry.document_count += promoted as u64;
        telemetry.last_source = source.to_string();
        telemetry.last_target = target_level_str.clone();
        telemetry.last_mode = mode.as_str().to_string();
        telemetry.last_policy_owner = policy_owner.to_string();
        let route_key = format!("{}->{}", source, target_level_str);
        *telemetry
            .counts_by_source_target
            .entry(route_key)
            .or_insert(0) += promoted as u64;

        Ok(promoted)
    }

    pub fn commit(&self) -> Result<()> {
        Ok(())
    }

    pub fn vacuum(&self) -> Result<()> {
        self.store.vacuum()
    }

    pub fn store_experience(&self, exp: Experience) -> Result<()> {
        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            if let Ok(embedding) = self.embed(&exp.task_query) {
                vs.add_at_level(
                    "experiences",
                    &exp.id,
                    "Experience",
                    0,
                    embedding,
                    QuantLevel::Cold,
                )?;
            }
        }
        self.store.store_experience(exp)
    }

    pub fn store_anti_pattern(&self, ap: AntiPattern) -> Result<()> {
        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            if let Ok(embedding) = self.embed(&ap.error_fingerprint) {
                vs.add_at_level(
                    "anti_patterns",
                    &ap.id,
                    "AntiPattern",
                    0,
                    embedding,
                    QuantLevel::Cold,
                )?;
            }
        }
        self.store.store_anti_pattern(ap)
    }

    pub fn index_experience(&self, exp: Experience, embedding: Vec<f32>) -> Result<()> {
        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            vs.add_at_level(
                "experiences",
                &exp.id,
                "Experience",
                0,
                embedding,
                QuantLevel::Cold,
            )?;
        }
        self.store.store_experience(exp)
    }

    pub fn index_anti_pattern(&self, ap: AntiPattern, embedding: Vec<f32>) -> Result<()> {
        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            vs.add_at_level(
                "anti_patterns",
                &ap.id,
                "AntiPattern",
                0,
                embedding,
                QuantLevel::Cold,
            )?;
        }
        self.store.store_anti_pattern(ap)
    }

    pub fn search_experiences(
        &self,
        _query: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<Experience>> {
        let mut results = Vec::new();
        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            let v_res = vs.search("experiences", embedding, limit)?;
            for r in v_res {
                if let Some(exp) = self.store.get_experience(&r.path)? {
                    results.push(exp);
                }
            }
        }
        Ok(results)
    }

    pub fn search_anti_patterns(
        &self,
        _query: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<AntiPattern>> {
        let mut results = Vec::new();
        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            let v_res = vs.search("anti_patterns", embedding, limit)?;
            for r in v_res {
                if let Some(ap) = self.store.get_anti_pattern(&r.path)? {
                    results.push(ap);
                }
            }
        }
        Ok(results)
    }

    pub fn perform_distillation(
        &self,
        age_threshold_days: u32,
        _utility_threshold: i32,
    ) -> Result<()> {
        info!("Performing memory distillation (Aging)...");
        let docs = self.store.fetch_all_docs_legacy()?;
        let now = Utc::now().timestamp_millis();
        let day_ms = 24 * 60 * 60 * 1000;
        let threshold_ms = age_threshold_days as i64 * day_ms;

        #[cfg(feature = "vector")]
        if let Some(vs) = &self.vector_store {
            for doc in docs {
                let age = now - doc.created_at_ms;
                if age > threshold_ms {
                    let target_level = if age > threshold_ms * 4 {
                        QuantLevel::Background
                    } else if age > threshold_ms * 2 {
                        QuantLevel::Cold
                    } else {
                        QuantLevel::Warm
                    };

                    let _ = vs.change_level(&doc.collection, &doc.path, target_level);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn folder_projection_updates_hidden_poincare_index() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("knowledge-tree");
        fs::create_dir_all(root.join("docs/chapter-1")).expect("create hierarchy");
        fs::write(root.join("README.md"), "root").expect("write root file");
        fs::write(root.join("docs/chapter-1/notes.md"), "notes").expect("write nested file");

        let config = HybridSearchConfig {
            db_path: temp.path().join("engram.redb"),
            use_vector: true,
            use_hierarchy_projection: true,
            vector_dimension: 8,
            ..Default::default()
        };
        let engine = HybridSearchEngine::new(config, None).expect("engine");

        let projected = engine
            .project_hierarchy_path(&root)
            .await
            .expect("project hierarchy");

        assert!(projected >= 4);
        assert_eq!(engine.hierarchy_vector_count(), projected);
    }

    #[tokio::test]
    async fn folder_projection_is_noop_when_disabled() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("knowledge-tree");
        fs::create_dir_all(&root).expect("create hierarchy");
        fs::write(root.join("README.md"), "root").expect("write root file");

        let config = HybridSearchConfig {
            db_path: temp.path().join("engram.redb"),
            use_vector: true,
            use_hierarchy_projection: false,
            vector_dimension: 8,
            ..Default::default()
        };
        let engine = HybridSearchEngine::new(config, None).expect("engine");

        let projected = engine
            .project_hierarchy_path(&root)
            .await
            .expect("project hierarchy");

        assert_eq!(projected, 0);
        assert_eq!(engine.hierarchy_vector_count(), 0);
    }
}
