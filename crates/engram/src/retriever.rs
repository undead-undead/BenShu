//! Recursive Hierarchical Retrieval
//!
//! Implements multi-level retrieval that traverses context levels (L0->L1->L2):
//! 1. Broad initial search (L0) to identify structural landmarks
//! 2. Drill-down search on high-value landmarks (L1)
//! 3. Merge and re-rank results with relevance boosting

use crate::error::{EngramError, Result};
use crate::hybrid_search::{HybridSearchEngine, HybridSearchResult};
use crate::intent::IntentAnalyzer;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, trace, warn};

/// Configuration for HierarchicalRetriever
#[derive(Debug, Clone)]
pub struct HierarchicalRetrieverConfig {
    /// Initial broad search multiplier (raw results = limit * multiplier)
    pub initial_search_multiplier: usize,
    /// Minimum RRF score threshold to consider a document as a landmark
    pub landmark_score_threshold: f64,
    /// Maximum number of landmarks to drill down into
    pub max_landmarks: usize,
    /// Number of results to retrieve per landmark
    pub drill_down_limit: usize,
    /// Relevance boost factor for drill-down results (e.g., 1.2 = 20% boost)
    pub drill_down_boost: f64,
    /// Whether to use intent analysis to guide landmark selection
    pub use_intent_analysis: bool,
    /// Whether to preserve original ranking for non-landmark results
    pub preserve_original_ranking: bool,
    /// Multiplier used when the first candidate pool is too sparse
    pub candidate_top_up_multiplier: usize,
    /// Max normalized query chars before truncation kicks in
    pub max_query_chars: usize,
    /// Token bucket capacity for recursive retrieval budget
    pub token_bucket_capacity: usize,
    /// Token bucket refill rate per second
    pub token_bucket_refill_per_sec: f64,
    /// Negative cache TTL for empty-result signatures
    pub negative_cache_ttl_secs: u64,
    /// Max number of negative-cache signatures to keep
    pub negative_cache_capacity: usize,
    /// Cooldown window for signatures that were recently throttled
    pub signature_cooldown_secs: u64,
}

impl Default for HierarchicalRetrieverConfig {
    fn default() -> Self {
        Self {
            initial_search_multiplier: 3,
            landmark_score_threshold: 0.3,
            max_landmarks: 3,
            drill_down_limit: 5,
            drill_down_boost: 1.2,
            use_intent_analysis: true,
            preserve_original_ranking: false,
            candidate_top_up_multiplier: 2,
            max_query_chars: 1024,
            token_bucket_capacity: 64,
            token_bucket_refill_per_sec: 16.0,
            negative_cache_ttl_secs: 30,
            negative_cache_capacity: 256,
            signature_cooldown_secs: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RetrievalReport {
    pub query: String,
    pub query_signature: String,
    pub requested_limit: usize,
    pub initial_limit: usize,
    pub initial_result_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadened_limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadened_result_count: Option<usize>,
    pub landmark_count: usize,
    pub drill_down_attempts: usize,
    pub drill_down_successes: usize,
    pub merged_result_count: usize,
    pub final_result_count: usize,
    #[serde(default)]
    pub safety_net_triggered: bool,
    #[serde(default)]
    pub candidate_top_up_applied: bool,
    #[serde(default)]
    pub backfilled_from_candidate_pool: bool,
    #[serde(default)]
    pub dos_hardening_triggered: bool,
    #[serde(default)]
    pub negative_cache_hit: bool,
    #[serde(default)]
    pub throttled: bool,
    #[serde(default)]
    pub signature_cooldown_hit: bool,
    #[serde(default)]
    pub query_truncated: bool,
    pub token_cost: usize,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degradation_reasons: Vec<String>,
    pub latency_ms: u64,
}

impl RetrievalReport {
    fn push_reason(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        if !self
            .degradation_reasons
            .iter()
            .any(|existing| existing == &reason)
        {
            self.degradation_reasons.push(reason);
        }
    }

    pub fn degradation_summary(&self) -> Option<String> {
        if self.degradation_reasons.is_empty() {
            None
        } else {
            Some(self.degradation_reasons.join(", "))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecursiveSearchOutcome {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub results: Vec<HybridSearchResult>,
    pub report: RetrievalReport,
}

/// Retriever that performs recursive/hierarchical search
pub struct HierarchicalRetriever {
    engine: Arc<HybridSearchEngine>,
    analyzer: IntentAnalyzer,
    config: HierarchicalRetrieverConfig,
    dos_state: Mutex<RetrievalDosState>,
}

#[derive(Debug, Clone)]
struct NegativeCacheEntry {
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct RetrievalDosState {
    available_tokens: f64,
    last_refill: Instant,
    negative_cache: HashMap<String, NegativeCacheEntry>,
    throttled_signatures: HashMap<String, Instant>,
}

impl RetrievalDosState {
    fn new(capacity: usize) -> Self {
        Self {
            available_tokens: capacity as f64,
            last_refill: Instant::now(),
            negative_cache: HashMap::new(),
            throttled_signatures: HashMap::new(),
        }
    }
}

impl HierarchicalRetriever {
    pub fn new(engine: Arc<HybridSearchEngine>) -> Self {
        Self {
            engine,
            analyzer: IntentAnalyzer::new(),
            config: HierarchicalRetrieverConfig::default(),
            dos_state: Mutex::new(RetrievalDosState::new(
                HierarchicalRetrieverConfig::default().token_bucket_capacity,
            )),
        }
    }

    pub fn engine(&self) -> Arc<HybridSearchEngine> {
        Arc::clone(&self.engine)
    }

    pub fn with_config(
        engine: Arc<HybridSearchEngine>,
        config: HierarchicalRetrieverConfig,
    ) -> Self {
        info!("Initializing HierarchicalRetriever with custom config");
        Self {
            engine,
            analyzer: IntentAnalyzer::new(),
            dos_state: Mutex::new(RetrievalDosState::new(config.token_bucket_capacity)),
            config,
        }
    }

    pub fn update_config(&mut self, config: HierarchicalRetrieverConfig) {
        debug!("Updating HierarchicalRetriever config");
        let mut dos_state = self.dos_state.lock();
        dos_state.available_tokens = dos_state
            .available_tokens
            .min(config.token_bucket_capacity as f64);
        self.config = config;
    }

    fn normalize_query_signature(&self, query: &str) -> String {
        query
            .split_whitespace()
            .map(|part| part.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn estimate_token_cost(&self, query: &str, limit: usize) -> usize {
        let char_cost = (query.chars().count().max(1) / 24).max(1);
        let term_cost = query.split_whitespace().count().max(1);
        let result_cost = (limit / 2).max(1);
        char_cost.max(term_cost).saturating_add(result_cost)
    }

    fn prune_negative_cache(&self, state: &mut RetrievalDosState, now: Instant) {
        state
            .negative_cache
            .retain(|_, entry| entry.expires_at > now);
        while state.negative_cache.len() > self.config.negative_cache_capacity {
            if let Some(signature) = state.negative_cache.keys().next().cloned() {
                state.negative_cache.remove(&signature);
            } else {
                break;
            }
        }
    }

    fn prune_throttled_signatures(&self, state: &mut RetrievalDosState, now: Instant) {
        state
            .throttled_signatures
            .retain(|_, expires_at| *expires_at > now);
    }

    fn try_consume_budget(
        &self,
        report: &mut RetrievalReport,
        signature: &str,
        token_cost: usize,
    ) -> bool {
        let now = Instant::now();
        let mut state = self.dos_state.lock();
        self.prune_negative_cache(&mut state, now);
        self.prune_throttled_signatures(&mut state, now);

        if state
            .negative_cache
            .get(signature)
            .is_some_and(|entry| entry.expires_at > now)
        {
            report.dos_hardening_triggered = true;
            report.negative_cache_hit = true;
            report.push_reason("negative_cache_hit");
            return false;
        }

        if state
            .throttled_signatures
            .get(signature)
            .is_some_and(|expires_at| *expires_at > now)
        {
            report.dos_hardening_triggered = true;
            report.throttled = true;
            report.signature_cooldown_hit = true;
            report.push_reason("query_signature_cooldown");
            return false;
        }

        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            let refill = elapsed * self.config.token_bucket_refill_per_sec;
            state.available_tokens =
                (state.available_tokens + refill).min(self.config.token_bucket_capacity as f64);
            state.last_refill = now;
        }

        if state.available_tokens + f64::EPSILON < token_cost as f64 {
            report.dos_hardening_triggered = true;
            report.throttled = true;
            report.push_reason("query_throttled");
            state.throttled_signatures.insert(
                signature.to_string(),
                now + std::time::Duration::from_secs(self.config.signature_cooldown_secs),
            );
            return false;
        }

        state.available_tokens -= token_cost as f64;
        state.throttled_signatures.remove(signature);
        true
    }

    fn store_negative_cache_if_needed(&self, signature: &str, result_count: usize) {
        let now = Instant::now();
        let mut state = self.dos_state.lock();
        self.prune_negative_cache(&mut state, now);
        if result_count == 0 {
            state.negative_cache.insert(
                signature.to_string(),
                NegativeCacheEntry {
                    expires_at: now
                        + std::time::Duration::from_secs(self.config.negative_cache_ttl_secs),
                },
            );
            self.prune_negative_cache(&mut state, now);
        } else {
            state.negative_cache.remove(signature);
        }
    }

    /// Identify high-value structural landmarks from results
    fn identify_landmarks(
        &self,
        _query: &str,
        results: &[HybridSearchResult],
    ) -> Vec<(String, String)> {
        let mut landmarks = Vec::new();

        for res in results {
            if res.rrf_score < self.config.landmark_score_threshold {
                continue;
            }

            // Landmarks must have structural metadata (abstract/overview)
            let is_structural =
                res.document.abstract_content.is_some() || res.document.overview_content.is_some();

            if !is_structural {
                continue;
            }

            landmarks.push((res.document.collection.clone(), res.document.path.clone()));

            if landmarks.len() >= self.config.max_landmarks {
                break;
            }
        }

        landmarks
    }

    /// Perform targeted drill-down into a specific document tree/prefix
    async fn drill_down(
        &self,
        query: &str,
        collection: &str,
        path: &str,
    ) -> Result<Vec<HybridSearchResult>> {
        let prefix = format!("{}:{}", collection, path);
        trace!("Drilling down into {}", prefix);

        let mut results = self
            .engine
            .search_with_path(query, collection, path, self.config.drill_down_limit)
            .map_err(|e| EngramError::RetrievalError(format!("Drill down failed: {}", e)))?;

        // Apply relevance boost
        for res in &mut results {
            res.rrf_score *= self.config.drill_down_boost;
        }

        Ok(results)
    }

    fn broadened_limit(&self, limit: usize, initial_limit: usize) -> usize {
        let multiplier = self.config.candidate_top_up_multiplier.max(1);
        initial_limit
            .saturating_mul(multiplier)
            .max(limit.saturating_mul(4))
            .max(limit)
    }

    /// Perform a recursive search based on hierarchical landmarks
    pub async fn search_recursive(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        Ok(self
            .search_recursive_with_report(query, limit)
            .await?
            .results)
    }

    /// Perform a recursive search and return both results and a degradation-aware report
    pub async fn search_recursive_with_report(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<RecursiveSearchOutcome> {
        let start_time = Instant::now();
        let mut effective_query = query.trim().to_string();
        let mut report = RetrievalReport {
            query: effective_query.clone(),
            requested_limit: limit,
            ..Default::default()
        };

        if query.trim().is_empty() || limit == 0 {
            report.latency_ms = start_time.elapsed().as_millis() as u64;
            return Ok(RecursiveSearchOutcome {
                results: Vec::new(),
                report,
            });
        }

        if effective_query.chars().count() > self.config.max_query_chars {
            effective_query = effective_query
                .chars()
                .take(self.config.max_query_chars)
                .collect();
            report.query = effective_query.clone();
            report.query_truncated = true;
            report.dos_hardening_triggered = true;
            report.push_reason("query_truncated_for_budget");
        }

        report.query_signature = self.normalize_query_signature(&effective_query);
        report.token_cost = self.estimate_token_cost(&effective_query, limit);
        let query_signature = report.query_signature.clone();
        let token_cost = report.token_cost;
        if !self.try_consume_budget(&mut report, &query_signature, token_cost) {
            report.degraded = !report.degradation_reasons.is_empty();
            report.latency_ms = start_time.elapsed().as_millis() as u64;
            return Ok(RecursiveSearchOutcome {
                results: Vec::new(),
                report,
            });
        }

        // 1. Intent Analysis
        let plan = self.analyzer.analyze(&effective_query).await?;

        // 2. Initial Broad Search (L0)
        let initial_limit = limit.max(limit.saturating_mul(self.config.initial_search_multiplier));
        report.initial_limit = initial_limit;
        let raw_results = self.engine.search(&plan.original_query, initial_limit)?;
        report.initial_result_count = raw_results.len();

        if raw_results.is_empty() {
            self.store_negative_cache_if_needed(&report.query_signature, 0);
            report.latency_ms = start_time.elapsed().as_millis() as u64;
            return Ok(RecursiveSearchOutcome {
                results: Vec::new(),
                report,
            });
        }

        let mut candidate_pool = raw_results;
        if candidate_pool.len() < limit {
            report.safety_net_triggered = true;
            report.candidate_top_up_applied = true;
            report.push_reason("candidate_pool_below_limit");
            let broadened_limit = self.broadened_limit(limit, initial_limit);
            report.broadened_limit = Some(broadened_limit);

            if broadened_limit > initial_limit {
                let broadened_results =
                    self.engine.search(&plan.original_query, broadened_limit)?;
                report.broadened_result_count = Some(broadened_results.len());
                if broadened_results.len() > candidate_pool.len() {
                    candidate_pool = broadened_results;
                }
            }
        }

        // 3. Identification & Drill-down
        let landmarks = self.identify_landmarks(&plan.original_query, &candidate_pool);
        report.landmark_count = landmarks.len();
        let mut final_results = Vec::new();
        let mut seen_docids = HashSet::new();
        let mut drill_down_unique_count = 0usize;

        if !landmarks.is_empty() {
            for (col, path) in landmarks {
                report.drill_down_attempts = report.drill_down_attempts.saturating_add(1);
                match self.drill_down(&plan.original_query, &col, &path).await {
                    Ok(drill_results) => {
                        if !drill_results.is_empty() {
                            report.drill_down_successes =
                                report.drill_down_successes.saturating_add(1);
                        }
                        for res in drill_results {
                            if seen_docids.insert(res.document.docid.clone()) {
                                final_results.push(res);
                                drill_down_unique_count = drill_down_unique_count.saturating_add(1);
                            }
                        }
                    }
                    Err(err) => {
                        warn!(
                            "Hierarchical drill-down degraded for {}:{}: {}",
                            col, path, err
                        );
                        report.safety_net_triggered = true;
                        report.push_reason("drill_down_failed");
                    }
                }
            }
        }

        // 4. Merge initial results (avoiding duplicates)
        for res in candidate_pool {
            if seen_docids.insert(res.document.docid.clone()) {
                final_results.push(res);
            }
        }
        report.merged_result_count = final_results.len();

        if report.landmark_count > 0
            && drill_down_unique_count < limit
            && report.merged_result_count > drill_down_unique_count
        {
            report.safety_net_triggered = true;
            report.backfilled_from_candidate_pool = true;
            report.push_reason("backfilled_from_candidate_pool");
        }

        // 5. Final Ranking & Truncation
        if !self.config.preserve_original_ranking {
            final_results.sort_by(|a, b| {
                b.rrf_score
                    .partial_cmp(&a.rrf_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        final_results.truncate(limit);
        report.final_result_count = final_results.len();
        if report.final_result_count < limit {
            report.push_reason("returned_below_limit");
        }
        self.store_negative_cache_if_needed(&report.query_signature, report.final_result_count);
        report.degraded = !report.degradation_reasons.is_empty();
        report.latency_ms = start_time.elapsed().as_millis() as u64;

        debug!(
            "Hierarchical search for '{}' completed in {:.2}ms ({} results)",
            query,
            start_time.elapsed().as_secs_f64() * 1000.0,
            final_results.len()
        );

        Ok(RecursiveSearchOutcome {
            results: final_results,
            report,
        })
    }

    pub fn get_stats(&self) -> HierarchicalRetrieverStats {
        HierarchicalRetrieverStats {
            total_searches: 0,
            avg_landmarks_per_search: 0.0,
        }
    }
}

pub struct HierarchicalRetrieverStats {
    pub total_searches: u64,
    pub avg_landmarks_per_search: f64,
}

impl Default for HierarchicalRetriever {
    fn default() -> Self {
        // This is primarily for testing/fallback
        Self::new(Arc::new(
            HybridSearchEngine::new(Default::default(), None).expect("Failed to initialize engine"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hybrid_search::HybridSearchConfig;
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[tokio::test]
    async fn recursive_search_reports_candidate_top_up_when_pool_is_sparse() {
        let temp = tempdir().expect("tempdir");
        let config = HybridSearchConfig {
            db_path: temp.path().join("engram.redb"),
            use_vector: false,
            use_reranker: false,
            ..Default::default()
        };
        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let store = engine.engram_store();
        store
            .store_document(
                "docs",
                "one.md",
                "One",
                "retrieval safety net document one",
                false,
                HashMap::new(),
            )
            .expect("store doc one");
        store
            .store_document(
                "docs",
                "two.md",
                "Two",
                "retrieval safety net document two",
                false,
                HashMap::new(),
            )
            .expect("store doc two");

        let retriever = HierarchicalRetriever::with_config(
            engine,
            HierarchicalRetrieverConfig {
                initial_search_multiplier: 1,
                candidate_top_up_multiplier: 3,
                ..Default::default()
            },
        );

        let outcome = retriever
            .search_recursive_with_report("retrieval safety", 5)
            .await
            .expect("search outcome");

        assert!(outcome.report.candidate_top_up_applied);
        assert!(outcome.report.safety_net_triggered);
        assert!(outcome.report.degraded);
        assert!(outcome
            .report
            .degradation_reasons
            .iter()
            .any(|reason| reason == "candidate_pool_below_limit"));
        assert!(outcome
            .report
            .degradation_reasons
            .iter()
            .any(|reason| reason == "returned_below_limit"));
        assert_eq!(outcome.results.len(), 2);
    }

    #[tokio::test]
    async fn recursive_search_short_circuits_on_negative_cache_hit() {
        let temp = tempdir().expect("tempdir");
        let config = HybridSearchConfig {
            db_path: temp.path().join("engram.redb"),
            use_vector: false,
            use_reranker: false,
            ..Default::default()
        };
        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let retriever = HierarchicalRetriever::with_config(
            engine,
            HierarchicalRetrieverConfig {
                negative_cache_ttl_secs: 60,
                ..Default::default()
            },
        );

        let first = retriever
            .search_recursive_with_report("missing retrieval target", 5)
            .await
            .expect("first search");
        let second = retriever
            .search_recursive_with_report("missing retrieval target", 5)
            .await
            .expect("second search");

        assert!(first.results.is_empty());
        assert!(second.results.is_empty());
        assert!(second.report.negative_cache_hit);
        assert!(second.report.dos_hardening_triggered);
        assert!(second
            .report
            .degradation_reasons
            .iter()
            .any(|reason| reason == "negative_cache_hit"));
    }

    #[tokio::test]
    async fn recursive_search_reports_query_throttled_when_bucket_is_exhausted() {
        let temp = tempdir().expect("tempdir");
        let config = HybridSearchConfig {
            db_path: temp.path().join("engram.redb"),
            use_vector: false,
            use_reranker: false,
            ..Default::default()
        };
        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let retriever = HierarchicalRetriever::with_config(
            engine,
            HierarchicalRetrieverConfig {
                token_bucket_capacity: 1,
                token_bucket_refill_per_sec: 0.0,
                ..Default::default()
            },
        );

        let outcome = retriever
            .search_recursive_with_report("this query is intentionally long enough to cost more", 5)
            .await
            .expect("search");

        assert!(outcome.results.is_empty());
        assert!(outcome.report.throttled);
        assert!(outcome.report.dos_hardening_triggered);
        assert!(outcome
            .report
            .degradation_reasons
            .iter()
            .any(|reason| reason == "query_throttled"));
    }

    #[tokio::test]
    async fn recursive_search_short_circuits_during_signature_cooldown() {
        let temp = tempdir().expect("tempdir");
        let config = HybridSearchConfig {
            db_path: temp.path().join("engram.redb"),
            use_vector: false,
            use_reranker: false,
            ..Default::default()
        };
        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let retriever = HierarchicalRetriever::with_config(
            engine,
            HierarchicalRetrieverConfig {
                token_bucket_capacity: 1,
                token_bucket_refill_per_sec: 0.0,
                signature_cooldown_secs: 60,
                ..Default::default()
            },
        );

        let first = retriever
            .search_recursive_with_report("this query is intentionally long enough to cost more", 5)
            .await
            .expect("first search");
        let second = retriever
            .search_recursive_with_report("this query is intentionally long enough to cost more", 5)
            .await
            .expect("second search");

        assert!(first.report.throttled);
        assert!(second.report.throttled);
        assert!(second.report.signature_cooldown_hit);
        assert!(second
            .report
            .degradation_reasons
            .iter()
            .any(|reason| reason == "query_signature_cooldown"));
    }
}
