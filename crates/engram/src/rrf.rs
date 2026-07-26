//! Reciprocal Rank Fusion (RRF) algorithm for hybrid search
//!
//! Combines multiple ranked lists of search results into a single unified ranking.
//! This implementation supports N-way weighted fusion with stable tie-breaking.
//!
//! RRF Formula:
//! score(d) = Σ (weight / (k + rank(d) + 1))
//! Where rank(d) is 0-indexed position in the result list.

use std::collections::HashMap;
use tracing::{debug, trace, warn};

/// RRF configuration parameters
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RrfConfig {
    /// RRF constant (k, typically 60) used to smooth the ranking impact.
    pub k: usize,
    /// Default weight for BM25 results
    pub bm25_weight: f64,
    /// Default weight for Vector results
    pub vector_weight: f64,
}

impl Default for RrfConfig {
    fn default() -> Self {
        Self {
            k: 60,
            bm25_weight: 0.4,
            vector_weight: 0.6,
        }
    }
}

impl RrfConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.k == 0 {
            return Err("RRF constant 'k' must be greater than 0".into());
        }
        if self.bm25_weight < 0.0 || self.vector_weight < 0.0 {
            return Err("Weights must be non-negative".into());
        }
        Ok(())
    }
}

/// A stream of ranked results from one source
#[derive(Debug, Clone)]
pub struct RankedStream {
    pub name: String,
    pub weight: f64,
    pub results: Vec<(String, f64)>, // (docid, score)
}

/// Combined fusion result
#[derive(Debug, Clone, PartialEq)]
pub struct FusedResult {
    pub docid: String,
    pub rrf_score: f64,
    /// Map of source_name -> (rank, score)
    pub source_metadata: HashMap<String, (usize, f64)>,
}

impl FusedResult {
    pub fn is_hybrid(&self) -> bool {
        self.source_metadata.contains_key("bm25") && self.source_metadata.contains_key("vector")
    }
}

/// The RrfFusion engine
#[derive(Debug, Clone)]
pub struct RrfFusion {
    config: RrfConfig,
}

impl RrfFusion {
    pub fn new() -> Self {
        Self::with_config(RrfConfig::default())
    }

    pub fn with_config(config: RrfConfig) -> Self {
        config.validate().expect("Invalid RRF configuration");
        Self { config }
    }

    /// Perform N-way weighted fusion.
    pub fn fuse(&self, streams: Vec<RankedStream>) -> Vec<FusedResult> {
        let start = std::time::Instant::now();
        let total_input_size: usize = streams.iter().map(|s| s.results.len()).sum();

        let mut builder_map: HashMap<String, FusedResultBuilder> =
            HashMap::with_capacity(total_input_size / 2);

        for stream in streams {
            for (rank, (docid, score)) in stream.results.into_iter().enumerate() {
                // RRF score component for this item
                let rrf_contribution = stream.weight / (self.config.k + rank + 1) as f64;

                let builder = builder_map
                    .entry(docid.clone())
                    .or_insert_with(|| FusedResultBuilder::new(docid));

                if builder.source_metadata.contains_key(&stream.name) {
                    warn!(
                        "Stream '{}' contains duplicate docid '{}'",
                        stream.name, builder.docid
                    );
                    continue;
                }

                builder.rrf_score += rrf_contribution;
                builder
                    .source_metadata
                    .insert(stream.name.clone(), (rank, score));
            }
        }

        let mut results: Vec<FusedResult> = builder_map.into_values().map(|b| b.build()).collect();

        // Stable Tie-breaking Strategy:
        // 1. Higher RRF Score (primary)
        // 2. Hybrid Agreement (prefer docs found by both BM25 & Vector)
        // 3. Document ID Lexicographical (deterministic fallback)
        results.sort_by(|a, b| {
            b.rrf_score
                .partial_cmp(&a.rrf_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    let a_hybrid = a.is_hybrid() as u8;
                    let b_hybrid = b.is_hybrid() as u8;
                    b_hybrid.cmp(&a_hybrid)
                })
                .then_with(|| a.docid.cmp(&b.docid))
        });

        debug!(
            "Fused {} results in {:.2}ms (total unique: {})",
            results.len(),
            start.elapsed().as_secs_f64() * 1000.0,
            results.len()
        );

        results
    }

    /// Helper for standard 2-way hybrid search
    pub fn fuse_hybrid(
        &self,
        bm25: &[(String, f64)],
        vector: &[(String, f64)],
        bm25_weight: f64,
        vector_weight: f64,
    ) -> Vec<FusedResult> {
        let streams = vec![
            RankedStream {
                name: "bm25".into(),
                weight: bm25_weight,
                results: bm25.to_vec(),
            },
            RankedStream {
                name: "vector".into(),
                weight: vector_weight,
                results: vector.to_vec(),
            },
        ];
        self.fuse(streams)
    }
}

struct FusedResultBuilder {
    docid: String,
    rrf_score: f64,
    source_metadata: HashMap<String, (usize, f64)>,
}

impl FusedResultBuilder {
    fn new(docid: String) -> Self {
        Self {
            docid,
            rrf_score: 0.0,
            source_metadata: HashMap::new(),
        }
    }

    fn build(self) -> FusedResult {
        FusedResult {
            docid: self.docid,
            rrf_score: self.rrf_score,
            source_metadata: self.source_metadata,
        }
    }
}

impl Default for RrfFusion {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hybrid_agreement_tie_break() {
        let fusion = RrfFusion::new();

        // Two docs with identical RRF scores
        // doc_a: Rank 0 in Stream 1 (1/61)
        // doc_b: Rank 1 in Stream 1 + Rank 1 in Stream 2 (1/62 + 1/62 ≈ 1/31) - wait, let's make them equal

        // Let's use k=1 for easy math
        let config = RrfConfig {
            k: 1,
            bm25_weight: 1.0,
            vector_weight: 1.0,
        };
        let fusion = RrfFusion::with_config(config);

        // doc_single: Rank 0 (1/(1+0+1) = 0.5)
        // doc_hybrid: Rank 2 (1/(1+2+1) = 0.25) + Rank 2 (0.25) = 0.5
        let s1 = RankedStream {
            name: "bm25".into(),
            weight: 1.0,
            results: vec![
                ("single".into(), 10.0),
                ("dummy".into(), 5.0),
                ("hybrid".into(), 3.0),
            ],
        };
        let s2 = RankedStream {
            name: "vector".into(),
            weight: 1.0,
            results: vec![
                ("dummy_v".into(), 0.9),
                ("dummy_v2".into(), 0.8),
                ("hybrid".into(), 0.7),
            ],
        };

        let out = fusion.fuse(vec![s1, s2]);
        let hybrid_res = out.iter().find(|r| r.docid == "hybrid").unwrap();
        let single_res = out.iter().find(|r| r.docid == "single").unwrap();

        // Scores are both 0.5
        assert_eq!(hybrid_res.rrf_score, 0.5);
        assert_eq!(single_res.rrf_score, 0.5);

        // Hybrid should be first
        assert_eq!(out[0].docid, "hybrid");
    }
}
