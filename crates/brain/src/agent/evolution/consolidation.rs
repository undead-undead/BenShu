//! Phase 12-C: Sleep-consolidation mechanism.
//!
//! During low-activity periods, reviews UNVERIFIED memory entries
//! and marks them as VERIFIED, PRUNED, or CONFLICT.

use std::sync::Arc;

use crate::agent::memory::{
    Fact, FactProtection, FactReviewPayload, FactReviewResolution, FactReviewResolutionOutcome,
    FactStatus, Memory,
};

use crate::agent::evolution::auditor::{AuditResult, Auditor, ChangeType};
use benshu_infra::traits::memory::{EventLevel, MemoryEvent};
use benshu_infra::traits::resource::ThrottleLevel;

/// Status of a consolidation run
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConsolidationReport {
    pub entries_reviewed: usize,
    pub entries_verified: usize,
    pub entries_pruned: usize,
    pub entries_conflicted: usize,
    pub pending_reviews_reviewed: usize,
    pub pending_reviews_verified: usize,
    pub pending_reviews_pruned: usize,
    pub pending_reviews_retained: usize,
    pub batches_processed: usize,
    pub pending_backlog_before: usize,
    pub pending_backlog_after: usize,
    pub backlog_drained: bool,
    pub persistence_failures: usize,
    pub experiences_pruned: usize,
    pub anti_patterns_pruned: usize,
    pub redundant_memories_pruned: usize,
    pub sovereignty_violations_neutralized: usize,
    pub conflicts_resolved: usize,
    pub decay_candidates: usize,
    pub decay_skipped_protected: usize,
    pub pruned_by_decay: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default)]
struct BacklogSnapshot {
    total_pending: usize,
    review_backlog: usize,
    high_priority_pending: usize,
    oldest_pending_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Copy)]
struct ReviewBudget {
    batch_size: usize,
    max_batches: usize,
    max_estimated_tokens: usize,
    max_latency: std::time::Duration,
}

#[derive(Debug, Default, Clone, Copy)]
struct ReviewBudgetUsage {
    estimated_tokens_consumed: usize,
}

/// Consolidates memory during sleep/maintenance periods.
///
/// Queries unverified memory entries, evaluates their quality,
/// and marks them accordingly. Uses an independent Auditor (LLM-based)
/// for assessment to ensure memory quality and safety.
pub struct SleepConsolidator {
    memory: Arc<dyn Memory>,
    auditor: Arc<Auditor>,
    evolution: Option<Arc<crate::agent::evolution::evolution_manager::EvolutionManager>>,
    /// Max entries to process per consolidation run
    batch_size: usize,
    /// Max pending-memory batches to process during a single sleep cycle
    max_batches_per_run: usize,
    /// Max estimated audit tokens to spend during a single consolidation cycle
    max_estimated_tokens_per_cycle: usize,
    /// Max wall-clock latency to spend on review work during a single consolidation cycle
    max_latency_per_cycle: std::time::Duration,
    /// Threshold for pruning low-utility cognitive assets
    prune_threshold: f32,
}

impl SleepConsolidator {
    fn is_high_priority_pending(fact: &Fact) -> bool {
        matches!(
            fact.protection,
            FactProtection::Pinned | FactProtection::Protected | FactProtection::CoreIdentity
        ) || fact.importance >= 0.8
            || matches!(
                fact.category.to_lowercase().as_str(),
                "identity" | "medical" | "safety" | "security" | "allergy"
            )
    }

    fn throttle_label(throttle: ThrottleLevel) -> &'static str {
        match throttle {
            ThrottleLevel::High => "high",
            ThrottleLevel::Medium => "medium",
            ThrottleLevel::Low => "low",
        }
    }

    async fn current_review_throttle(&self) -> ThrottleLevel {
        match &self.evolution {
            Some(evolution) => evolution.current_background_throttle().await,
            None => ThrottleLevel::High,
        }
    }

    fn effective_review_budget(&self, throttle: ThrottleLevel) -> ReviewBudget {
        match throttle {
            ThrottleLevel::High => ReviewBudget {
                batch_size: self.batch_size.max(1),
                max_batches: self.max_batches_per_run.max(1),
                max_estimated_tokens: self.max_estimated_tokens_per_cycle.max(1),
                max_latency: self.max_latency_per_cycle,
            },
            ThrottleLevel::Medium => ReviewBudget {
                batch_size: (self.batch_size / 2).max(1),
                max_batches: (self.max_batches_per_run / 2).max(1),
                max_estimated_tokens: (self.max_estimated_tokens_per_cycle / 2).max(128),
                max_latency: self
                    .max_latency_per_cycle
                    .div_f32(2.0)
                    .max(std::time::Duration::from_millis(250)),
            },
            ThrottleLevel::Low => ReviewBudget {
                batch_size: (self.batch_size / 4).max(1),
                max_batches: 1,
                max_estimated_tokens: (self.max_estimated_tokens_per_cycle / 4).max(128),
                max_latency: self
                    .max_latency_per_cycle
                    .div_f32(4.0)
                    .max(std::time::Duration::from_millis(100)),
            },
        }
    }

    fn estimated_audit_tokens(content: &str) -> usize {
        let base = (content.chars().count() / 4).max(1);
        base + 128
    }

    fn can_spend_budget(
        budget: ReviewBudget,
        usage: &ReviewBudgetUsage,
        next_estimated_tokens: usize,
        processed_any: bool,
        cycle_started_at: std::time::Instant,
    ) -> bool {
        if cycle_started_at.elapsed() >= budget.max_latency {
            return false;
        }

        if processed_any
            && usage
                .estimated_tokens_consumed
                .saturating_add(next_estimated_tokens)
                > budget.max_estimated_tokens
        {
            return false;
        }

        true
    }

    fn champion_score(fact: &Fact, newest_at: chrono::DateTime<chrono::Utc>) -> f32 {
        let age_hours = newest_at
            .signed_duration_since(fact.updated_at)
            .num_hours()
            .max(0) as f32;
        let recency_score = (1.0 - (age_hours / (24.0 * 30.0))).clamp(0.0, 1.0);
        let status_bonus = match fact.status {
            FactStatus::Verified => 0.25,
            FactStatus::PendingReview => 0.10,
            FactStatus::Pending => 0.0,
            FactStatus::Archived => -0.25,
        };

        recency_score * 0.45 + fact.importance * 0.35 + fact.confidence * 0.20 + status_bonus
    }

    fn is_external_memory_candidate(fact: &crate::agent::memory::Fact) -> bool {
        let category = fact.category.to_lowercase();
        let source = fact.source.as_deref().unwrap_or_default().to_lowercase();
        let content = fact.content.to_lowercase();

        let category_marked = matches!(
            category.as_str(),
            "external" | "imported" | "attachment" | "document" | "rag"
        );
        let source_marked = [
            "external:",
            "external/",
            "skill:",
            "skill/",
            "import:",
            "import/",
            "rag:",
            "rag/",
            "attachment:",
            "attachment/",
            "document:",
            "document/",
            "ocr:",
            "vision:",
        ]
        .iter()
        .any(|marker| source.contains(marker));
        let legacy_content_marked =
            content.contains("[external]") || content.contains("summarized by skill");

        category_marked || source_marked || legacy_content_marked
    }

    pub fn new(memory: Arc<dyn Memory>, auditor: Arc<Auditor>) -> Self {
        Self {
            memory,
            auditor,
            evolution: None,
            batch_size: 50,
            max_batches_per_run: 4,
            max_estimated_tokens_per_cycle: 4_096,
            max_latency_per_cycle: std::time::Duration::from_secs(2),
            prune_threshold: -1.0, // Prune if utility drops below -1.0
        }
    }

    pub fn with_evolution(
        mut self,
        evolution: Arc<crate::agent::evolution::evolution_manager::EvolutionManager>,
    ) -> Self {
        self.evolution = Some(evolution);
        self
    }

    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    pub fn with_max_batches_per_run(mut self, count: usize) -> Self {
        self.max_batches_per_run = count.max(1);
        self
    }

    pub fn with_max_estimated_tokens_per_cycle(mut self, tokens: usize) -> Self {
        self.max_estimated_tokens_per_cycle = tokens.max(1);
        self
    }

    pub fn with_max_latency_per_cycle(mut self, latency: std::time::Duration) -> Self {
        self.max_latency_per_cycle = latency;
        self
    }

    pub fn with_prune_threshold(mut self, threshold: f32) -> Self {
        self.prune_threshold = threshold;
        self
    }

    async fn pending_backlog_count(&self, fallback_limit: usize) -> usize {
        if let Ok(facts) = self.memory.retrieve_facts("global", None).await {
            return facts
                .into_iter()
                .filter(|fact| {
                    matches!(fact.status, FactStatus::Pending | FactStatus::PendingReview)
                })
                .count();
        }

        self.memory
            .list_unverified(None, fallback_limit)
            .await
            .map(|facts| facts.len())
            .unwrap_or_default()
    }

    async fn backlog_snapshot(&self, fallback_limit: usize) -> BacklogSnapshot {
        let mut snapshot = BacklogSnapshot::default();
        let facts = match self.memory.retrieve_facts("global", None).await {
            Ok(facts) => facts,
            Err(_) => self
                .memory
                .list_unverified(None, fallback_limit.max(1))
                .await
                .unwrap_or_default(),
        };

        for fact in facts {
            if matches!(fact.status, FactStatus::Pending | FactStatus::PendingReview) {
                snapshot.total_pending += 1;
                if matches!(fact.status, FactStatus::PendingReview) {
                    snapshot.review_backlog += 1;
                }
                if Self::is_high_priority_pending(&fact) {
                    snapshot.high_priority_pending += 1;
                }
                snapshot.oldest_pending_at = match snapshot.oldest_pending_at {
                    Some(oldest) if oldest <= fact.created_at => Some(oldest),
                    _ => Some(fact.created_at),
                };
            }
        }

        snapshot
    }

    async fn persist_backlog_metadata(
        &self,
        pending_backlog_before: usize,
        pending_backlog_after: usize,
        snapshot: &BacklogSnapshot,
        throttle_level: &str,
        budget: ReviewBudget,
        usage: ReviewBudgetUsage,
    ) {
        let writes = [
            (
                "brain.memory.backlog.pending_before_last_cycle",
                pending_backlog_before.to_string(),
            ),
            (
                "brain.memory.backlog.total_pending",
                snapshot
                    .total_pending
                    .max(pending_backlog_after)
                    .to_string(),
            ),
            (
                "brain.memory.backlog.review_backlog",
                snapshot.review_backlog.to_string(),
            ),
            (
                "brain.memory.backlog.high_priority_pending",
                snapshot.high_priority_pending.to_string(),
            ),
            (
                "brain.memory.backlog.oldest_pending_at",
                snapshot
                    .oldest_pending_at
                    .map(|ts| ts.to_rfc3339())
                    .unwrap_or_default(),
            ),
            (
                "brain.memory.backlog.last_throttle_level",
                throttle_level.to_string(),
            ),
            (
                "brain.memory.backlog.last_effective_batch_size",
                budget.batch_size.to_string(),
            ),
            (
                "brain.memory.backlog.last_effective_max_batches",
                budget.max_batches.to_string(),
            ),
            (
                "brain.memory.backlog.last_effective_max_estimated_tokens",
                budget.max_estimated_tokens.to_string(),
            ),
            (
                "brain.memory.backlog.last_effective_max_latency_ms",
                budget.max_latency.as_millis().to_string(),
            ),
            (
                "brain.memory.backlog.last_estimated_tokens_consumed",
                usage.estimated_tokens_consumed.to_string(),
            ),
            (
                "brain.memory.backlog.updated_at",
                chrono::Utc::now().to_rfc3339(),
            ),
        ];

        for (key, value) in writes {
            let _ = self.memory.set_metadata(key, &value).await;
        }
    }

    async fn list_pending_review_facts(&self, max_review_slots: usize) -> Vec<Fact> {
        self.memory
            .retrieve_facts("global", None)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|fact| matches!(fact.status, FactStatus::PendingReview))
            .take(max_review_slots.max(1))
            .collect()
    }

    async fn evaluate_pending_review(
        &self,
        fact: &Fact,
        payload: &FactReviewPayload,
    ) -> FactReviewResolution {
        let mut content = format!(
            "PENDING_REVIEW FACT\nID: {}\nCATEGORY: {}\nCONTENT: {}\n",
            fact.id, fact.category, fact.content
        );
        if let Some(reason) = payload.review_reason.as_deref() {
            content.push_str(&format!("REVIEW_REASON: {}\n", reason));
        }
        if let Some(summary) = payload.challenger_summary.as_deref() {
            content.push_str(&format!("CHALLENGER_SUMMARY: {}\n", summary));
        }
        if let Some(source) = payload.challenger_source.as_deref() {
            content.push_str(&format!("CHALLENGER_SOURCE: {}\n", source));
        }

        match self
            .auditor
            .audit(
                &ChangeType::MemoryPurification {
                    docid: fact.id.clone(),
                },
                &content,
            )
            .await
        {
            AuditResult::Approved => FactReviewResolution {
                outcome: FactReviewResolutionOutcome::Verified,
                resolution_reason: Some("challenger review accepted the fact as valid".to_string()),
                resolution_basis: Some("challenger_re_summary".to_string()),
                resolved_by: Some("sleep_consolidator_challenger".to_string()),
                resolved_at: chrono::Utc::now(),
            },
            AuditResult::Rejected { reason } => FactReviewResolution {
                outcome: FactReviewResolutionOutcome::Pruned,
                resolution_reason: Some(reason),
                resolution_basis: Some("challenger_re_summary".to_string()),
                resolved_by: Some("sleep_consolidator_challenger".to_string()),
                resolved_at: chrono::Utc::now(),
            },
            AuditResult::NeedsReview { summary } => FactReviewResolution {
                outcome: FactReviewResolutionOutcome::PendingReview,
                resolution_reason: Some(summary),
                resolution_basis: Some("challenger_re_summary".to_string()),
                resolved_by: Some("sleep_consolidator_challenger".to_string()),
                resolved_at: chrono::Utc::now(),
            },
        }
    }

    async fn process_pending_reviews(
        &self,
        max_review_slots: usize,
        budget: ReviewBudget,
        usage: &mut ReviewBudgetUsage,
        cycle_started_at: std::time::Instant,
    ) -> anyhow::Result<(usize, usize, usize, usize)> {
        let candidates = self.list_pending_review_facts(max_review_slots).await;
        let mut reviewed = 0usize;
        let mut verified = 0usize;
        let mut pruned = 0usize;
        let mut retained = 0usize;

        for fact in candidates {
            let payload = self
                .memory
                .get_fact_review_payload(&fact.id)
                .await
                .unwrap_or(None)
                .unwrap_or_default();
            let mut token_probe = format!(
                "PENDING_REVIEW FACT\nID: {}\nCATEGORY: {}\nCONTENT: {}\n",
                fact.id, fact.category, fact.content
            );
            if let Some(reason) = payload.review_reason.as_deref() {
                token_probe.push_str(reason);
            }
            if let Some(summary) = payload.challenger_summary.as_deref() {
                token_probe.push_str(summary);
            }
            if let Some(source) = payload.challenger_source.as_deref() {
                token_probe.push_str(source);
            }
            let estimated_tokens = Self::estimated_audit_tokens(&token_probe);
            if !Self::can_spend_budget(
                budget,
                usage,
                estimated_tokens,
                reviewed > 0,
                cycle_started_at,
            ) {
                break;
            }

            reviewed += 1;
            usage.estimated_tokens_consumed = usage
                .estimated_tokens_consumed
                .saturating_add(estimated_tokens);
            let resolution = self.evaluate_pending_review(&fact, &payload).await;
            match resolution.outcome {
                FactReviewResolutionOutcome::Verified => verified += 1,
                FactReviewResolutionOutcome::Pruned => pruned += 1,
                FactReviewResolutionOutcome::PendingReview => retained += 1,
            }
            self.memory
                .resolve_pending_review(&fact.id, resolution)
                .await
                .map_err(|e| {
                    anyhow::anyhow!("failed to resolve pending review {}: {}", fact.id, e)
                })?;
        }

        Ok((reviewed, verified, pruned, retained))
    }

    /// Run a consolidation cycle.
    ///
    /// 1. Fetch unverified entries
    /// 2. Evaluation via Auditor (LLM or Rule-based)
    /// 3. Mark as VERIFIED / PRUNED / CONFLICT
    /// 4. Resolve conflicts between facts (Phase 14.3)
    /// 5. Neutralize sovereignty violations (Phase 14.3)
    /// 6. Prune low-utility cognitive assets
    pub async fn consolidate(&self) -> anyhow::Result<ConsolidationReport> {
        let start = std::time::Instant::now();
        let throttle = self.current_review_throttle().await;
        let throttle_label = Self::throttle_label(throttle).to_string();
        let review_budget = self.effective_review_budget(throttle);
        let pending_backlog_fallback_limit = review_budget
            .batch_size
            .saturating_mul(review_budget.max_batches.max(1))
            .saturating_mul(16)
            .max(review_budget.batch_size);
        let pending_backlog_before = self
            .pending_backlog_count(pending_backlog_fallback_limit)
            .await;
        let mut budget_usage = ReviewBudgetUsage::default();

        self.memory.emit_event(
            MemoryEvent::ReviewBudgetApplied {
                throttle_level: throttle_label.clone(),
                configured_batch_size: self.batch_size,
                configured_max_batches: self.max_batches_per_run,
                configured_max_estimated_tokens: self.max_estimated_tokens_per_cycle,
                configured_max_latency_ms: self.max_latency_per_cycle.as_millis() as u64,
                effective_batch_size: review_budget.batch_size,
                effective_max_batches: review_budget.max_batches,
                effective_max_estimated_tokens: review_budget.max_estimated_tokens,
                effective_max_latency_ms: review_budget.max_latency.as_millis() as u64,
            },
            EventLevel::Info,
        );

        // 1. Memory Purification / Pending Backlog Sweep
        let mut total = 0usize;
        let mut verified = 0usize;
        let mut pruned = 0usize;
        let mut conflicted = 0usize;
        let mut persistence_failures = 0usize;
        let mut batches_processed = 0usize;

        loop {
            if batches_processed >= review_budget.max_batches
                || start.elapsed() >= review_budget.max_latency
            {
                break;
            }

            let pending_only: Vec<_> = self
                .memory
                .list_unverified(None, review_budget.batch_size)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to list unverified entries: {}", e))?
                .into_iter()
                .filter(|fact| matches!(fact.status, FactStatus::Pending))
                .collect();

            if pending_only.is_empty() {
                break;
            }

            batches_processed += 1;

            for msg in &pending_only {
                let estimated_tokens = Self::estimated_audit_tokens(&msg.content);
                if !Self::can_spend_budget(
                    review_budget,
                    &budget_usage,
                    estimated_tokens,
                    budget_usage.estimated_tokens_consumed > 0,
                    start,
                ) {
                    break;
                }

                let text = &msg.content;
                let docid = uuid::Uuid::new_v4().to_string();
                total += 1;
                budget_usage.estimated_tokens_consumed = budget_usage
                    .estimated_tokens_consumed
                    .saturating_add(estimated_tokens);

                let decision = self.evaluate(&docid, text).await;
                match decision {
                    ConsolidationDecision::Verify => {
                        if let Err(err) = self.memory.mark_verified(&msg.id).await {
                            persistence_failures += 1;
                            tracing::warn!(
                                fact_id = %msg.id,
                                error = %err,
                                "Sleep consolidation failed to mark fact as verified"
                            );
                        } else {
                            verified += 1;
                        }
                    }
                    ConsolidationDecision::Prune => {
                        if let Err(err) = self.memory.mark_pruned(&msg.id).await {
                            persistence_failures += 1;
                            tracing::warn!(
                                fact_id = %msg.id,
                                error = %err,
                                "Sleep consolidation failed to mark fact as pruned"
                            );
                        } else {
                            pruned += 1;
                        }
                    }
                    ConsolidationDecision::Conflict { summary } => {
                        if let Err(err) = self
                            .memory
                            .mark_pending_review(&msg.id, Some(summary.as_str()))
                            .await
                        {
                            persistence_failures += 1;
                            tracing::warn!(
                                fact_id = %msg.id,
                                error = %err,
                                "Sleep consolidation failed to mark fact as pending review"
                            );
                        } else {
                            conflicted += 1;
                        }
                    }
                }
            }

            if pending_only.len() < review_budget.batch_size
                || start.elapsed() >= review_budget.max_latency
                || budget_usage.estimated_tokens_consumed >= review_budget.max_estimated_tokens
            {
                break;
            }
        }

        let (
            pending_reviews_reviewed,
            pending_reviews_verified,
            pending_reviews_pruned,
            pending_reviews_retained,
        ) = self
            .process_pending_reviews(
                review_budget
                    .batch_size
                    .saturating_mul(review_budget.max_batches.max(1)),
                review_budget,
                &mut budget_usage,
                start,
            )
            .await?;

        let pending_backlog_after = self
            .pending_backlog_count(pending_backlog_fallback_limit)
            .await;
        let backlog_drained = pending_backlog_after == 0;
        let backlog_snapshot = self.backlog_snapshot(pending_backlog_fallback_limit).await;
        let pending_review_count = backlog_snapshot.review_backlog;

        self.persist_backlog_metadata(
            pending_backlog_before,
            pending_backlog_after,
            &backlog_snapshot,
            &throttle_label,
            review_budget,
            budget_usage,
        )
        .await;

        self.memory.emit_event(
            MemoryEvent::BacklogHealth {
                pending_backlog_before,
                pending_backlog_after,
                pending_review_count,
                high_priority_pending: backlog_snapshot.high_priority_pending,
                oldest_pending_at: backlog_snapshot.oldest_pending_at.map(|ts| ts.to_rfc3339()),
                batches_processed,
                backlog_drained,
                throttle_level: throttle_label.clone(),
            },
            if backlog_drained {
                EventLevel::Info
            } else {
                EventLevel::Warn
            },
        );

        if pending_backlog_after > 0 {
            tracing::warn!(
                pending_backlog_before,
                pending_backlog_after,
                batches_processed,
                batch_size = review_budget.batch_size,
                max_batches_per_run = review_budget.max_batches,
                throttle_level = %throttle_label,
                "Sleep consolidation left pending facts for future cycles"
            );
        }

        // 2. Conflict Resolution (Phase 14.3)
        let resolved_count = self.resolve_conflicts(None).await.unwrap_or(0);

        // 3. Memory Thinning (Phase 14.3)
        let redundant_pruned = 0;

        // 4. Sovereignty Enforcement
        let sovereignty_count = self.enforce_sovereignty(None).await.unwrap_or(0);

        // 5. Cognitive Hygiene
        let (exp_pruned, ap_pruned) = self.prune_cognitive_assets().await?;

        // 6. Utility Aging
        let (decay_candidates, decay_skipped_protected, pruned_by_decay) =
            self.update_utility_heat().await?;

        // 7. Physical Vector Aging (Phase 14.3 Integration)
        // Explicitly trigger engram to perform SIMD-accelerated quantization aging
        let _ = self.memory.age_vectors("global", 7).await;

        let duration_ms = start.elapsed().as_millis() as u64;

        let report = ConsolidationReport {
            entries_reviewed: total,
            entries_verified: verified,
            entries_pruned: pruned,
            entries_conflicted: conflicted,
            pending_reviews_reviewed,
            pending_reviews_verified,
            pending_reviews_pruned,
            pending_reviews_retained,
            batches_processed,
            pending_backlog_before,
            pending_backlog_after,
            backlog_drained,
            persistence_failures,
            experiences_pruned: exp_pruned,
            anti_patterns_pruned: ap_pruned,
            redundant_memories_pruned: redundant_pruned,
            sovereignty_violations_neutralized: sovereignty_count,
            conflicts_resolved: resolved_count,
            decay_candidates,
            decay_skipped_protected,
            pruned_by_decay,
            duration_ms,
        };

        tracing::info!(
            reviewed = total,
            verified = verified,
            pruned = pruned,
            conflicted = conflicted,
            pending_reviews_reviewed = pending_reviews_reviewed,
            pending_reviews_verified = pending_reviews_verified,
            pending_reviews_pruned = pending_reviews_pruned,
            pending_reviews_retained = pending_reviews_retained,
            batches_processed = batches_processed,
            pending_backlog_before = pending_backlog_before,
            pending_backlog_after = pending_backlog_after,
            throttle_level = %throttle_label,
            effective_batch_size = review_budget.batch_size,
            effective_max_batches = review_budget.max_batches,
            effective_max_estimated_tokens = review_budget.max_estimated_tokens,
            effective_max_latency_ms = review_budget.max_latency.as_millis() as u64,
            estimated_tokens_consumed = budget_usage.estimated_tokens_consumed,
            backlog_drained = backlog_drained,
            persistence_failures = persistence_failures,
            exp_pruned = exp_pruned,
            ap_pruned = ap_pruned,
            redundant_pruned = redundant_pruned,
            resolved = resolved_count,
            sovereignty = sovereignty_count,
            decay_candidates = decay_candidates,
            decay_skipped_protected = decay_skipped_protected,
            pruned_by_decay = pruned_by_decay,
            duration_ms = duration_ms,
            "Sleep consolidation complete"
        );

        Ok(report)
    }

    /// Update utility heat (decay for messages and assets)
    async fn update_utility_heat(&self) -> anyhow::Result<(usize, usize, usize)> {
        let now = chrono::Utc::now();
        let mut decay_candidates = 0usize;
        let mut decay_skipped_protected = 0usize;
        let mut pruned_by_decay = 0usize;
        // 1. Decay logic for long-term facts
        let facts = self
            .memory
            .retrieve_facts("global", None)
            .await
            .unwrap_or_default();
        for fact in facts {
            let days_since_access = now.signed_duration_since(fact.updated_at).num_days() as f32;
            if days_since_access > 7.0 {
                decay_candidates += 1;
                match fact.protection {
                    FactProtection::Protected | FactProtection::CoreIdentity => {
                        decay_skipped_protected += 1;
                    }
                    FactProtection::Pinned => {
                        let new_importance = (fact.importance * 0.98).max(0.1);
                        let _ = self
                            .memory
                            .update_fact_importance("global", None, &fact.id, new_importance)
                            .await;
                    }
                    FactProtection::Normal => {
                        let decay = if fact.verified && fact.importance >= 0.8 {
                            0.95f32
                        } else {
                            0.9f32
                        };
                        let new_importance = fact.importance * decay;
                        if new_importance < 0.1 {
                            if self.memory.mark_pruned(&fact.id).await.is_ok() {
                                pruned_by_decay += 1;
                            }
                        } else {
                            let _ = self
                                .memory
                                .update_fact_importance("global", None, &fact.id, new_importance)
                                .await;
                        }
                    }
                }
            }
        }
        Ok((decay_candidates, decay_skipped_protected, pruned_by_decay))
    }

    /// Phase 14.3: Conflict Resolution (Deconfliction)
    /// Merge or update conflicting facts based on recency and confidence.
    async fn resolve_conflicts(&self, agent_id: Option<&str>) -> anyhow::Result<usize> {
        let mut count = 0;
        let facts = self
            .memory
            .retrieve_facts("global", agent_id)
            .await
            .unwrap_or_default();

        let mut categories: std::collections::HashMap<String, Vec<crate::agent::memory::Fact>> =
            std::collections::HashMap::new();
        for fact in facts {
            categories
                .entry(fact.category.clone())
                .or_default()
                .push(fact);
        }

        for (category, mut cat_facts) in categories {
            if cat_facts.len() < 2 {
                continue;
            }

            let newest_at = cat_facts
                .iter()
                .map(|fact| fact.updated_at)
                .max()
                .unwrap_or_else(chrono::Utc::now);

            cat_facts.sort_by(|a, b| {
                let left = Self::champion_score(a, newest_at);
                let right = Self::champion_score(b, newest_at);
                right
                    .partial_cmp(&left)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.updated_at.cmp(&a.updated_at))
            });

            // Check for semantic conflicts via Auditor
            let content = serde_json::to_string(&cat_facts)?;
            let change = ChangeType::MemoryDeconfliction {
                category: category.clone(),
            };
            let result = self.auditor.audit(&change, &content).await;

            if let AuditResult::Rejected { reason } = result {
                // If the auditor confirms a conflict (e.g., "Apple vs Xiaomi"),
                // we keep the most recent one (the first in sorted list) and prune others.
                tracing::warn!("Consolidator[Deconfliction]: Conflict in category '{}': {}. Resolving to latest.", category, reason);

                // Phase 14-H: Reporting error to trigger Potential Auto-Rollback of compromised evolution
                if let Some(em) = &self.evolution {
                    em.report_error(&format!(
                        "Memory Conflict in category '{}': {}",
                        category, reason
                    ));
                }

                let champion_id = &cat_facts[0].id;
                tracing::info!(
                    "Consolidator[Deconfliction]: Champion '{}' selected for category '{}'",
                    champion_id,
                    category
                );
                for loser in &cat_facts[1..] {
                    let _ = self.memory.mark_pruned(&loser.id).await;
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Phase 14.3: Sovereignty Enforcement
    /// Detect and remove remnants of external memory skills that mimic internal facts.
    async fn enforce_sovereignty(&self, agent_id: Option<&str>) -> anyhow::Result<usize> {
        let mut count = 0;
        let facts = self
            .memory
            .retrieve_facts("global", agent_id)
            .await
            .unwrap_or_default();

        for fact in facts {
            if Self::is_external_memory_candidate(&fact) {
                let change = ChangeType::SovereigntyAudit {
                    source: fact.source.clone().unwrap_or_default(),
                };
                let result = self.auditor.audit(&change, &fact.content).await;

                if matches!(result, AuditResult::Rejected { .. }) {
                    tracing::info!("Consolidator[Sovereignty]: Neutralizing external cognitive interference in fact '{}'", fact.id);
                    let _ = self.memory.mark_pruned(&fact.id).await;
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Prune low-utility cognitive assets (Experiences and Anti-Patterns)
    async fn prune_cognitive_assets(&self) -> anyhow::Result<(usize, usize)> {
        let mut exp_pruned = 0;
        let mut ap_pruned = 0;

        // 1. Prune Experiences
        // Note: Using search_experiences with empty query as a fallback/listing mechanism
        if let Ok(exps) = self.memory.search_experiences("", 100).await {
            for exp in exps {
                let id = exp.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                let utility = exp
                    .get("utility_score")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                if !id.is_empty() && utility < self.prune_threshold as f64 {
                    tracing::info!(
                        "Consolidator: Pruning low-utility experience '{}' (Utility: {:.2})",
                        id,
                        utility
                    );
                    let _ = self.memory.delete_experience(id).await;
                    exp_pruned += 1;
                }
            }
        }

        // 2. Prune Anti-Patterns
        if let Ok(aps) = self.memory.search_anti_patterns("", 100).await {
            for ap in aps {
                let id = ap.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                let utility = ap
                    .get("utility_score")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                if !id.is_empty() && utility < self.prune_threshold as f64 {
                    tracing::info!(
                        "Consolidator: Pruning low-utility anti-pattern '{}' (Utility: {:.2})",
                        id,
                        utility
                    );
                    let _ = self.memory.delete_anti_pattern(id).await;
                    ap_pruned += 1;
                }
            }
        }

        Ok((exp_pruned, ap_pruned))
    }

    /// Evaluate a memory entry using the auditor
    async fn evaluate(&self, docid: &str, content: &str) -> ConsolidationDecision {
        let change = ChangeType::MemoryPurification {
            docid: docid.to_string(),
        };
        let result = self.auditor.audit(&change, content).await;

        match result {
            AuditResult::Approved => ConsolidationDecision::Verify,
            AuditResult::Rejected { .. } => ConsolidationDecision::Prune,
            AuditResult::NeedsReview { summary } => ConsolidationDecision::Conflict { summary },
        }
    }
}

#[derive(Debug)]
enum ConsolidationDecision {
    Verify,
    Prune,
    Conflict { summary: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::memory::{Fact, FactStatus, InMemoryMemory};
    use benshu_infra::traits::memory::{EventLevel, MemoryEmitter, MemoryEvent};
    use benshu_infra::traits::resource::{HostResources, ResourceSensor};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingEmitter {
        events: Mutex<Vec<(MemoryEvent, EventLevel)>>,
    }

    impl RecordingEmitter {
        fn snapshot(&self) -> Vec<(MemoryEvent, EventLevel)> {
            self.events.lock().expect("lock emitter").clone()
        }
    }

    impl MemoryEmitter for RecordingEmitter {
        fn emit(&self, event: MemoryEvent, level: EventLevel) {
            self.events
                .lock()
                .expect("lock emitter")
                .push((event, level));
        }
    }

    struct FixedSensor {
        resources: HostResources,
    }

    impl ResourceSensor for FixedSensor {
        fn check_resources(&mut self, _detailed: bool) -> HostResources {
            self.resources.clone()
        }
    }

    #[tokio::test]
    async fn test_consolidation_empty() {
        let memory = Arc::new(InMemoryMemory::new());
        let provider = Arc::new(crate::agent::provider::MockProvider::new("APPROVED"));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let consolidator = SleepConsolidator::new(memory, auditor);
        let report = consolidator.consolidate().await.unwrap();
        assert_eq!(report.entries_reviewed, 0);
    }

    #[tokio::test]
    async fn consolidation_drains_pending_backlog_across_multiple_batches() {
        let memory = Arc::new(InMemoryMemory::new());
        for idx in 0..5 {
            memory
                .store_fact(
                    "global",
                    None,
                    Fact {
                        id: format!("pending-{idx}"),
                        category: "prefs".to_string(),
                        content: format!("safe fact {idx}"),
                        importance: 0.7,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                        verified: false,
                        source: Some("test".to_string()),
                        confidence: 0.8,
                        relations: Vec::new(),
                        semantic_hash: Some(format!("pending-{idx}")),
                        status: FactStatus::Pending,
                        protection: FactProtection::Normal,
                    },
                )
                .await
                .expect("fact stored");
        }

        let provider = Arc::new(crate::agent::provider::MockProvider::new(
            r#"{"decision":"APPROVED","reason":"ok"}"#,
        ));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let consolidator = SleepConsolidator::new(memory.clone(), auditor)
            .with_batch_size(2)
            .with_max_batches_per_run(3);

        let report = consolidator.consolidate().await.expect("consolidate");
        assert_eq!(report.pending_backlog_before, 5);
        assert_eq!(report.entries_reviewed, 5);
        assert_eq!(report.entries_verified, 5);
        assert_eq!(report.batches_processed, 3);
        assert_eq!(report.pending_backlog_after, 0);
        assert!(report.backlog_drained);
    }

    #[tokio::test]
    async fn consolidation_emits_backlog_health_and_applies_low_throttle_budget() {
        let memory = Arc::new(InMemoryMemory::new());
        let emitter = Arc::new(RecordingEmitter::default());
        memory.set_emitter(emitter.clone());

        for idx in 0..3 {
            memory
                .store_fact(
                    "global",
                    None,
                    Fact {
                        id: format!("budgeted-{idx}"),
                        category: "prefs".to_string(),
                        content: format!("safe fact {idx}"),
                        importance: 0.7,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                        verified: false,
                        source: Some("test".to_string()),
                        confidence: 0.8,
                        relations: Vec::new(),
                        semantic_hash: Some(format!("budgeted-{idx}")),
                        status: FactStatus::Pending,
                        protection: FactProtection::Normal,
                    },
                )
                .await
                .expect("fact stored");
        }

        let provider = Arc::new(crate::agent::provider::MockProvider::new(
            r#"{"decision":"APPROVED","reason":"ok"}"#,
        ));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let evolution = Arc::new(
            crate::agent::evolution::evolution_manager::EvolutionManager::new(
                auditor.clone(),
                std::env::temp_dir(),
            ),
        );
        let low_pressure_sensor = Arc::new(tokio::sync::Mutex::new(FixedSensor {
            resources: HostResources {
                cpu_usage: 98.0,
                free_memory_pct: 10.0,
                ..HostResources::default()
            },
        }));
        evolution.set_sensor(low_pressure_sensor);

        let consolidator = SleepConsolidator::new(memory.clone(), auditor)
            .with_evolution(evolution)
            .with_batch_size(4)
            .with_max_batches_per_run(4);

        let report = consolidator.consolidate().await.expect("consolidate");
        assert_eq!(report.entries_reviewed, 1);
        assert_eq!(report.entries_verified, 1);
        assert_eq!(report.batches_processed, 1);
        assert_eq!(report.pending_backlog_before, 3);
        assert_eq!(report.pending_backlog_after, 2);
        assert!(!report.backlog_drained);
        assert_eq!(
            memory
                .get_metadata("brain.memory.backlog.total_pending")
                .await
                .expect("total_pending metadata")
                .as_deref(),
            Some("2")
        );
        assert_eq!(
            memory
                .get_metadata("brain.memory.backlog.review_backlog")
                .await
                .expect("review_backlog metadata")
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            memory
                .get_metadata("brain.memory.backlog.high_priority_pending")
                .await
                .expect("high_priority metadata")
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            memory
                .get_metadata("brain.memory.backlog.last_throttle_level")
                .await
                .expect("throttle metadata")
                .as_deref(),
            Some("low")
        );
        assert!(memory
            .get_metadata("brain.memory.backlog.oldest_pending_at")
            .await
            .expect("oldest_pending metadata")
            .is_some());

        let events = emitter.snapshot();
        assert!(events.iter().any(|(event, level)| {
            matches!(
                event,
                MemoryEvent::ReviewBudgetApplied {
                    throttle_level,
                    configured_batch_size: 4,
                    configured_max_batches: 4,
                    configured_max_estimated_tokens: 4096,
                    configured_max_latency_ms: 2000,
                    effective_batch_size: 1,
                    effective_max_batches: 1,
                    effective_max_estimated_tokens: 1024,
                    effective_max_latency_ms: 500,
                } if throttle_level == "low"
            ) && *level == EventLevel::Info
        }));
        assert!(events.iter().any(|(event, level)| {
            matches!(
                event,
                MemoryEvent::BacklogHealth {
                    pending_backlog_before: 3,
                    pending_backlog_after: 2,
                    high_priority_pending: 0,
                    oldest_pending_at: Some(_),
                    batches_processed: 1,
                    backlog_drained: false,
                    throttle_level,
                    ..
                } if throttle_level == "low"
            ) && *level == EventLevel::Warn
        }));
    }

    #[tokio::test]
    async fn consolidation_respects_estimated_token_budget() {
        let memory = Arc::new(InMemoryMemory::new());
        let long_content = "x".repeat(1400);
        for idx in 0..3 {
            memory
                .store_fact(
                    "global",
                    None,
                    Fact {
                        id: format!("token-budget-{idx}"),
                        category: "prefs".to_string(),
                        content: long_content.clone(),
                        importance: 0.7,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                        verified: false,
                        source: Some("test".to_string()),
                        confidence: 0.8,
                        relations: Vec::new(),
                        semantic_hash: Some(format!("token-budget-{idx}")),
                        status: FactStatus::Pending,
                        protection: FactProtection::Normal,
                    },
                )
                .await
                .expect("fact stored");
        }

        let provider = Arc::new(crate::agent::provider::MockProvider::new(
            r#"{"decision":"APPROVED","reason":"ok"}"#,
        ));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let consolidator = SleepConsolidator::new(memory.clone(), auditor)
            .with_batch_size(4)
            .with_max_batches_per_run(4)
            .with_max_estimated_tokens_per_cycle(600)
            .with_max_latency_per_cycle(std::time::Duration::from_secs(5));

        let report = consolidator.consolidate().await.expect("consolidate");
        assert_eq!(report.entries_reviewed, 1);
        assert_eq!(report.entries_verified, 1);
        assert_eq!(report.pending_backlog_before, 3);
        assert_eq!(report.pending_backlog_after, 2);
        assert_eq!(
            memory
                .get_metadata("brain.memory.backlog.last_effective_max_estimated_tokens")
                .await
                .expect("max estimated tokens metadata")
                .as_deref(),
            Some("600")
        );
        let consumed = memory
            .get_metadata("brain.memory.backlog.last_estimated_tokens_consumed")
            .await
            .expect("consumed tokens metadata")
            .expect("consumed tokens present")
            .parse::<usize>()
            .expect("consumed tokens numeric");
        assert!(consumed >= 400);
        assert!(consumed <= 600);
    }

    #[tokio::test]
    async fn consolidation_respects_zero_latency_budget() {
        let memory = Arc::new(InMemoryMemory::new());
        memory
            .store_fact(
                "global",
                None,
                Fact {
                    id: "latency-budget".to_string(),
                    category: "prefs".to_string(),
                    content: "safe fact".to_string(),
                    importance: 0.7,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    verified: false,
                    source: Some("test".to_string()),
                    confidence: 0.8,
                    relations: Vec::new(),
                    semantic_hash: Some("latency-budget".to_string()),
                    status: FactStatus::Pending,
                    protection: FactProtection::Normal,
                },
            )
            .await
            .expect("fact stored");

        let provider = Arc::new(crate::agent::provider::MockProvider::new(
            r#"{"decision":"APPROVED","reason":"ok"}"#,
        ));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let consolidator = SleepConsolidator::new(memory.clone(), auditor)
            .with_batch_size(4)
            .with_max_batches_per_run(4)
            .with_max_estimated_tokens_per_cycle(4096)
            .with_max_latency_per_cycle(std::time::Duration::ZERO);

        let report = consolidator.consolidate().await.expect("consolidate");
        assert_eq!(report.entries_reviewed, 0);
        assert_eq!(report.entries_verified, 0);
        assert_eq!(report.pending_backlog_before, 1);
        assert_eq!(report.pending_backlog_after, 1);
        assert_eq!(report.batches_processed, 0);
        assert_eq!(
            memory
                .get_metadata("brain.memory.backlog.last_effective_max_latency_ms")
                .await
                .expect("max latency metadata")
                .as_deref(),
            Some("0")
        );
    }

    #[tokio::test]
    async fn consolidation_marks_needs_review_facts_as_pending_review() {
        let memory = Arc::new(InMemoryMemory::new());
        memory
            .store_fact(
                "global",
                None,
                Fact {
                    id: "review-me".to_string(),
                    category: "prefs".to_string(),
                    content: "This fact needs a second pass".to_string(),
                    importance: 0.7,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    verified: false,
                    source: Some("test".to_string()),
                    confidence: 0.6,
                    relations: Vec::new(),
                    semantic_hash: Some("review-me".to_string()),
                    status: FactStatus::Pending,
                    protection: FactProtection::Normal,
                },
            )
            .await
            .expect("fact stored");

        let provider = Arc::new(crate::agent::provider::MockProvider::new(
            r#"{"decision":"NEEDS_REVIEW","reason":"conflicts with prior summary"}"#,
        ));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let consolidator = SleepConsolidator::new(memory.clone(), auditor);

        let report = consolidator.consolidate().await.expect("consolidate");
        assert_eq!(report.entries_conflicted, 1);

        let facts = memory
            .retrieve_facts("global", None)
            .await
            .expect("facts retrieved");
        let fact = facts
            .into_iter()
            .find(|fact| fact.id == "review-me")
            .expect("fact retained");
        assert!(matches!(fact.status, FactStatus::PendingReview));
        assert!(!fact.verified);
    }

    #[tokio::test]
    async fn consolidation_challenger_resolves_pending_review_fact() {
        let memory = Arc::new(InMemoryMemory::new());
        memory
            .store_fact(
                "global",
                None,
                Fact {
                    id: "challenger-review".to_string(),
                    category: "prefs".to_string(),
                    content: "The user prefers Rust for low-level systems work.".to_string(),
                    importance: 0.8,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    verified: false,
                    source: Some("test".to_string()),
                    confidence: 0.7,
                    relations: Vec::new(),
                    semantic_hash: Some("challenger-review".to_string()),
                    status: FactStatus::PendingReview,
                    protection: FactProtection::Normal,
                },
            )
            .await
            .expect("store fact");
        memory
            .mark_pending_review("challenger-review", Some("summary drift detected"))
            .await
            .expect("mark pending review");

        let provider = Arc::new(crate::agent::provider::MockProvider::new(
            r#"{"decision":"APPROVED","reason":"re-summary looks consistent"}"#,
        ));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let consolidator = SleepConsolidator::new(memory.clone(), auditor);

        let report = consolidator.consolidate().await.expect("consolidate");
        assert_eq!(report.pending_reviews_reviewed, 1);
        assert_eq!(report.pending_reviews_verified, 1);
        assert_eq!(report.pending_reviews_pruned, 0);
        assert_eq!(report.pending_reviews_retained, 0);

        let fact = memory
            .retrieve_facts("global", None)
            .await
            .expect("facts")
            .into_iter()
            .find(|fact| fact.id == "challenger-review")
            .expect("fact exists");
        assert!(matches!(fact.status, FactStatus::Verified));
        assert!(fact.verified);

        let payload = memory
            .get_fact_review_payload("challenger-review")
            .await
            .expect("payload retrieval")
            .expect("payload exists");
        let resolution = payload.resolution.expect("resolution exists");
        assert!(matches!(
            resolution.outcome,
            FactReviewResolutionOutcome::Verified
        ));
    }

    #[tokio::test]
    async fn consolidation_skips_decay_for_protected_fact() {
        let memory = Arc::new(InMemoryMemory::new());
        memory
            .store_fact(
                "global",
                None,
                Fact {
                    id: "protected-fact".to_string(),
                    category: "identity".to_string(),
                    content: "The user is severely allergic to penicillin.".to_string(),
                    importance: 0.05,
                    created_at: chrono::Utc::now() - chrono::Duration::days(30),
                    updated_at: chrono::Utc::now() - chrono::Duration::days(14),
                    verified: true,
                    source: Some("test".to_string()),
                    confidence: 0.95,
                    relations: Vec::new(),
                    semantic_hash: Some("protected-fact".to_string()),
                    status: FactStatus::Verified,
                    protection: FactProtection::Protected,
                },
            )
            .await
            .expect("store fact");

        let provider = Arc::new(crate::agent::provider::MockProvider::new("APPROVED"));
        let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
        let consolidator = SleepConsolidator::new(memory.clone(), auditor);

        let report = consolidator.consolidate().await.expect("consolidate");
        assert_eq!(report.decay_candidates, 1);
        assert_eq!(report.decay_skipped_protected, 1);
        assert_eq!(report.pruned_by_decay, 0);

        let fact = memory
            .retrieve_facts("global", None)
            .await
            .expect("facts")
            .into_iter()
            .find(|fact| fact.id == "protected-fact")
            .expect("fact retained");
        assert!(matches!(fact.protection, FactProtection::Protected));
        assert!((fact.importance - 0.05).abs() < f32::EPSILON);
    }
}
