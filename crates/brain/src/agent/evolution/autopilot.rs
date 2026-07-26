//! Phase 16.3: Cognitive Autopilot.
//!
//! Tracks user behavior patterns and proactively adjusts agent strategy.
//! Includes speculative pre-warming of memory tiers based on intent prediction.

use crate::error::Result;
use benshu_inference::QuantLevel;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Reasoning depths for the System 1 (Fast) vs System 2 (Slow) architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ReasoningDepth {
    /// System 1: Instinctive, low-latency, avoids complex tool chains and deep CoT.
    System1,
    /// System 2: Analytical, high-latency, full reasoning logs and recursive tool execution.
    System2,
}

/// Represents a single observation of user interaction
#[derive(Debug, Clone)]
pub struct QueryObservation {
    pub timestamp: DateTime<Utc>,
    pub intent_label: String,
    pub latency_ms: f32,
    pub success: bool,
}

/// Tracks and analyzes query patterns to predict "hot" contexts
pub struct BehaviorTracker {
    observations: RwLock<Vec<QueryObservation>>,
    max_history: usize,
}

impl BehaviorTracker {
    pub fn new(max_history: usize) -> Self {
        Self {
            observations: RwLock::new(Vec::with_capacity(max_history)),
            max_history,
        }
    }

    pub async fn record(&self, obs: QueryObservation) {
        let mut history = self.observations.write().await;
        if history.len() >= self.max_history {
            history.remove(0);
        }
        history.push(obs);
    }

    /// Predict the likely next intent or "hot" collection based on frequency
    pub async fn predict_hot_intent(&self) -> Option<String> {
        let history = self.observations.read().await;
        if history.is_empty() {
            return None;
        }

        let mut counts = HashMap::new();
        for obs in history.iter().rev().take(5) {
            *counts.entry(&obs.intent_label).or_insert(0) += 1;
        }

        counts
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(label, _)| label.clone())
    }
}

pub struct Autopilot {
    tracker: Arc<BehaviorTracker>,
    memory: Arc<dyn crate::agent::memory::Memory>,
}

impl Autopilot {
    pub fn new(memory: Arc<dyn crate::agent::memory::Memory>) -> Self {
        Self {
            tracker: Arc::new(BehaviorTracker::new(100)),
            memory,
        }
    }

    pub fn tracker(&self) -> Arc<BehaviorTracker> {
        Arc::clone(&self.tracker)
    }

    /// Proactively "pre-warm" memory if a pattern is detected.
    /// E.g., if the user is asking many "code" queries, move code vectors to Warm tier.
    pub async fn perform_prewarming(&self) -> Result<()> {
        if let Some(intent) = self.tracker.predict_hot_intent().await {
            info!(
                "Autopilot: Predicting hot-intent '{}'. Initiating tier-promotion...",
                intent
            );

            let collections = match intent.as_str() {
                "code" => vec!["crates", "src", "docs"],
                "memory" => vec!["experiences", "chats"],
                _ => vec![],
            };

            for col in collections {
                // Production logic: Send promotion signal to Memory Tiering System
                // This moves Background/Cold vectors of these collections into Warm (RAM)
                if let Err(e) = self.memory.promote_vectors(col, QuantLevel::Warm).await {
                    tracing::warn!("Autopilot: Failed to promote vectors for {}: {}", col, e);
                }
            }
        }
        Ok(())
    }

    /// Determines the optimal reasoning depth based on current Cognitive Tension (Latency).
    /// If average latency exceeds 100ms, it forces System 1 for subsequent simple tasks to regain fluidity.
    pub async fn get_optimized_depth(&self) -> ReasoningDepth {
        let history = self.tracker.observations.read().await;
        if history.len() < 3 {
            return ReasoningDepth::System2; // Default to System 2 for new sessions
        }

        let avg_latency = history
            .iter()
            .rev()
            .take(5)
            .map(|o| o.latency_ms)
            .sum::<f32>()
            / 5.0;

        if avg_latency > 100.0 {
            debug!(
                "Cognitive Tension high ({}ms). Switching to System 1 (Fast Thought).",
                avg_latency
            );
            ReasoningDepth::System1
        } else {
            ReasoningDepth::System2
        }
    }

    /// Detect "Cognitive Tension" - if search latency is spiking, it suggests a bottleneck
    pub async fn detect_tension(&self) -> f32 {
        let history = self.tracker.observations.read().await;
        if history.is_empty() {
            return 0.0;
        }
        history
            .iter()
            .rev()
            .take(5)
            .map(|o| o.latency_ms)
            .sum::<f32>()
            / 5.0
    }
}
