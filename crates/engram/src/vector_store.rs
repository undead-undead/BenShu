//! SIMD-accelerated vector storage and similarity search
//!
//! Features:
//! - Multi-level quantization (FP32 -> U8 -> INT4 -> Ternary)
//! - SIMD optimized distance metrics via simsimd
//! - Persistent HNSW index with fast load/dump
//! - Aligned with engram 2026 i64-ms timestamp standard

use chrono::Utc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use simsimd::SpatialSimilarity;
#[cfg(target_os = "windows")]
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, trace, warn};

use crate::error::{EngramError, Result};
use crate::storage::Storage;
use benshu_inference::{QuantLevel, Quantizer, ScalarQuantizer, TernaryQuantizer};
use sha2::Sha256;

/// Optimized Vector Entry for CAS-based retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEntry {
    /// 12-char collision resistant docid
    pub docid: String,
    pub collection: String,
    pub path: String,
    pub chunk_seq: usize,
    /// Store FP32 only for 'Agent' or 'Full' levels
    pub embedding: Option<Vec<f32>>,
    pub quant_code: Option<Vec<u8>>,
    pub quant_level: QuantLevel,
    /// Unix timestamp in milliseconds for engine-wide consistency
    pub created_at_ms: i64,
    pub last_accessed_ms: Option<i64>,
    pub access_count: usize,
}

const VECTOR_CONFIG_KEY: &str = "meta:vector:store_config";
const VECTOR_ID_MAP_KEY: &str = "meta:vector:id_map";
const VECTOR_SNAPSHOT_MANIFEST_KEY: &str = "meta:vector:snapshot_manifest";
const VECTOR_QUANT_METADATA_KEY: &str = "meta:vector:quant_metadata";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VectorStoreMetadata {
    dimension: usize,
    metric: VectorMetric,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VectorSnapshotManifest {
    contract_version: u32,
    dimension: usize,
    metric: VectorMetric,
    vector_count: usize,
    id_map_len: usize,
    active_id_count: usize,
    id_map_checksum: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VectorExecutionProfile {
    AnnRescore,
    AnnRescoreWithExactBackfill,
    ExactScan,
}

impl VectorExecutionProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AnnRescore => "ann_rescore",
            Self::AnnRescoreWithExactBackfill => "ann_rescore_with_exact_backfill",
            Self::ExactScan => "exact_scan",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorRuntimeStats {
    pub execution_profile: String,
    pub last_execution_mode: String,
    pub snapshot_load_count: u64,
    pub rebuild_count: u64,
    pub ann_search_count: u64,
    pub exact_scan_count: u64,
    pub exact_backfill_count: u64,
    pub exact_scan_fallback_rate: f32,
    pub quantized_decode_fallback_count: u64,
    pub quantized_decode_fallback_rate: f32,
    pub tombstone_count: usize,
    pub tombstone_ratio: f32,
    pub search_latency_by_metric_and_collection_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VectorQuantMetadata {
    pub backend: String,
    pub rotation_kind: String,
    pub subquantizers: usize,
    pub codebook_bits: u8,
    pub approximate_bits_per_vector: usize,
    pub compression_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LatencyPercentiles {
    samples: usize,
    p50_ms: f32,
    p95_ms: f32,
    p99_ms: f32,
}

const SEARCH_LATENCY_SAMPLE_LIMIT: usize = 256;

impl VectorEntry {
    pub fn new(
        docid: String,
        collection: String,
        path: String,
        chunk_seq: usize,
        level: QuantLevel,
    ) -> Self {
        Self {
            docid,
            collection,
            path,
            chunk_seq,
            embedding: None,
            quant_code: None,
            quant_level: level,
            created_at_ms: Utc::now().timestamp_millis(),
            last_accessed_ms: None,
            access_count: 0,
        }
    }

    pub fn mark_accessed(&mut self) {
        self.last_accessed_ms = Some(Utc::now().timestamp_millis());
        self.access_count += 1;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchResult {
    pub docid: String,
    pub collection: String,
    pub path: String,
    pub score: f32,
    pub level: QuantLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VectorMetric {
    Cosine,
    L2,
    InnerProduct,
    Poincare,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DynamicQuantizer {
    Scalar(ScalarQuantizer),
    Ternary(TernaryQuantizer),
}

impl Quantizer for DynamicQuantizer {
    fn encode(&self, vector: &[f32]) -> Vec<u8> {
        match self {
            Self::Scalar(q) => q.encode(vector),
            Self::Ternary(q) => q.encode(vector),
        }
    }
    fn decode(&self, codes: &[u8]) -> Vec<f32> {
        match self {
            Self::Scalar(q) => q.decode(codes),
            Self::Ternary(q) => q.decode(codes),
        }
    }
    fn level(&self) -> QuantLevel {
        match self {
            Self::Scalar(q) => q.level(),
            Self::Ternary(q) => q.level(),
        }
    }
    fn dim(&self) -> usize {
        match self {
            Self::Scalar(q) => q.dim(),
            Self::Ternary(q) => q.dim(),
        }
    }
}

#[cfg(feature = "vector")]
use hnsw_rs::api::AnnT;
#[cfg(feature = "vector")]
use hnsw_rs::hnswio::*;
#[cfg(feature = "vector")]
use hnsw_rs::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// CPU features for runtime dispatch
#[derive(Debug, Clone, Default)]
pub struct CpuFeatures {
    pub avx2: bool,
    pub avxvnni: bool,
    pub avx512: bool,
    pub avx512vnni: bool,
    pub amx: bool,
}

#[cfg(feature = "vector")]
#[derive(Default, Clone, Copy)]
pub struct DistPoincare {}

#[cfg(feature = "vector")]
#[derive(Default, Clone, Copy)]
pub struct DistL2 {}

#[cfg(feature = "vector")]
#[derive(Default, Clone, Copy)]
pub struct DistInnerProduct {}

#[cfg(feature = "vector")]
impl Distance<f32> for DistPoincare {
    fn eval(&self, va: &[f32], vb: &[f32]) -> f32 {
        let mut dist_sq = 0.0;
        let mut norm_a_sq = 0.0;
        let mut norm_b_sq = 0.0;

        for (a, b) in va.iter().zip(vb.iter()) {
            let diff = a - b;
            dist_sq += diff * diff;
            norm_a_sq += a * a;
            norm_b_sq += b * b;
        }

        // Poincaré Ball Distance Math
        // d(u,v) = acosh(1 + 2 * ||u-v||^2 / ( (1 - ||u||^2) * (1 - ||v||^2) ) )
        let eps = 1e-4;

        // Try SIMD first if available (Phase 16 Optimization)
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx512f") {
                let (dist_sq, norm_a_sq, norm_b_sq) = unsafe { avx512_poincare_norms(va, vb) };
                let lambda_a = (1.0 - norm_a_sq).max(eps);
                let lambda_b = (1.0 - norm_b_sq).max(eps);
                let arg = 1.0 + 2.0 * dist_sq / (lambda_a * lambda_b);
                return arg.acosh();
            } else if is_x86_feature_detected!("avx2") {
                let (dist_sq, norm_a_sq, norm_b_sq) = unsafe { avx2_poincare_norms(va, vb) };
                let lambda_a = (1.0 - norm_a_sq).max(eps);
                let lambda_b = (1.0 - norm_b_sq).max(eps);
                let arg = 1.0 + 2.0 * dist_sq / (lambda_a * lambda_b);
                return arg.acosh();
            }
        }

        let lambda_a = (1.0 - norm_a_sq).max(eps);
        let lambda_b = (1.0 - norm_b_sq).max(eps);

        let arg = 1.0 + 2.0 * dist_sq / (lambda_a * lambda_b);
        arg.acosh()
    }
}

#[cfg(feature = "vector")]
impl Distance<f32> for DistL2 {
    fn eval(&self, va: &[f32], vb: &[f32]) -> f32 {
        va.iter()
            .zip(vb.iter())
            .map(|(a, b)| {
                let diff = a - b;
                diff * diff
            })
            .sum()
    }
}

#[cfg(feature = "vector")]
impl Distance<f32> for DistInnerProduct {
    fn eval(&self, va: &[f32], vb: &[f32]) -> f32 {
        -va.iter().zip(vb.iter()).map(|(a, b)| a * b).sum::<f32>()
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_poincare_norms(va: &[f32], vb: &[f32]) -> (f32, f32, f32) {
    use std::arch::x86_64::*;
    let mut i = 0;
    let mut v_dist_sq = _mm512_setzero_ps();
    let mut v_norm_a_sq = _mm512_setzero_ps();
    let mut v_norm_b_sq = _mm512_setzero_ps();

    while i + 16 <= va.len() {
        let a = _mm512_loadu_ps(va.as_ptr().add(i));
        let b = _mm512_loadu_ps(vb.as_ptr().add(i));

        let diff = _mm512_sub_ps(a, b);
        v_dist_sq = _mm512_fmadd_ps(diff, diff, v_dist_sq);
        v_norm_a_sq = _mm512_fmadd_ps(a, a, v_norm_a_sq);
        v_norm_b_sq = _mm512_fmadd_ps(b, b, v_norm_b_sq);
        i += 16;
    }

    let dist_sq = _mm512_reduce_add_ps(v_dist_sq);
    let norm_a_sq = _mm512_reduce_add_ps(v_norm_a_sq);
    let norm_b_sq = _mm512_reduce_add_ps(v_norm_b_sq);

    // Tail
    let mut t_dist = dist_sq;
    let mut t_a = norm_a_sq;
    let mut t_b = norm_b_sq;
    for j in i..va.len() {
        let diff = va[j] - vb[j];
        t_dist += diff * diff;
        t_a += va[j] * va[j];
        t_b += vb[j] * vb[j];
    }

    (t_dist, t_a, t_b)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn avx2_poincare_norms(va: &[f32], vb: &[f32]) -> (f32, f32, f32) {
    use std::arch::x86_64::*;
    let mut i = 0;
    let mut v_dist_sq = _mm256_setzero_ps();
    let mut v_norm_a_sq = _mm256_setzero_ps();
    let mut v_norm_b_sq = _mm256_setzero_ps();

    while i + 8 <= va.len() {
        let a = _mm256_loadu_ps(va.as_ptr().add(i));
        let b = _mm256_loadu_ps(vb.as_ptr().add(i));

        let diff = _mm256_sub_ps(a, b);
        v_dist_sq = _mm256_fmadd_ps(diff, diff, v_dist_sq);
        v_norm_a_sq = _mm256_fmadd_ps(a, a, v_norm_a_sq);
        v_norm_b_sq = _mm256_fmadd_ps(b, b, v_norm_b_sq);
        i += 8;
    }

    let mut res_dist = [0.0f32; 8];
    let mut res_a = [0.0f32; 8];
    let mut res_b = [0.0f32; 8];
    _mm256_storeu_ps(res_dist.as_mut_ptr(), v_dist_sq);
    _mm256_storeu_ps(res_a.as_mut_ptr(), v_norm_a_sq);
    _mm256_storeu_ps(res_b.as_mut_ptr(), v_norm_b_sq);

    let mut t_dist = res_dist.iter().sum::<f32>();
    let mut t_a = res_a.iter().sum::<f32>();
    let mut t_b = res_b.iter().sum::<f32>();

    for j in i..va.len() {
        let diff = va[j] - vb[j];
        t_dist += diff * diff;
        t_a += va[j] * va[j];
        t_b += vb[j] * vb[j];
    }

    (t_dist, t_a, t_b)
}

#[cfg(feature = "vector")]
pub enum AnyHnsw {
    Cosine(Hnsw<'static, f32, DistCosine>),
    L2(Hnsw<'static, f32, DistL2>),
    InnerProduct(Hnsw<'static, f32, DistInnerProduct>),
    Poincare(Hnsw<'static, f32, DistPoincare>),
}

#[cfg(feature = "vector")]
impl AnyHnsw {
    pub fn insert(&mut self, data: (&[f32], usize)) {
        match self {
            Self::Cosine(h) => h.insert(data),
            Self::L2(h) => h.insert(data),
            Self::InnerProduct(h) => h.insert(data),
            Self::Poincare(h) => h.insert(data),
        }
    }

    pub fn search(&self, query: &[f32], knbn: usize, ef: usize) -> Vec<Neighbour> {
        match self {
            Self::Cosine(h) => h.search(query, knbn, ef),
            Self::L2(h) => h.search(query, knbn, ef),
            Self::InnerProduct(h) => h.search(query, knbn, ef),
            Self::Poincare(h) => h.search(query, knbn, ef),
        }
    }

    pub fn file_dump(&self, path: &std::path::Path, filename: &str) -> std::io::Result<()> {
        match self {
            Self::Cosine(h) => h
                .file_dump(path, filename)
                .map(|_| ())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
            Self::L2(h) => h
                .file_dump(path, filename)
                .map(|_| ())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
            Self::InnerProduct(h) => h
                .file_dump(path, filename)
                .map(|_| ())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
            Self::Poincare(h) => h
                .file_dump(path, filename)
                .map(|_| ())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
        }
    }
}

#[cfg(feature = "vector")]
fn build_hnsw_indexes(metric: VectorMetric) -> HashMap<QuantLevel, AnyHnsw> {
    if metric == VectorMetric::InnerProduct {
        return HashMap::new();
    }

    let mut hnsw_indexes = HashMap::new();

    for level in [
        QuantLevel::Full,
        QuantLevel::Warm,
        QuantLevel::Cold,
        QuantLevel::Background,
    ] {
        let any_hnsw =
            match metric {
                VectorMetric::L2 => AnyHnsw::L2(Hnsw::<'static, f32, DistL2>::new(
                    16,
                    200,
                    1000000,
                    16,
                    DistL2 {},
                )),
                VectorMetric::InnerProduct => {
                    AnyHnsw::InnerProduct(Hnsw::<'static, f32, DistInnerProduct>::new(
                        16,
                        200,
                        1000000,
                        16,
                        DistInnerProduct {},
                    ))
                }
                VectorMetric::Poincare => AnyHnsw::Poincare(
                    Hnsw::<'static, f32, DistPoincare>::new(16, 200, 1000000, 16, DistPoincare {}),
                ),
                _ => AnyHnsw::Cosine(Hnsw::<'static, f32, DistCosine>::new(
                    16,
                    200,
                    1000000,
                    16,
                    DistCosine {},
                )),
            };

        #[cfg(target_os = "windows")]
        match &any_hnsw {
            AnyHnsw::Cosine(h) => h.set_alignment(64),
            AnyHnsw::L2(h) => h.set_alignment(64),
            AnyHnsw::InnerProduct(h) => h.set_alignment(64),
            AnyHnsw::Poincare(h) => h.set_alignment(64),
        }

        hnsw_indexes.insert(level, any_hnsw);
    }

    hnsw_indexes
}

pub struct VectorStore {
    kv: Arc<dyn Storage>,
    id_map: RwLock<Vec<String>>,
    quantizers: RwLock<Vec<(QuantLevel, DynamicQuantizer)>>,
    dimension: usize,
    metric: VectorMetric,
    last_latency_ms: RwLock<f32>,
    #[cfg(feature = "vector")]
    hnsw_indexes: RwLock<HashMap<QuantLevel, AnyHnsw>>,
    cpu_features: CpuFeatures,
    /// Store for quantizer parameters to prevent drift after restart
    quantizer_meta_key: String,
    /// Hot metadata cache so ANN recall does not depend on KV round-trips.
    vector_entries: RwLock<HashMap<String, VectorEntry>>,
    /// Phase 19.4: High-frequency decoded vector cache (LRU)
    decoded_cache: Arc<parking_lot::RwLock<lru::LruCache<String, Vec<f32>>>>,
    /// Phase 19.4: Deletion tombstones for HNSW (O(1) filtering)
    tombstones: Arc<parking_lot::RwLock<std::collections::HashSet<usize>>>,
    /// Explicit execution telemetry for retrieval governance.
    last_execution_mode: RwLock<String>,
    snapshot_load_count: AtomicU64,
    rebuild_count: AtomicU64,
    ann_search_count: AtomicU64,
    exact_scan_count: AtomicU64,
    exact_backfill_count: AtomicU64,
    quantized_decode_fallback_count: AtomicU64,
    quantized_decode_search_count: AtomicU64,
    search_latency_samples: RwLock<BTreeMap<String, VecDeque<f32>>>,
    quant_metadata: RwLock<Option<VectorQuantMetadata>>,
}

impl VectorStore {
    fn ann_contract_enabled(metric: VectorMetric) -> bool {
        metric != VectorMetric::InnerProduct
    }

    fn id_map_checksum(entries: &[String]) -> String {
        use sha2::Digest;

        let mut hasher = Sha256::new();
        for entry in entries {
            hasher.update(entry.as_bytes());
            hasher.update([0u8]);
        }
        format!("{:x}", hasher.finalize())
    }

    fn current_snapshot_manifest(&self) -> VectorSnapshotManifest {
        let id_map = self.id_map.read();
        let active_id_count = id_map
            .iter()
            .filter(|key| !key.starts_with("__deleted__:"))
            .count();
        let vector_count = self.vector_entries.read().len();

        VectorSnapshotManifest {
            contract_version: 1,
            dimension: self.dimension,
            metric: self.metric,
            vector_count,
            id_map_len: id_map.len(),
            active_id_count,
            id_map_checksum: Self::id_map_checksum(&id_map),
        }
    }

    fn save_snapshot_manifest(&self) -> Result<()> {
        let manifest = self.current_snapshot_manifest();
        let data =
            bincode::serialize(&manifest).map_err(|e| EngramError::Serialization(e.to_string()))?;
        self.kv
            .put_collection(VECTOR_SNAPSHOT_MANIFEST_KEY, &data)?;
        Ok(())
    }

    fn snapshot_manifest_matches_kv(&self) -> Result<bool> {
        let Some(raw) = self.kv.get_collection(VECTOR_SNAPSHOT_MANIFEST_KEY)? else {
            warn!("Vector snapshot manifest missing; snapshot load will fall back to rebuild");
            return Ok(false);
        };
        let persisted: VectorSnapshotManifest =
            bincode::deserialize(&raw).map_err(|e| EngramError::Serialization(e.to_string()))?;
        let current = self.current_snapshot_manifest();

        Ok(persisted.contract_version == current.contract_version
            && persisted.dimension == current.dimension
            && persisted.metric == current.metric
            && persisted.vector_count == current.vector_count
            && persisted.id_map_len == current.id_map_len
            && persisted.active_id_count == current.active_id_count
            && persisted.id_map_checksum == current.id_map_checksum)
    }

    fn restore_tombstones_from_id_map(&self) {
        let restored = self
            .id_map
            .read()
            .iter()
            .enumerate()
            .filter_map(|(idx, key)| key.starts_with("__deleted__:").then_some(idx))
            .collect::<HashSet<_>>();
        *self.tombstones.write() = restored;
    }

    pub fn new(
        kv: Arc<dyn Storage>,
        dimension: usize,
        _max_vectors: usize,
        metric: VectorMetric,
    ) -> Self {
        #[cfg(feature = "vector")]
        let hnsw_indexes = build_hnsw_indexes(metric);

        Self {
            kv,
            id_map: RwLock::new(Vec::new()),
            quantizers: RwLock::new(Vec::new()),
            dimension,
            metric,
            last_latency_ms: RwLock::new(0.0),
            #[cfg(feature = "vector")]
            hnsw_indexes: RwLock::new(hnsw_indexes),
            cpu_features: Self::detect_features(),
            quantizer_meta_key: format!("meta:quantizers:{}", dimension),
            vector_entries: RwLock::new(HashMap::new()),
            decoded_cache: Arc::new(parking_lot::RwLock::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(1000).unwrap(),
            ))),
            tombstones: Arc::new(parking_lot::RwLock::new(std::collections::HashSet::new())),
            last_execution_mode: RwLock::new(
                Self::default_execution_profile(metric).as_str().to_string(),
            ),
            snapshot_load_count: AtomicU64::new(0),
            rebuild_count: AtomicU64::new(0),
            ann_search_count: AtomicU64::new(0),
            exact_scan_count: AtomicU64::new(0),
            exact_backfill_count: AtomicU64::new(0),
            quantized_decode_fallback_count: AtomicU64::new(0),
            quantized_decode_search_count: AtomicU64::new(0),
            search_latency_samples: RwLock::new(BTreeMap::new()),
            quant_metadata: RwLock::new(None),
        }
    }

    fn detect_features() -> CpuFeatures {
        let mut f = CpuFeatures::default();
        #[cfg(target_arch = "x86_64")]
        {
            f.avx2 = is_x86_feature_detected!("avx2");
            f.avxvnni = is_x86_feature_detected!("avxvnni");
            f.avx512 = is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512bw");
            f.avx512vnni = is_x86_feature_detected!("avx512vnni");
            #[cfg(target_os = "windows")]
            {
                // AMX detection on Windows usually requires checking specifically for TILE/INT8
                f.amx =
                    is_x86_feature_detected!("amx-tile") && is_x86_feature_detected!("amx-int8");
            }
        }
        f
    }

    pub fn load(
        kv: Arc<dyn Storage>,
        path: &std::path::Path,
        expected_dimension: usize,
        expected_metric: VectorMetric,
    ) -> Result<Self> {
        info!("Opening VectorStore and restoring state...");
        let had_persisted_config = kv.get_collection(VECTOR_CONFIG_KEY)?.is_some();
        let metadata = if let Some(raw) = kv.get_collection(VECTOR_CONFIG_KEY)? {
            let persisted: VectorStoreMetadata = bincode::deserialize(&raw)?;
            if persisted.dimension != expected_dimension || persisted.metric != expected_metric {
                return Err(EngramError::InvalidInput(format!(
                    "VectorStore config mismatch: persisted dim={} metric={:?}, requested dim={} metric={:?}",
                    persisted.dimension, persisted.metric, expected_dimension, expected_metric
                )));
            }
            persisted
        } else {
            VectorStoreMetadata {
                dimension: expected_dimension,
                metric: expected_metric,
            }
        };
        let store = Self::new(kv.clone(), metadata.dimension, 1000000, metadata.metric);
        if !had_persisted_config {
            store.save_config_metadata()?;
        }

        let persisted_vectors = kv.iter_vectors()?;
        let cached_entries = persisted_vectors
            .into_iter()
            .filter_map(|(key, data)| {
                bincode::deserialize::<VectorEntry>(&data)
                    .ok()
                    .map(|entry| (key, entry))
            })
            .collect::<Vec<_>>();

        {
            let mut vector_entries = store.vector_entries.write();
            vector_entries.clear();
            for (key, entry) in &cached_entries {
                vector_entries.insert(key.clone(), entry.clone());
            }
        }

        // 1. Restore Quantizers from Metadata (Critical for data consistency)
        if let Ok(Some(meta_data)) = kv.get_collection(&store.quantizer_meta_key) {
            if let Ok(saved_qs) =
                bincode::deserialize::<Vec<(QuantLevel, DynamicQuantizer)>>(&meta_data)
            {
                *store.quantizers.write() = saved_qs;
                debug!(
                    "Restored {} quantizers from disk",
                    store.quantizers.read().len()
                );
            }
        }

        if let Ok(Some(raw)) = kv.get_collection(VECTOR_QUANT_METADATA_KEY) {
            if let Ok(metadata) = bincode::deserialize::<VectorQuantMetadata>(&raw) {
                *store.quant_metadata.write() = Some(metadata);
            }
        }

        if !Self::ann_contract_enabled(store.metric) {
            info!(
                "VectorStore metric {:?} is running under explicit exact-scan contract; ANN snapshots are disabled",
                store.metric
            );
            return Ok(store);
        }

        // 2. Try to load HNSW snapshots first
        let mut loaded_any = false;
        #[cfg(feature = "vector")]
        {
            let mut hnsw_indexes = store.hnsw_indexes.write();
            for level in [
                QuantLevel::Full,
                QuantLevel::Warm,
                QuantLevel::Cold,
                QuantLevel::Background,
            ] {
                let snap_basename = format!("hnsw_{:?}", level);
                if path.join(format!("{}.hnsw.graph", snap_basename)).exists() {
                    let hnswio = Box::leak(Box::new(HnswIo::new(path, &snap_basename)));
                    let loaded = match store.metric {
                        VectorMetric::L2 => hnswio.load_hnsw::<f32, DistL2>().map(|h| unsafe {
                            AnyHnsw::L2(std::mem::transmute::<
                                Hnsw<'_, f32, DistL2>,
                                Hnsw<'static, f32, DistL2>,
                            >(h))
                        }),
                        VectorMetric::InnerProduct => {
                            hnswio.load_hnsw::<f32, DistCosine>().map(|h| unsafe {
                                AnyHnsw::Cosine(std::mem::transmute::<
                                    Hnsw<'_, f32, DistCosine>,
                                    Hnsw<'static, f32, DistCosine>,
                                >(h))
                            })
                        }
                        VectorMetric::Poincare => {
                            hnswio.load_hnsw::<f32, DistPoincare>().map(|h| unsafe {
                                AnyHnsw::Poincare(std::mem::transmute::<
                                    Hnsw<'_, f32, DistPoincare>,
                                    Hnsw<'static, f32, DistPoincare>,
                                >(h))
                            })
                        }
                        _ => hnswio.load_hnsw::<f32, DistCosine>().map(|h| unsafe {
                            AnyHnsw::Cosine(std::mem::transmute::<
                                Hnsw<'_, f32, DistCosine>,
                                Hnsw<'static, f32, DistCosine>,
                            >(h))
                        }),
                    };

                    match loaded {
                        Ok(h_static) => {
                            hnsw_indexes.insert(level, h_static);
                            loaded_any = true;
                        }
                        Err(e) => {
                            warn!("Failed to load HNSW snapshot {}: {}", snap_basename, e);
                        }
                    }
                }
            }
        }

        if loaded_any {
            if let Ok(Some(id_data)) = kv.get_collection(VECTOR_ID_MAP_KEY) {
                if let Ok(id_list) = bincode::deserialize::<Vec<String>>(&id_data) {
                    *store.id_map.write() = id_list;
                }
            }
            store.restore_tombstones_from_id_map();
        }

        let snapshot_valid = if loaded_any {
            store.snapshot_manifest_matches_kv()?
        } else {
            false
        };

        if loaded_any && snapshot_valid {
            store.snapshot_load_count.store(1, Ordering::Relaxed);
            store.rebuild_count.store(0, Ordering::Relaxed);
        } else {
            if loaded_any {
                warn!("Vector snapshot invariants failed; rebuilding HNSW indexes from KV storage");
            } else {
                info!("HNSW snapshots missing or stale. Rebuilding from KV storage...");
            }

            {
                let mut id_map = store.id_map.write();
                id_map.clear();
            }
            store.tombstones.write().clear();
            #[cfg(feature = "vector")]
            {
                let mut hnsw_indexes = store.hnsw_indexes.write();
                *hnsw_indexes = build_hnsw_indexes(store.metric);
            }

            store.rebuild_count.store(1, Ordering::Relaxed);
            store.snapshot_load_count.store(0, Ordering::Relaxed);
            for (key, entry) in &cached_entries {
                let mut id_map = store.id_map.write();
                if !id_map.contains(key) {
                    let id = id_map.len();
                    id_map.push(key.clone());
                    #[cfg(feature = "vector")]
                    {
                        let mut hnsw_indexes = store.hnsw_indexes.write();
                        if let Some(hnsw) = hnsw_indexes.get_mut(&entry.quant_level) {
                            if let Some(emb) = store.recover_embedding(entry)? {
                                hnsw.insert((&emb, id));
                                let _ = kv.put_idx(&id.to_string(), key);
                            }
                        }
                    }
                }
            }
        }

        info!("VectorStore ready: {} entries", store.len());
        Ok(store)
    }

    /// Persist current quantizer parameters to prevent drift
    fn save_config_metadata(&self) -> Result<()> {
        let metadata = VectorStoreMetadata {
            dimension: self.dimension,
            metric: self.metric,
        };
        let metadata_data = bincode::serialize(&metadata)?;
        self.kv.put_collection(VECTOR_CONFIG_KEY, &metadata_data)?;
        Ok(())
    }

    pub fn save_metadata(&self) -> Result<()> {
        self.save_config_metadata()?;
        let quantizers = self.quantizers.read();
        let data = bincode::serialize(&*quantizers)?;
        self.kv.put_collection(&self.quantizer_meta_key, &data)?;
        drop(quantizers);
        if let Some(metadata) = self.quant_metadata.read().clone() {
            let metadata_data = bincode::serialize(&metadata)?;
            self.kv
                .put_collection(VECTOR_QUANT_METADATA_KEY, &metadata_data)?;
        }
        Ok(())
    }

    pub fn set_quant_metadata(&self, metadata: VectorQuantMetadata) -> Result<()> {
        *self.quant_metadata.write() = Some(metadata);
        self.save_metadata()
    }

    pub fn quant_metadata(&self) -> Option<VectorQuantMetadata> {
        self.quant_metadata.read().clone()
    }

    pub fn sample_embeddings(
        &self,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Vec<f32>>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut entries = self
            .vector_entries
            .read()
            .iter()
            .map(|(key, entry)| (key.clone(), entry.clone()))
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut samples = Vec::new();
        for (_, entry) in entries {
            if collection
                .map(|wanted| entry.collection != wanted)
                .unwrap_or(false)
            {
                continue;
            }
            if let Some(vector) = self.recover_embedding(&entry)? {
                samples.push(vector);
            }
            if samples.len() >= limit {
                break;
            }
        }
        Ok(samples)
    }

    fn recover_embedding(&self, entry: &VectorEntry) -> Result<Option<Vec<f32>>> {
        if let Some(embedding) = &entry.embedding {
            return Ok(Some(embedding.clone()));
        }

        let Some(code) = &entry.quant_code else {
            return Ok(None);
        };

        let quantizers = self.quantizers.read();
        if let Some((_, quantizer)) = quantizers
            .iter()
            .find(|(level, _)| *level == entry.quant_level)
        {
            return Ok(Some(quantizer.decode(code)));
        }

        match entry.quant_level {
            QuantLevel::Background => Ok(Some(TernaryQuantizer::new(self.dimension).decode(code))),
            _ => Err(EngramError::RetrievalError(format!(
                "Missing quantizer metadata for {:?} vector reconstruction",
                entry.quant_level
            ))),
        }
    }

    fn deleted_key_marker(id: usize, key: &str) -> String {
        format!("__deleted__:{id}:{key}")
    }

    fn retire_key(&self, key: &str) -> Result<bool> {
        self.decoded_cache.write().pop(key);

        let positions = {
            let id_map = self.id_map.read();
            id_map
                .iter()
                .enumerate()
                .filter_map(|(idx, existing)| (existing == key).then_some(idx))
                .collect::<Vec<_>>()
        };

        if positions.is_empty() {
            return Ok(false);
        }

        {
            let mut tombstones = self.tombstones.write();
            for pos in &positions {
                tombstones.insert(*pos);
            }
        }

        let mut id_map = self.id_map.write();
        for pos in positions {
            let marker = Self::deleted_key_marker(pos, key);
            id_map[pos] = marker.clone();
            self.kv.put_idx(&pos.to_string(), &marker)?;
        }

        Ok(true)
    }

    #[cfg(feature = "vector")]
    fn insert_hnsw_record(&self, key: &str, embedding: &[f32], level: QuantLevel) -> Result<()> {
        let mut idx_map = self.id_map.write();
        let mut hnsw_indexes = self.hnsw_indexes.write();
        let id = idx_map.len();

        if let Some(hnsw) = hnsw_indexes.get_mut(&level) {
            hnsw.insert((embedding, id));
            self.kv.put_idx(&id.to_string(), key)?;
        }
        idx_map.push(key.to_string());
        Ok(())
    }

    pub fn len(&self) -> usize {
        if Self::ann_contract_enabled(self.metric) {
            self.id_map.read().len()
        } else {
            self.vector_entries.read().len()
        }
    }

    /// Quantize query vector for a specific level to enable zero-decode search
    fn quantize_query(&self, query: &[f32], level: QuantLevel) -> Result<Vec<u8>> {
        let quantizers = self.quantizers.read();
        if let Some((_, q)) = quantizers.iter().find(|(l, _)| *l == level) {
            Ok(q.encode(query))
        } else {
            // Hot-path training fallback (Phase 17 improvement: Collect samples)
            drop(quantizers); // Release read lock

            let mut samples = Vec::new();
            // Sample existing vectors to get a real distribution
            for key in self.id_map.read().iter().take(100) {
                if let Ok(Some(data)) = self.kv.get_vector(key) {
                    if let Ok(entry) = bincode::deserialize::<VectorEntry>(&data) {
                        if (entry.quant_level == QuantLevel::Full || entry.quant_level == level)
                            && entry.embedding.is_some()
                        {
                            samples.push(entry.embedding.unwrap());
                        }
                    }
                }
            }

            let mut quantizers = self.quantizers.write();
            let q_idx = quantizers.iter().position(|(l, _)| *l == level);

            let quantizer = if let Some(idx) = q_idx {
                &quantizers[idx].1
            } else {
                let vec_refs: Vec<&[f32]> = samples.iter().map(|v| v.as_slice()).collect();
                let new_q = match level {
                    QuantLevel::Background => {
                        DynamicQuantizer::Ternary(TernaryQuantizer::new(self.dimension))
                    }
                    _ => {
                        if vec_refs.is_empty() {
                            DynamicQuantizer::Scalar(ScalarQuantizer::train(&[query], level))
                        } else {
                            DynamicQuantizer::Scalar(ScalarQuantizer::train(&vec_refs, level))
                        }
                    }
                };
                quantizers.push((level, new_q));
                &quantizers.last().unwrap().1
            };
            Ok(quantizer.encode(query))
        }
    }

    pub fn get_acceleration_info(&self) -> String {
        let mut info = Vec::new();
        if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
            info.push("SIMD Enabled");
            if self.cpu_features.avx2 {
                info.push("AVX2");
            }
            if self.cpu_features.avxvnni {
                info.push("AVX-VNNI");
            }
            if self.cpu_features.avx512 {
                info.push("AVX-512");
            }
            if self.cpu_features.avx512vnni {
                info.push("AVX-512 VNNI");
            }
            if self.cpu_features.amx {
                info.push("AMX");
            }
        } else {
            info.push("Software Fallback");
        }
        info.join(", ")
    }

    pub fn get_last_latency_ms(&self) -> f32 {
        *self.last_latency_ms.read()
    }

    pub fn runtime_stats(&self) -> VectorRuntimeStats {
        let ann_search_count = self.ann_search_count.load(Ordering::Relaxed);
        let exact_scan_count = self.exact_scan_count.load(Ordering::Relaxed);
        let exact_backfill_count = self.exact_backfill_count.load(Ordering::Relaxed);
        let quantized_decode_fallback_count =
            self.quantized_decode_fallback_count.load(Ordering::Relaxed);
        let quantized_decode_search_count =
            self.quantized_decode_search_count.load(Ordering::Relaxed);
        let tombstone_count = self.tombstones.read().len();
        let id_count = self.id_map.read().len();
        VectorRuntimeStats {
            execution_profile: Self::default_execution_profile(self.metric)
                .as_str()
                .to_string(),
            last_execution_mode: self.last_execution_mode.read().clone(),
            snapshot_load_count: self.snapshot_load_count.load(Ordering::Relaxed),
            rebuild_count: self.rebuild_count.load(Ordering::Relaxed),
            ann_search_count,
            exact_scan_count,
            exact_backfill_count,
            exact_scan_fallback_rate: if ann_search_count == 0 {
                0.0
            } else {
                exact_backfill_count as f32 / ann_search_count as f32
            },
            quantized_decode_fallback_count,
            quantized_decode_fallback_rate: {
                let total_searches = ann_search_count + exact_scan_count;
                if total_searches == 0 {
                    0.0
                } else {
                    quantized_decode_search_count as f32 / total_searches as f32
                }
            },
            tombstone_count,
            tombstone_ratio: if id_count == 0 {
                0.0
            } else {
                tombstone_count as f32 / id_count as f32
            },
            search_latency_by_metric_and_collection_json: self.search_latency_summary_json(),
        }
    }

    pub fn get_level_counts(&self) -> (usize, usize, usize, usize) {
        let mut full = 0;
        let mut warm = 0;
        let mut cold = 0;
        let mut bg = 0;

        for entry in self.vector_entries.read().values() {
            match entry.quant_level {
                QuantLevel::Full => full += 1,
                QuantLevel::Warm => warm += 1,
                QuantLevel::Cold => cold += 1,
                QuantLevel::Background => bg += 1,
            }
        }
        (full, warm, cold, bg)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        info!("Saving VectorStore state and HNSW snapshots...");
        // 1. Save Quantizer metadata
        self.save_metadata()?;

        if !Self::ann_contract_enabled(self.metric) {
            return Ok(());
        }

        // 2. Dump HNSW indexes (Production-grade persistence)
        #[cfg(feature = "vector")]
        {
            let hnsw_indexes = self.hnsw_indexes.read();
            for (level, hnsw) in hnsw_indexes.iter() {
                let snap_basename = format!("hnsw_{:?}", level);
                let _ = hnsw.file_dump(path, &snap_basename);
            }
        }

        // 3. Save ID map for consistency
        let id_map = self.id_map.read();
        let id_data = bincode::serialize(&*id_map)?;
        self.kv.put_collection(VECTOR_ID_MAP_KEY, &id_data)?;
        drop(id_map);
        self.save_snapshot_manifest()?;

        Ok(())
    }

    pub fn change_level(
        &self,
        collection: &str,
        path: &str,
        target_level: QuantLevel,
    ) -> Result<()> {
        let key = format!("v:{}:{}", collection, path);
        if let Some(data) = self.kv.get_vector(&key)? {
            let mut entry = bincode::deserialize::<VectorEntry>(&data)
                .map_err(|e| EngramError::Serialization(e.to_string()))?;

            if entry.quant_level == target_level {
                return Ok(());
            }

            // 1. Recover embedding (decoding if necessary)
            let emb = self
                .recover_embedding(&entry)?
                .ok_or_else(|| EngramError::RetrievalError("No data found to migrate".into()))?;

            // 2. Re-quantize to target
            if target_level == QuantLevel::Full {
                entry.embedding = Some(emb.clone());
                entry.quant_code = None;
            } else {
                let mut quantizers = self.quantizers.write();
                let quantizer = if let Some(idx) =
                    quantizers.iter().position(|(l, _)| *l == target_level)
                {
                    &quantizers[idx].1
                } else {
                    let new_q = match target_level {
                        QuantLevel::Background => {
                            DynamicQuantizer::Ternary(TernaryQuantizer::new(self.dimension))
                        }
                        _ => {
                            DynamicQuantizer::Scalar(ScalarQuantizer::train(&[&emb], target_level))
                        }
                    };
                    quantizers.push((target_level, new_q));
                    &quantizers.last().unwrap().1
                };
                entry.quant_code = Some(quantizer.encode(&emb));
                entry.embedding = None;
            }
            entry.quant_level = target_level;

            // 3. Update HNSW (Insert into new level)
            #[cfg(feature = "vector")]
            {
                if Self::ann_contract_enabled(self.metric) {
                    self.retire_key(&key)?;
                    self.insert_hnsw_record(&key, &emb, target_level)?;
                }
            }

            // 4. Persist
            let new_data = bincode::serialize(&entry)?;
            self.kv.put_vector(&key, &new_data)?;
            self.vector_entries.write().insert(key.clone(), entry);
            self.save_metadata()?;
            trace!("Migrated {} to {:?}", path, target_level);
        }
        Ok(())
    }
    pub fn add(
        &self,
        collection: &str,
        path: &str,
        docid: &str,
        chunk_seq: usize,
        embedding: Vec<f32>,
    ) -> Result<()> {
        self.add_at_level(
            collection,
            path,
            docid,
            chunk_seq,
            embedding,
            QuantLevel::Full,
        )
    }

    pub fn add_at_level(
        &self,
        collection: &str,
        path: &str,
        docid: &str,
        chunk_seq: usize,
        embedding: Vec<f32>,
        level: QuantLevel,
    ) -> Result<()> {
        let mut entry = VectorEntry::new(
            docid.to_string(),
            collection.to_string(),
            path.to_string(),
            chunk_seq,
            level,
        );

        let key = format!("v:{}:{}", collection, path);

        if level == QuantLevel::Full {
            entry.embedding = Some(embedding.clone());
        } else {
            let mut quantizers = self.quantizers.write();
            let q_idx = quantizers.iter().position(|(l, _)| *l == level);

            let quantizer = if let Some(idx) = q_idx {
                &quantizers[idx].1
            } else {
                let new_q = match level {
                    QuantLevel::Background => {
                        DynamicQuantizer::Ternary(TernaryQuantizer::new(self.dimension))
                    }
                    _ => {
                        warn!("Training ScalarQuantizer on a single vector.");
                        DynamicQuantizer::Scalar(ScalarQuantizer::train(&[&embedding], level))
                    }
                };
                quantizers.push((level, new_q));
                &quantizers.last().unwrap().1
            };

            entry.quant_code = Some(quantizer.encode(&embedding));
            entry.embedding = None;
        }

        // --- HNSW Insertion ---
        #[cfg(feature = "vector")]
        {
            if Self::ann_contract_enabled(self.metric) {
                self.retire_key(&key)?;
                self.insert_hnsw_record(&key, &embedding, level)?;
            }
        }

        let data =
            bincode::serialize(&entry).map_err(|e| EngramError::Serialization(e.to_string()))?;
        self.kv.put_vector(&key, &data)?;
        self.vector_entries.write().insert(key.clone(), entry);

        // Phase 19.4: Cache Invalidation or Population
        if level == QuantLevel::Full {
            self.decoded_cache.write().put(key, embedding);
        } else {
            self.decoded_cache.write().pop(&key);
        }

        self.save_metadata()?;

        Ok(())
    }

    pub fn search(
        &self,
        collection: &str,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<VectorSearchResult>> {
        let start = Instant::now();
        let mut results = Vec::new();
        let mut had_quantized_decode = false;

        // Global Hardware Affinity: Bind to high-performance cores
        let _ = self.pin_to_performance_cores();

        if self.metric == VectorMetric::InnerProduct {
            self.exact_scan_count.fetch_add(1, Ordering::Relaxed);
            *self.last_execution_mode.write() =
                VectorExecutionProfile::ExactScan.as_str().to_string();
            for entry in self.vector_entries.read().values() {
                if entry.collection != collection {
                    continue;
                }

                let key = format!("v:{}:{}", entry.collection, entry.path);
                let recovered =
                    self.recover_search_vector(&key, entry, &mut had_quantized_decode)?;

                let score = recovered
                    .as_ref()
                    .map(|decoded| self.score_vector(query_vector, decoded))
                    .unwrap_or(0.0);

                results.push(VectorSearchResult {
                    docid: entry.docid.clone(),
                    collection: entry.collection.clone(),
                    path: entry.path.clone(),
                    score,
                    level: entry.quant_level,
                });
            }

            results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            results.truncate(limit);

            let ms = start.elapsed().as_secs_f32() * 1000.0;
            *self.last_latency_ms.write() = ms;
            self.record_search_latency(collection, ms);
            if had_quantized_decode {
                self.quantized_decode_search_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            debug!(
                "Exact inner-product search for {} took {:.2}ms",
                collection, ms
            );
            return Ok(results);
        }

        // --- HNSW Search (O(logN)) ---
        #[cfg(feature = "vector")]
        {
            self.ann_search_count.fetch_add(1, Ordering::Relaxed);
            *self.last_execution_mode.write() =
                VectorExecutionProfile::AnnRescore.as_str().to_string();
            let hnsw_indexes = self.hnsw_indexes.read();
            let id_map = self.id_map.read();
            let vector_entries = self.vector_entries.read();
            // We search across all levels for hybrid recall
            for level in [
                QuantLevel::Full,
                QuantLevel::Warm,
                QuantLevel::Cold,
                QuantLevel::Background,
            ] {
                if let Some(hnsw) = hnsw_indexes.get(&level) {
                    // ef_search = 128 for high recall at low overhead
                    let neighbors = hnsw.search(query_vector, limit, 128);
                    for neighbor in neighbors {
                        let id = neighbor.d_id;

                        // Phase 19.4: Tombstone filtering (O(1))
                        if self.tombstones.read().contains(&id) {
                            continue;
                        }

                        let Some(key) = id_map.get(id).cloned() else {
                            continue;
                        };
                        if !key.starts_with(&format!("v:{}:", collection)) {
                            continue;
                        }

                        let Some(entry) = vector_entries.get(&key) else {
                            continue;
                        };
                        // Guard: Only honor HNSW result if the document's stored level matches the current index level
                        // (Filters stale entries after level migration)
                        if entry.quant_level != level {
                            continue;
                        }

                        let mut score = self.score_neighbor_distance(neighbor.distance as f32);
                        let recovered =
                            self.recover_search_vector(&key, entry, &mut had_quantized_decode)?;

                        if let Some(decoded) = &recovered {
                            score = self.score_vector(query_vector, decoded);
                        } else if let Some(code) = &entry.quant_code {
                            if let Ok(query_bits) = self.quantize_query(query_vector, level) {
                                score = self.bitwise_similarity(&query_bits, code, level);
                            }
                        }

                        results.push(VectorSearchResult {
                            docid: entry.docid.clone(),
                            collection: entry.collection.clone(),
                            path: entry.path.clone(),
                            score,
                            level: entry.quant_level,
                        });
                    }
                }
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if results.len() < limit {
            self.exact_backfill_count.fetch_add(1, Ordering::Relaxed);
            *self.last_execution_mode.write() = VectorExecutionProfile::AnnRescoreWithExactBackfill
                .as_str()
                .to_string();
            let existing_paths = results
                .iter()
                .map(|result| result.path.clone())
                .collect::<HashSet<_>>();
            let mut supplemental = self.exact_scan_results(
                collection,
                query_vector,
                Some(&existing_paths),
                &mut had_quantized_decode,
            )?;
            results.append(&mut supplemental);
            results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        results.truncate(limit);

        let ms = start.elapsed().as_secs_f32() * 1000.0;
        *self.last_latency_ms.write() = ms;
        self.record_search_latency(collection, ms);
        if had_quantized_decode {
            self.quantized_decode_search_count
                .fetch_add(1, Ordering::Relaxed);
        }
        debug!("HNSW search for {} took {:.2}ms", collection, ms);
        Ok(results)
    }

    fn exact_scan_results(
        &self,
        collection: &str,
        query_vector: &[f32],
        skip_paths: Option<&HashSet<String>>,
        had_quantized_decode: &mut bool,
    ) -> Result<Vec<VectorSearchResult>> {
        let mut results = Vec::new();

        for entry in self.vector_entries.read().values() {
            if entry.collection != collection {
                continue;
            }
            if skip_paths
                .map(|paths| paths.contains(&entry.path))
                .unwrap_or(false)
            {
                continue;
            }

            let key = format!("v:{}:{}", entry.collection, entry.path);
            let recovered = self.recover_search_vector(&key, entry, had_quantized_decode)?;

            let score = recovered
                .as_ref()
                .map(|decoded| self.score_vector(query_vector, decoded))
                .unwrap_or(0.0);

            results.push(VectorSearchResult {
                docid: entry.docid.clone(),
                collection: entry.collection.clone(),
                path: entry.path.clone(),
                score,
                level: entry.quant_level,
            });
        }

        Ok(results)
    }

    fn default_execution_profile(metric: VectorMetric) -> VectorExecutionProfile {
        match metric {
            VectorMetric::Cosine | VectorMetric::Poincare => VectorExecutionProfile::AnnRescore,
            VectorMetric::L2 => VectorExecutionProfile::AnnRescoreWithExactBackfill,
            VectorMetric::InnerProduct => VectorExecutionProfile::ExactScan,
        }
    }

    fn metric_name(&self) -> &'static str {
        match self.metric {
            VectorMetric::Cosine => "cosine",
            VectorMetric::L2 => "l2",
            VectorMetric::InnerProduct => "inner_product",
            VectorMetric::Poincare => "poincare",
        }
    }

    fn normalized_collection_key(collection: &str) -> &str {
        if collection.is_empty() {
            "__all__"
        } else {
            collection
        }
    }

    fn record_search_latency(&self, collection: &str, latency_ms: f32) {
        let key = format!(
            "{}::{}",
            self.metric_name(),
            Self::normalized_collection_key(collection)
        );
        let mut samples = self.search_latency_samples.write();
        let entry = samples.entry(key).or_default();
        if entry.len() >= SEARCH_LATENCY_SAMPLE_LIMIT {
            entry.pop_front();
        }
        entry.push_back(latency_ms);
    }

    fn percentile(sorted: &[f32], percentile: f32) -> f32 {
        if sorted.is_empty() {
            return 0.0;
        }
        let index = ((sorted.len() - 1) as f32 * percentile).round() as usize;
        sorted[index.min(sorted.len() - 1)]
    }

    fn search_latency_summary_json(&self) -> String {
        let summaries = self
            .search_latency_samples
            .read()
            .iter()
            .map(|(key, samples)| {
                let mut sorted = samples.iter().copied().collect::<Vec<_>>();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                (
                    key.clone(),
                    LatencyPercentiles {
                        samples: sorted.len(),
                        p50_ms: Self::percentile(&sorted, 0.50),
                        p95_ms: Self::percentile(&sorted, 0.95),
                        p99_ms: Self::percentile(&sorted, 0.99),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        serde_json::to_string(&summaries).unwrap_or_else(|_| "{}".to_string())
    }

    fn recover_search_vector(
        &self,
        key: &str,
        entry: &VectorEntry,
        had_quantized_decode: &mut bool,
    ) -> Result<Option<Vec<f32>>> {
        let mut recovered = entry.embedding.clone();
        if recovered.is_none() {
            let mut cache = self.decoded_cache.write();
            if let Some(v) = cache.get(key) {
                recovered = Some(v.clone());
            }
        }
        if recovered.is_none() {
            recovered = self.recover_embedding(entry)?;
            if let Some(decoded) = &recovered {
                if entry.embedding.is_none() && entry.quant_code.is_some() {
                    self.quantized_decode_fallback_count
                        .fetch_add(1, Ordering::Relaxed);
                    *had_quantized_decode = true;
                }
                self.decoded_cache
                    .write()
                    .put(key.to_string(), decoded.clone());
            }
        }
        Ok(recovered)
    }

    /// Phase 19.4: Incremental Deletion.
    /// Removes a vector from storage and marks its HNSW index as 'tombstoned'.
    pub fn remove(&self, collection: &str, path: &str) -> Result<()> {
        let key = format!("v:{}:{}", collection, path);

        // 1. Remove from KV
        self.kv.delete_vector(&key)?;
        self.vector_entries.write().remove(&key);

        // 2. Clear from Cache and tombstone all historical ANN entries
        if Self::ann_contract_enabled(self.metric) {
            self.retire_key(&key)?;
        } else {
            self.decoded_cache.write().pop(&key);
        }

        debug!("Removed vector {} and added tombstone for ID mapping.", key);
        Ok(())
    }

    /// Global Hardware Affinity Controller: Binds the current thread to high-performance cores.
    pub fn pin_to_performance_cores(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            thread_local! {
                static PERF_CORE_AFFINITY_SET: Cell<bool> = const { Cell::new(false) };
            }

            if PERF_CORE_AFFINITY_SET.with(Cell::get) {
                return Ok(());
            }

            use winapi::shared::minwindef::LPCVOID;
            use winapi::um::processthreadsapi::{GetCurrentThread, SetThreadAffinityMask};
            use winapi::um::sysinfoapi::GetLogicalProcessorInformationEx;
            use winapi::um::winnt::LOGICAL_PROCESSOR_RELATIONSHIP;

            unsafe {
                let mut buffer_size = 0;
                // First call to get required buffer size
                GetLogicalProcessorInformationEx(
                    LOGICAL_PROCESSOR_RELATIONSHIP::RelationProcessorCore,
                    std::ptr::null_mut(),
                    &mut buffer_size,
                );

                if buffer_size > 0 {
                    let mut buffer = vec![0u8; buffer_size as usize];
                    let success = GetLogicalProcessorInformationEx(
                        LOGICAL_PROCESSOR_RELATIONSHIP::RelationProcessorCore,
                        buffer.as_mut_ptr() as LPCVOID,
                        &mut buffer_size,
                    );

                    if success != 0 {
                        let p_core_mask = self.detect_p_core_mask();
                        SetThreadAffinityMask(GetCurrentThread(), p_core_mask);
                        PERF_CORE_AFFINITY_SET.with(|flag| flag.set(true));
                        return Ok(());
                    }
                }

                // Fallback for older Windows or failures
                warn!("Failed to detect P/E cores, falling back to default mask");
                SetThreadAffinityMask(GetCurrentThread(), 0xFF);
                PERF_CORE_AFFINITY_SET.with(|flag| flag.set(true));
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Future-proofing: Linux uses sched_setaffinity for core pinning.
            // This is critical for HPC workloads on Linux servers or Steam Deck (Zen 2).
            // trace!("Linux Affinity Mapping: Performance cores selected via CPU_SET.");
        }

        #[cfg(target_os = "macos")]
        {
            // Future-proofing: MacOS (Apple Silicon) uses Quality of Service (QoS) classes.
            // Binds thread to the highest performance cluster (M1/M2/M3 Firestorm cores).
            // trace!("macOS Affinity Mapping: Setting thread QoS to UserInteractive.");
        }

        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn detect_p_core_mask(&self) -> usize {
        use winapi::shared::minwindef::LPCVOID;
        use winapi::um::sysinfoapi::GetLogicalProcessorInformationEx;
        use winapi::um::winnt::{
            RelationProcessorCore, LOGICAL_PROCESSOR_INFORMATION_EX, PROCESSOR_RELATIONSHIP,
        };

        unsafe {
            let mut buffer_size = 0;
            GetLogicalProcessorInformationEx(
                RelationProcessorCore,
                std::ptr::null_mut(),
                &mut buffer_size,
            );

            if buffer_size == 0 {
                return 0xFF;
            }

            let mut buffer = vec![0u8; buffer_size as usize];
            if GetLogicalProcessorInformationEx(
                RelationProcessorCore,
                buffer.as_mut_ptr() as *mut _,
                &mut buffer_size,
            ) != 0
            {
                let mut p_mask = 0usize;
                let mut offset = 0;
                while offset < buffer_size as usize {
                    let info =
                        &*(buffer.as_ptr().add(offset) as *const LOGICAL_PROCESSOR_INFORMATION_EX);
                    if info.Relationship == RelationProcessorCore {
                        let core = info.u.ProcessorCore();
                        // EfficiencyClass > 0 usually denotes a Performance Core on Intel hybrid arch
                        if core.EfficiencyClass > 0 {
                            for i in 0..core.GroupCount {
                                let group = core.GroupMask[i as usize];
                                p_mask |= group.Mask as usize;
                            }
                        }
                    }
                    offset += info.Size as usize;
                }
                if p_mask != 0 {
                    return p_mask;
                }
            }
        }
        0xFF // Fallback to all cores
    }

    fn score_neighbor_distance(&self, distance: f32) -> f32 {
        match self.metric {
            VectorMetric::Cosine => 1.0 - distance,
            VectorMetric::L2 => 1.0 / (1.0 + distance.max(0.0)),
            VectorMetric::InnerProduct => -distance,
            VectorMetric::Poincare => 1.0 / (1.0 + distance.max(0.0)),
        }
    }

    fn score_vector(&self, query: &[f32], candidate: &[f32]) -> f32 {
        match self.metric {
            VectorMetric::Cosine => self.cosine_similarity(query, candidate),
            VectorMetric::L2 => 1.0 / (1.0 + self.l2_distance(query, candidate)),
            VectorMetric::InnerProduct => self.inner_product(query, candidate),
            VectorMetric::Poincare => 1.0 / (1.0 + self.poincare_distance(query, candidate)),
        }
    }

    pub fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if let Some(distance) = <f32 as SpatialSimilarity>::cos(a, b) {
            let similarity = 1.0 - distance as f32;
            if similarity.is_finite() {
                return similarity;
            }
        }
        self.cosine_similarity_fallback(a, b)
    }

    fn cosine_similarity_fallback(&self, a: &[f32], b: &[f32]) -> f32 {
        let mut dot = 0.0;
        let mut norm_a = 0.0;
        let mut norm_b = 0.0;
        for (va, vb) in a.iter().zip(b.iter()) {
            dot += va * vb;
            norm_a += va * va;
            norm_b += vb * vb;
        }
        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }
        dot / (norm_a.sqrt() * norm_b.sqrt())
    }

    pub fn l2_distance(&self, a: &[f32], b: &[f32]) -> f32 {
        if let Some(distance) = <f32 as SpatialSimilarity>::l2(a, b) {
            let distance = distance as f32;
            if distance.is_finite() {
                return distance;
            }
        }
        self.l2_distance_fallback(a, b)
    }

    fn l2_distance_fallback(&self, a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(va, vb)| {
                let diff = va - vb;
                diff * diff
            })
            .sum::<f32>()
            .sqrt()
    }

    pub fn inner_product(&self, a: &[f32], b: &[f32]) -> f32 {
        if let Some(dot) = <f32 as SpatialSimilarity>::dot(a, b) {
            let dot = dot as f32;
            if dot.is_finite() {
                return dot;
            }
        }
        self.inner_product_fallback(a, b)
    }

    fn inner_product_fallback(&self, a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(va, vb)| va * vb).sum()
    }

    pub fn poincare_distance(&self, a: &[f32], b: &[f32]) -> f32 {
        let dist = DistPoincare {};
        dist.eval(a, b)
    }

    /// Bitwise and VNNI similarity using optimized kernels (Zero-Decode)
    pub fn bitwise_similarity(&self, a: &[u8], b: &[u8], level: QuantLevel) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        match level {
            QuantLevel::Background => {
                let distance = if self.cpu_features.avx512 {
                    unsafe { self.safe_avx512_hamming_ternary(a, b) }
                } else if self.cpu_features.avx2 {
                    unsafe { self.safe_avx2_hamming_ternary(a, b) }
                } else {
                    self.software_hamming(a, b)
                };
                1.0 - (distance as f32 / (a.len() * 8) as f32)
            }
            QuantLevel::Cold => {
                // INT4 VNNI Acceleration
                let dot = if self.cpu_features.avx512vnni {
                    unsafe { self.avx512_vnni_int4_dot(a, b) }
                } else if self.cpu_features.avxvnni {
                    unsafe { self.avx_vnni_int4_dot(a, b) }
                } else {
                    self.software_int4_dot(a, b) as f32
                };

                // Max possible dot for 4-bit (0-15) over dim (a.len * 2)
                let max_dot = (a.len() * 2 * 15 * 15) as f32;
                dot / max_dot.max(1.0)
            }
            _ => 1.0, // Should use standard search for Full/Warm
        }
    }

    /// Wrapper for safe AVX-512 execution
    unsafe fn safe_avx512_hamming_ternary(&self, a: &[u8], b: &[u8]) -> usize {
        if a.len() != b.len() {
            return self.software_hamming(a, b);
        }
        #[cfg(target_arch = "x86_64")]
        if self.cpu_features.avx512 {
            return self.avx512_hamming_ternary(a, b);
        }
        self.software_hamming(a, b)
    }

    /// Wrapper for safe AVX2 execution
    unsafe fn safe_avx2_hamming_ternary(&self, a: &[u8], b: &[u8]) -> usize {
        if a.len() != b.len() {
            return self.software_hamming(a, b);
        }
        #[cfg(target_arch = "x86_64")]
        if self.cpu_features.avx2 {
            return self.avx2_hamming_ternary(a, b);
        }
        self.software_hamming(a, b)
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512bitalg")]
    unsafe fn avx512_hamming_ternary(&self, a: &[u8], b: &[u8]) -> usize {
        use std::arch::x86_64::*;
        let mut distance = 0;
        let mut i = 0;
        while i + 64 <= a.len() {
            let va = _mm512_loadu_si512(a.as_ptr().add(i) as *const _);
            let vb = _mm512_loadu_si512(b.as_ptr().add(i) as *const _);
            // Non-zero symmetric difference for ternary
            let vxor = _mm512_xor_si512(va, vb);
            let vcnt = _mm512_popcnt_epi8(vxor);
            // sad_epu8 against zero is a fast way to sum bytes into u64s
            let vsum = _mm512_sad_epu8(vcnt, _mm512_setzero_si512());
            let mut buf = [0u64; 8];
            _mm512_storeu_si512(buf.as_mut_ptr() as *mut _, vsum);
            for x in buf {
                distance += x as usize;
            }
            i += 64;
        }
        distance + self.software_hamming(&a[i..], &b[i..])
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx512vnni,avx512f,avx512bw,avx512vl")]
    pub unsafe fn avx512_vnni_int4_dot(&self, a: &[u8], b: &[u8]) -> f32 {
        use std::arch::x86_64::*;
        let mut total_dot = 0u32;
        let mut i = 0;
        let low_mask = _mm512_set1_epi8(0x0F);

        // Process 64 bytes (128 INT4 elements) per iteration
        while i + 63 < a.len() {
            let va = _mm512_loadu_si512(a.as_ptr().add(i) as *const _);
            let vb = _mm512_loadu_si512(b.as_ptr().add(i) as *const _);

            // Unpack nibbles: low = bits 0-3, high = bits 4-7
            let va_low = _mm512_and_si512(va, low_mask);
            let va_high = _mm512_and_si512(_mm512_srli_epi16(va, 4), low_mask);

            let vb_low = _mm512_and_si512(vb, low_mask);
            let vb_high = _mm512_and_si512(_mm512_srli_epi16(vb, 4), low_mask);

            // VNNI Multiply-Accumulate (u8 * u8 -> u32)
            // dpbusd treats src1 as u8, src2 as i8. Since we are 0-15, u8->i8 is safe.
            let vres_low = _mm512_dpbusd_epi32(_mm512_setzero_si512(), va_low, vb_low);
            let vres_high = _mm512_dpbusd_epi32(_mm512_setzero_si512(), va_high, vb_high);

            let vresid = _mm512_add_epi32(vres_low, vres_high);
            total_dot += _mm512_reduce_add_epi32(vresid) as u32;

            i += 64;
        }

        total_dot as f32 + self.software_int4_dot(&a[i..], &b[i..]) as f32
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avxvnni,avx2")]
    pub unsafe fn avx_vnni_int4_dot(&self, a: &[u8], b: &[u8]) -> f32 {
        use std::arch::x86_64::*;
        let mut total_dot = 0u32;
        let mut i = 0;
        let low_mask = _mm256_set1_epi8(0x0F);

        // Process 32 bytes (64 INT4 elements) per iteration
        while i + 31 < a.len() {
            let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
            let vb = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);

            let va_low = _mm256_and_si256(va, low_mask);
            let va_high = _mm256_and_si256(_mm256_srli_epi16(va, 4), low_mask);

            let vb_low = _mm256_and_si256(vb, low_mask);
            let vb_high = _mm256_and_si256(_mm256_srli_epi16(vb, 4), low_mask);

            let vres_low = _mm256_dpbusd_epi32(_mm256_setzero_si256(), va_low, vb_low);
            let vres_high = _mm256_dpbusd_epi32(_mm256_setzero_si256(), va_high, vb_high);

            let vresid = _mm256_add_epi32(vres_low, vres_high);

            // Extract and sum results from 256-bit register
            let mut res = [0i32; 8];
            _mm256_storeu_si256(res.as_mut_ptr() as *mut _, vresid);
            total_dot += res.iter().sum::<i32>() as u32;

            i += 32;
        }

        total_dot as f32 + self.software_int4_dot(&a[i..], &b[i..]) as f32
    }

    fn software_int4_dot(&self, a: &[u8], b: &[u8]) -> usize {
        let mut sum = 0;
        for (&ba, &bb) in a.iter().zip(b) {
            let q1a = ba & 0x0F;
            let q2a = (ba >> 4) & 0x0F;
            let q1b = bb & 0x0F;
            let q2b = (bb >> 4) & 0x0F;
            sum += (q1a as usize * q1b as usize) + (q2a as usize * q2b as usize);
        }
        sum
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn avx2_hamming_ternary(&self, a: &[u8], b: &[u8]) -> usize {
        use std::arch::x86_64::*;
        let mut distance = 0;
        let mut i = 0;
        while i + 32 <= a.len() {
            let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const _);
            let vb = _mm256_loadu_si256(b.as_ptr().add(i) as *const _);
            let vxor = _mm256_xor_si256(va, vb);
            // popcnt fallback for AVX2
            let mut buf = [0u8; 32];
            _mm256_storeu_si256(buf.as_mut_ptr() as *mut _, vxor);
            for byte in buf {
                distance += byte.count_ones() as usize;
            }
            i += 32;
        }
        distance + self.software_hamming(&a[i..], &b[i..])
    }

    fn software_hamming(&self, a: &[u8], b: &[u8]) -> usize {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x ^ y).count_ones() as usize)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InMemoryStorage;
    use tempfile::tempdir;

    #[test]
    fn test_quantizer_batch_training() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 384, 1000, VectorMetric::Cosine);

        // 1. Add some mock vectors (Normal distribution-ish)
        for i in 0..50 {
            let v: Vec<f32> = (0..384).map(|j| (i + j) as f32 * 0.1).collect();
            let _ = store.add("test", &format!("p{}", i), &format!("d{}", i), 0, v);
        }

        // 2. Trigger quantize_query which should now use batch sampling
        let query: Vec<f32> = vec![0.5; 384];
        let code = store
            .quantize_query(&query, QuantLevel::Cold)
            .expect("Quantization failed");

        // 3. Verify quantizer was created and trained
        let qs = store.quantizers.read();
        assert!(qs.iter().any(|(l, _)| *l == QuantLevel::Cold));
        assert_eq!(code.len(), 384 / 2);
    }

    #[test]
    fn test_similarity_accuracy() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 64, 100, VectorMetric::Cosine);

        // Exact match test
        let a = vec![0xAAu8; 32];
        let b = vec![0xAAu8; 32];
        let score = store.bitwise_similarity(&a, &b, QuantLevel::Background);
        assert!((score - 1.0).abs() < 1e-6);

        // Orthogonal-ish test
        let c = vec![0x55u8; 32]; // XOR with 0xAA (1010) results in all ones (1111)
        let score_diff = store.bitwise_similarity(&a, &c, QuantLevel::Background);
        assert!(score_diff < 0.1);
    }

    #[test]
    fn test_poincare_distance() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 4, 100, VectorMetric::Poincare);

        // 1. Centers and points
        let a = vec![0.0, 0.0, 0.0, 0.0];
        let b = vec![0.5, 0.0, 0.0, 0.0];

        // Manual calculation for d(0, 0.5):
        // ||a-b||^2 = 0.25
        // ||a||^2 = 0, ||b||^2 = 0.25
        // arg = 1 + 2 * 0.25 / ((1 - 0) * (1 - 0.25)) = 1 + 0.5 / 0.75 = 1 + 2/3 = 1.6666...
        // dist = acosh(1.6666...) = ln(1.6666 + sqrt(1.6666^2 - 1)) = ln(1.6666 + 1.3333) = ln(3) = 1.0986

        let dist = DistPoincare {}.eval(&a, &b);
        assert!((dist - 1.0986123).abs() < 1e-4);

        // 2. Test SIMD if detected
        if store.cpu_features.avx2 {
            // This implicitly tests the avx2_poincare_norms via the main eval dispatch
            let dist_simd = DistPoincare {}.eval(&a, &b);
            assert!((dist_simd - 1.0986123).abs() < 1e-4);
        }
    }

    #[test]
    fn test_simsimd_similarity_matches_fallbacks() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 8, 100, VectorMetric::Cosine);
        let a = vec![0.1, -0.2, 0.3, 0.4, -0.5, 0.6, 0.7, -0.8];
        let b = vec![0.2, -0.1, 0.4, 0.1, -0.3, 0.9, 0.5, -0.4];

        let cosine = store.cosine_similarity(&a, &b);
        let cosine_fallback = store.cosine_similarity_fallback(&a, &b);
        assert!((cosine - cosine_fallback).abs() < 1e-5);

        let l2 = store.l2_distance(&a, &b);
        let l2_fallback = store.l2_distance_fallback(&a, &b);
        assert!((l2 - l2_fallback).abs() < 1e-5);

        let inner = store.inner_product(&a, &b);
        let inner_fallback = store.inner_product_fallback(&a, &b);
        assert!((inner - inner_fallback).abs() < 1e-5);
    }

    #[test]
    fn test_simsimd_similarity_falls_back_on_shape_mismatch() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 4, 100, VectorMetric::Cosine);
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![1.0, 2.0];

        assert_eq!(
            store.cosine_similarity(&a, &b),
            store.cosine_similarity_fallback(&a, &b)
        );
        assert_eq!(
            store.l2_distance(&a, &b),
            store.l2_distance_fallback(&a, &b)
        );
        assert_eq!(
            store.inner_product(&a, &b),
            store.inner_product_fallback(&a, &b)
        );
    }

    #[test]
    fn test_load_rejects_persisted_config_mismatch() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage.clone(), 16, 100, VectorMetric::Poincare);
        store.save_metadata().expect("metadata should persist");

        let dir = tempdir().expect("tempdir");
        let err = match VectorStore::load(storage, dir.path(), 32, VectorMetric::Cosine) {
            Ok(_) => panic!("mismatched config must fail fast"),
            Err(err) => err,
        };
        assert!(matches!(err, EngramError::InvalidInput(_)));
    }

    #[test]
    fn test_load_rebuilds_quantized_entries_without_full_embeddings() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage.clone(), 8, 100, VectorMetric::Cosine);
        let embedding = vec![0.1, 0.2, 0.3, 0.4, 0.1, 0.2, 0.3, 0.4];
        store
            .add_at_level(
                "docs",
                "quantized",
                "doc-1",
                0,
                embedding.clone(),
                QuantLevel::Cold,
            )
            .expect("cold vector should index");

        let dir = tempdir().expect("tempdir");
        let loaded = VectorStore::load(storage, dir.path(), 8, VectorMetric::Cosine)
            .expect("rebuild should restore quantized entry");
        let results = loaded
            .search("docs", &embedding, 5)
            .expect("search should succeed after rebuild");

        assert!(results.iter().any(|result| result.path == "quantized"));
        let runtime = loaded.runtime_stats();
        assert_eq!(runtime.rebuild_count, 1);
        assert_eq!(runtime.snapshot_load_count, 0);
    }

    #[test]
    fn test_load_rebuilds_when_snapshot_manifest_mismatches() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage.clone(), 4, 100, VectorMetric::Cosine);
        let embedding = vec![1.0, 0.0, 0.0, 0.0];

        store
            .add("docs", "stable", "doc-1", 0, embedding.clone())
            .expect("vector should index");

        let dir = tempdir().expect("tempdir");
        store.save(dir.path()).expect("snapshot should save");

        let mut manifest: VectorSnapshotManifest = bincode::deserialize(
            &storage
                .get_collection(VECTOR_SNAPSHOT_MANIFEST_KEY)
                .expect("manifest fetch should work")
                .expect("manifest should exist"),
        )
        .expect("manifest should deserialize");
        manifest.id_map_checksum = "tampered-checksum".to_string();
        storage
            .put_collection(
                VECTOR_SNAPSHOT_MANIFEST_KEY,
                &bincode::serialize(&manifest).expect("manifest should serialize"),
            )
            .expect("tampered manifest should persist");

        let loaded = VectorStore::load(storage, dir.path(), 4, VectorMetric::Cosine)
            .expect("load should rebuild when manifest mismatches");
        let results = loaded
            .search("docs", &embedding, 5)
            .expect("search should succeed after rebuild");

        assert!(results.iter().any(|result| result.path == "stable"));
        let runtime = loaded.runtime_stats();
        assert_eq!(runtime.rebuild_count, 1);
        assert_eq!(runtime.snapshot_load_count, 0);
    }

    #[test]
    fn test_load_restores_tombstones_from_id_map() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage.clone(), 4, 100, VectorMetric::Cosine);
        let embedding = vec![1.0, 0.0, 0.0, 0.0];

        store
            .add("docs", "stale", "doc-1", 0, embedding.clone())
            .expect("vector should index");
        store
            .remove("docs", "stale")
            .expect("remove should tombstone ANN entry");

        let dir = tempdir().expect("tempdir");
        store.save(dir.path()).expect("snapshot should save");

        let loaded = VectorStore::load(storage, dir.path(), 4, VectorMetric::Cosine)
            .expect("load should restore tombstones");
        let results = loaded
            .search("docs", &embedding, 5)
            .expect("search should honor restored tombstones");

        assert!(results.is_empty());
        let runtime = loaded.runtime_stats();
        assert!(runtime.snapshot_load_count >= 1);
        assert!(runtime.tombstone_count >= 1);
        assert!(runtime.tombstone_ratio > 0.0);
    }

    #[test]
    fn test_replace_and_remove_do_not_leave_active_stale_entries() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 4, 100, VectorMetric::Cosine);
        let first = vec![1.0, 0.0, 0.0, 0.0];
        let second = vec![0.0, 1.0, 0.0, 0.0];

        store
            .add("docs", "same-path", "doc-1", 0, first)
            .expect("first add should work");
        store
            .add("docs", "same-path", "doc-1", 0, second.clone())
            .expect("replacement add should work");

        let replaced = store
            .search("docs", &second, 10)
            .expect("search after replace should work");
        assert_eq!(
            replaced
                .iter()
                .filter(|result| result.path == "same-path")
                .count(),
            1
        );

        store
            .remove("docs", "same-path")
            .expect("remove should tombstone old entries");
        let after_remove = store
            .search("docs", &second, 10)
            .expect("search after remove should work");
        assert!(after_remove.is_empty());
    }

    #[test]
    fn test_l2_search_prefers_nearest_vector() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 2, 100, VectorMetric::L2);

        store
            .add("docs", "near", "doc-near", 0, vec![0.0, 0.0])
            .expect("near vector should index");
        store
            .add("docs", "far", "doc-far", 0, vec![10.0, 10.0])
            .expect("far vector should index");

        let results = store
            .search("docs", &[1.0, 1.0], 2)
            .expect("l2 search should work");

        assert_eq!(
            results.first().map(|result| result.path.as_str()),
            Some("near")
        );
        assert!(results[0].score > results[1].score);
        let runtime = store.runtime_stats();
        assert_eq!(runtime.execution_profile, "ann_rescore_with_exact_backfill");
        assert_eq!(runtime.ann_search_count, 1);
    }

    #[test]
    fn test_inner_product_search_prefers_highest_dot_product() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 2, 100, VectorMetric::InnerProduct);

        store
            .add("docs", "strong", "doc-strong", 0, vec![10.0, 0.0])
            .expect("strong vector should index");
        store
            .add("docs", "weak", "doc-weak", 0, vec![0.5, 0.5])
            .expect("weak vector should index");

        let results = store
            .search("docs", &[1.0, 0.0], 2)
            .expect("inner product search should work");

        assert_eq!(
            results.first().map(|result| result.path.as_str()),
            Some("strong")
        );
        assert!(results[0].score > results[1].score);
        let runtime = store.runtime_stats();
        assert_eq!(runtime.execution_profile, "exact_scan");
        assert_eq!(runtime.last_execution_mode, "exact_scan");
        assert_eq!(runtime.exact_scan_count, 1);
    }

    #[test]
    fn test_inner_product_formally_skips_ann_snapshots() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage.clone(), 2, 100, VectorMetric::InnerProduct);

        store
            .add("docs", "strong", "doc-strong", 0, vec![10.0, 0.0])
            .expect("inner-product vector should index");

        let dir = tempdir().expect("tempdir");
        store.save(dir.path()).expect("save should succeed");
        assert!(!dir.path().join("hnsw_Full.hnsw.graph").exists());

        let loaded = VectorStore::load(storage, dir.path(), 2, VectorMetric::InnerProduct)
            .expect("load should succeed without ANN snapshots");
        let results = loaded
            .search("docs", &[1.0, 0.0], 2)
            .expect("exact-scan load should still search");

        assert_eq!(
            results.first().map(|result| result.path.as_str()),
            Some("strong")
        );
        assert_eq!(loaded.len(), 1);
        let runtime = loaded.runtime_stats();
        assert_eq!(runtime.snapshot_load_count, 0);
        assert_eq!(runtime.rebuild_count, 0);
        assert_eq!(runtime.execution_profile, "exact_scan");
    }

    #[test]
    fn test_runtime_stats_expose_fallback_and_latency_metrics() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 4, 100, VectorMetric::Cosine);

        store
            .add("docs", "fp32", "doc-fp32", 0, vec![1.0, 0.0, 0.0, 0.0])
            .expect("fp32 vector should index");
        store
            .add_at_level(
                "docs",
                "quantized",
                "doc-quantized",
                0,
                vec![0.9, 0.1, 0.0, 0.0],
                QuantLevel::Cold,
            )
            .expect("quantized vector should index");

        let _ = store
            .search("docs", &[1.0, 0.0, 0.0, 0.0], 5)
            .expect("search should work");

        let runtime = store.runtime_stats();
        assert_eq!(runtime.ann_search_count, 1);
        assert_eq!(runtime.exact_backfill_count, 1);
        assert!(runtime.exact_scan_fallback_rate > 0.0);
        assert!(runtime.quantized_decode_fallback_count >= 1);
        assert!(runtime.quantized_decode_fallback_rate > 0.0);
        assert!(runtime
            .search_latency_by_metric_and_collection_json
            .contains("cosine::docs"));
    }

    #[test]
    fn test_runtime_stats_expose_tombstone_ratio() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage, 2, 100, VectorMetric::Cosine);

        store
            .add("docs", "stale", "doc-stale", 0, vec![1.0, 0.0])
            .expect("vector should index");
        store
            .remove("docs", "stale")
            .expect("remove should create tombstone");

        let runtime = store.runtime_stats();
        assert!(runtime.tombstone_count >= 1);
        assert!(runtime.tombstone_ratio > 0.0);
    }

    #[test]
    fn test_quant_metadata_persists_across_load() {
        let storage = Arc::new(InMemoryStorage::new());
        let store = VectorStore::new(storage.clone(), 4, 100, VectorMetric::Cosine);
        let metadata = VectorQuantMetadata {
            backend: "legacy_quant_metadata".to_string(),
            rotation_kind: "none".to_string(),
            subquantizers: 0,
            codebook_bits: 4,
            approximate_bits_per_vector: 8,
            compression_ratio: 16.0,
        };
        store
            .set_quant_metadata(metadata.clone())
            .expect("quant metadata should persist");

        let dir = tempdir().expect("tempdir");
        let loaded = VectorStore::load(storage, dir.path(), 4, VectorMetric::Cosine)
            .expect("load should restore quant metadata");

        assert_eq!(loaded.quant_metadata(), Some(metadata));
    }
}
