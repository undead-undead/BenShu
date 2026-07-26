use crate::hybrid_search::HybridSearchStats;
use std::collections::HashMap;

pub(crate) const META_FACT_SOURCE: &str = "fact_source";
pub(crate) const META_FACT_CONTRACT_VERSION: &str = "fact_contract_version";
pub(crate) const META_FACT_POLICY_OWNER: &str = "fact_policy_owner";
pub(crate) const META_FACT_HOT_AUTHORITY: &str = "fact_hot_authority";
pub(crate) const META_FACT_DURABLE_AUTHORITY: &str = "fact_durable_authority";
pub(crate) const META_FACT_PERSISTENCE_SCOPE: &str = "fact_persistence_scope";
pub(crate) const META_FACT_LIFECYCLE_STATE: &str = "fact_lifecycle_state";
pub(crate) const META_FACT_CONFIDENCE: &str = "fact_confidence";
pub(crate) const META_FACT_STATUS: &str = "fact_status";
pub(crate) const META_FACT_PROTECTION: &str = "fact_protection";
pub(crate) const META_FACT_VERIFIED: &str = "fact_verified";
pub(crate) const META_FACT_SEMANTIC_HASH: &str = "fact_semantic_hash";
pub(crate) const META_FACT_CREATED_AT: &str = "fact_created_at";
pub(crate) const META_FACT_UPDATED_AT: &str = "fact_updated_at";
pub(crate) const META_FACT_RELATIONS: &str = "fact_relations";
pub(crate) const META_FACT_ARCHIVED_AT_MS: &str = "fact_archived_at_ms";
pub(crate) const META_FACT_PRUNED_AT_MS: &str = "fact_pruned_at_ms";
pub(crate) const META_FACT_PRUNE_REASON: &str = "fact_prune_reason";

pub(crate) const META_FACT_REVIEW_REASON: &str = "fact_review_reason";
pub(crate) const META_FACT_REVIEW_SUMMARY: &str = "fact_review_summary";
pub(crate) const META_FACT_REVIEW_SOURCE: &str = "fact_review_source";
pub(crate) const META_FACT_REVIEW_REQUESTED_AT_MS: &str = "fact_review_requested_at_ms";
pub(crate) const META_FACT_REVIEW_RESOLUTION_OUTCOME: &str = "fact_review_resolution_outcome";
pub(crate) const META_FACT_REVIEW_RESOLUTION_REASON: &str = "fact_review_resolution_reason";
pub(crate) const META_FACT_REVIEW_RESOLUTION_BASIS: &str = "fact_review_resolution_basis";
pub(crate) const META_FACT_REVIEW_RESOLVED_BY: &str = "fact_review_resolved_by";
pub(crate) const META_FACT_REVIEW_RESOLVED_AT_MS: &str = "fact_review_resolved_at_ms";

pub(crate) const FACT_LIFECYCLE_ACTIVE: &str = "active";
pub(crate) const FACT_LIFECYCLE_ARCHIVED: &str = "archived";
pub(crate) const FACT_LIFECYCLE_VERIFIED: &str = "verified";
pub(crate) const FACT_LIFECYCLE_PRUNED: &str = "pruned";

pub(crate) const FACT_REVIEW_OUTCOME_VERIFIED: &str = "verified";
pub(crate) const FACT_REVIEW_OUTCOME_PRUNED: &str = "pruned";
pub(crate) const FACT_REVIEW_OUTCOME_PENDING_REVIEW: &str = "pending_review";
pub(crate) const FACT_PRUNE_REASON_MANUAL: &str = "manual_prune";
pub(crate) const FACT_POLICY_OWNER_BRAIN: &str = "brain";
pub(crate) const FACT_HOT_AUTHORITY_BRAIN_HOT_MEMORY: &str = "brain_hot_memory";
pub(crate) const FACT_DURABLE_AUTHORITY_ENGRAM: &str = "engram";
pub(crate) const FACT_PERSISTENCE_SCOPE_DURABLE: &str = "durable";

pub(crate) const META_DOCUMENT_CONTRACT_VERSION: &str = "document_contract_version";
pub(crate) const META_DOCUMENT_POLICY_OWNER: &str = "document_policy_owner";
pub(crate) const META_DOCUMENT_DURABLE_AUTHORITY: &str = "document_durable_authority";
pub(crate) const META_DOCUMENT_PERSISTENCE_SCOPE: &str = "document_persistence_scope";
pub(crate) const META_DOCUMENT_CONTEXT_ROLE: &str = "document_context_role";
pub(crate) const META_DOCUMENT_LIFECYCLE_STATE: &str = "document_lifecycle_state";
pub(crate) const META_DOCUMENT_INGEST_SOURCE: &str = "document_ingest_source";
pub(crate) const META_DOCUMENT_ARCHIVED: &str = "document_archived";
pub(crate) const META_DOCUMENT_HAS_SUMMARY: &str = "document_has_summary";
pub(crate) const META_DOCUMENT_IS_STRUCTURAL: &str = "document_is_structural";
pub(crate) const META_DOCUMENT_SUMMARY_STATE: &str = "document_summary_state";
pub(crate) const META_DOCUMENT_ARCHIVE_REASON: &str = "document_archive_reason";
pub(crate) const META_DOCUMENT_ARCHIVED_AT_MS: &str = "document_archived_at_ms";
pub(crate) const META_DOCUMENT_PRUNED_AT_MS: &str = "document_pruned_at_ms";
pub(crate) const META_DOCUMENT_PRUNE_REASON: &str = "document_prune_reason";
pub(crate) const META_DOCUMENT_RETENTION_POLICY_VERSION: &str = "document_retention_policy_version";
pub(crate) const META_SUMMARY_UPDATED_AT_MS: &str = "summary_updated_at_ms";
pub(crate) const META_SUMMARY_SOURCE: &str = "summary_source";

pub(crate) const META_MULTIMODAL_CONTRACT_VERSION: &str = "multimodal_contract_version";
pub(crate) const META_MULTIMODAL_KIND: &str = "multimodal_kind";
pub(crate) const META_MULTIMODAL_MODALITY: &str = "multimodal_modality";
pub(crate) const META_MULTIMODAL_HAS_DERIVED_FACT: &str = "multimodal_has_derived_fact";
pub(crate) const META_MULTIMODAL_SOURCE_PATH: &str = "multimodal_source_path";
pub(crate) const META_MULTIMODAL_SOURCE_URL: &str = "multimodal_source_url";
pub(crate) const META_MULTIMODAL_ROUTE: &str = "multimodal_route";
pub(crate) const META_MULTIMODAL_MODEL: &str = "multimodal_model";
pub(crate) const META_MULTIMODAL_PROMPT: &str = "multimodal_prompt";
pub(crate) const META_MULTIMODAL_ARTIFACT_LOCATOR: &str = "multimodal_artifact_locator";
pub(crate) const MULTIMODAL_CONTRACT_VERSION: &str = "1";

pub(crate) const META_TIER_TARGET: &str = "tier_target";
pub(crate) const META_TIER_PROMOTED_AT_MS: &str = "tier_promoted_at_ms";
pub(crate) const META_TIER_PROMOTION_SOURCE: &str = "tier_promotion_source";
pub(crate) const META_TIER_PROMOTION_CONTRACT_VERSION: &str = "tier_promotion_contract_version";
pub(crate) const META_TIER_PROMOTION_MODE: &str = "tier_promotion_mode";
pub(crate) const META_TIER_POLICY_OWNER: &str = "tier_policy_owner";
pub(crate) const META_TIER_DURABLE_AUTHORITY: &str = "tier_durable_authority";

pub(crate) const PROMOTION_CONTRACT_VERSION: &str = "1";
pub(crate) const TIER_DURABLE_AUTHORITY_ENGRAM: &str = "engram";
pub(crate) const PROMOTION_MODE_POLICY_DRIVEN: &str = "policy_driven";
pub(crate) const PROMOTION_MODE_UTILITY_DRIVEN: &str = "utility_driven";

pub(crate) const RUNTIME_META_ENGRAM_CONTRACT_ROLE: &str = "engram.contract.role";
pub(crate) const RUNTIME_META_ENGRAM_CONTRACT_FACT_VERSION: &str = "engram.contract.fact_version";
pub(crate) const RUNTIME_META_ENGRAM_CONTRACT_DOCUMENT_VERSION: &str =
    "engram.contract.document_version";
pub(crate) const RUNTIME_META_ENGRAM_CONTRACT_SESSION_VERSION: &str =
    "engram.contract.session_version";
pub(crate) const RUNTIME_META_ENGRAM_CONTRACT_PROMOTION_VERSION: &str =
    "engram.contract.promotion_version";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_EXECUTION_PROFILE: &str =
    "engram.vector.execution_profile";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_LAST_EXECUTION_MODE: &str =
    "engram.vector.last_execution_mode";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_SNAPSHOT_LOAD_COUNT: &str =
    "engram.vector.snapshot_load_count";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_REBUILD_COUNT: &str = "engram.vector.rebuild_count";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_ANN_SEARCH_COUNT: &str =
    "engram.vector.ann_search_count";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_EXACT_SCAN_COUNT: &str =
    "engram.vector.exact_scan_count";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_EXACT_BACKFILL_COUNT: &str =
    "engram.vector.exact_backfill_count";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_EXACT_SCAN_FALLBACK_RATE: &str =
    "engram.vector.exact_scan_fallback_rate";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_QUANTIZED_DECODE_FALLBACK_COUNT: &str =
    "engram.vector.quantized_decode_fallback_count";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_QUANTIZED_DECODE_FALLBACK_RATE: &str =
    "engram.vector.quantized_decode_fallback_rate";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_TOMBSTONE_COUNT: &str = "engram.vector.tombstone_count";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_TOMBSTONE_RATIO: &str = "engram.vector.tombstone_ratio";
pub(crate) const RUNTIME_META_ENGRAM_VECTOR_SEARCH_LATENCY_BY_METRIC_AND_COLLECTION: &str =
    "engram.vector.search_latency_by_metric_and_collection";
pub(crate) const RUNTIME_META_ENGRAM_SEARCH_TOTAL_DOCUMENTS: &str = "engram.search.total_documents";
pub(crate) const RUNTIME_META_ENGRAM_SEARCH_TOTAL_COLLECTIONS: &str =
    "engram.search.total_collections";
pub(crate) const RUNTIME_META_ENGRAM_SEARCH_TOTAL_UNVERIFIED: &str =
    "engram.search.total_unverified";
pub(crate) const RUNTIME_META_ENGRAM_SESSION_ARCHIVE_COUNT: &str = "engram.session.archive_count";
pub(crate) const RUNTIME_META_ENGRAM_SESSION_RECOVERY_COUNT: &str = "engram.session.recovery_count";
pub(crate) const RUNTIME_META_ENGRAM_SESSION_BACKGROUND_ARCHIVE_COUNT: &str =
    "engram.session.background_archive_count";
pub(crate) const RUNTIME_META_ENGRAM_SESSION_BACKGROUND_RECOVERY_COUNT: &str =
    "engram.session.background_recovery_count";
pub(crate) const RUNTIME_META_ENGRAM_PRUNE_COUNTS_BY_REASON: &str = "engram.prune.counts_by_reason";
pub(crate) const RUNTIME_META_ENGRAM_RETENTION_POLICY: &str = "engram.retention.policy";
pub(crate) const RUNTIME_META_ENGRAM_RETENTION_LAST_RUN: &str = "engram.retention.last_run";
pub(crate) const RUNTIME_META_ENGRAM_PROMOTION_OPERATION_COUNT: &str =
    "engram.promotion.operation_count";
pub(crate) const RUNTIME_META_ENGRAM_PROMOTION_DOCUMENT_COUNT: &str =
    "engram.promotion.document_count";
pub(crate) const RUNTIME_META_ENGRAM_PROMOTION_LAST_SOURCE: &str = "engram.promotion.last_source";
pub(crate) const RUNTIME_META_ENGRAM_PROMOTION_LAST_TARGET: &str = "engram.promotion.last_target";
pub(crate) const RUNTIME_META_ENGRAM_PROMOTION_LAST_MODE: &str = "engram.promotion.last_mode";
pub(crate) const RUNTIME_META_ENGRAM_PROMOTION_LAST_POLICY_OWNER: &str =
    "engram.promotion.last_policy_owner";
pub(crate) const RUNTIME_META_ENGRAM_PROMOTION_COUNTS_BY_SOURCE_TARGET: &str =
    "engram.promotion.counts_by_source_target";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_OUTCOME: &str =
    "engram.windows_native.embed_outcome";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_CLASS: &str =
    "engram.windows_native.embed_class";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_PROVIDER: &str =
    "engram.windows_native.embed_provider";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_DEVICE_TARGET: &str =
    "engram.windows_native.embed_device_target";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_FALLBACK_MODE: &str =
    "engram.windows_native.embed_fallback_mode";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_STRATEGY: &str =
    "engram.windows_native.embed_strategy";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_NOTE: &str =
    "engram.windows_native.embed_note";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_OUTCOME: &str =
    "engram.windows_native.rerank_outcome";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_CLASS: &str =
    "engram.windows_native.rerank_class";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_PROVIDER: &str =
    "engram.windows_native.rerank_provider";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_DEVICE_TARGET: &str =
    "engram.windows_native.rerank_device_target";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_FALLBACK_MODE: &str =
    "engram.windows_native.rerank_fallback_mode";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_STRATEGY: &str =
    "engram.windows_native.rerank_strategy";
pub(crate) const RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_NOTE: &str =
    "engram.windows_native.rerank_note";

#[cfg(test)]
pub(crate) const RUNTIME_METADATA_VIEW_KEYS: &[&str] = &[
    RUNTIME_META_ENGRAM_CONTRACT_ROLE,
    RUNTIME_META_ENGRAM_CONTRACT_FACT_VERSION,
    RUNTIME_META_ENGRAM_CONTRACT_DOCUMENT_VERSION,
    RUNTIME_META_ENGRAM_CONTRACT_SESSION_VERSION,
    RUNTIME_META_ENGRAM_CONTRACT_PROMOTION_VERSION,
    RUNTIME_META_ENGRAM_VECTOR_EXECUTION_PROFILE,
    RUNTIME_META_ENGRAM_VECTOR_LAST_EXECUTION_MODE,
    RUNTIME_META_ENGRAM_VECTOR_SNAPSHOT_LOAD_COUNT,
    RUNTIME_META_ENGRAM_VECTOR_REBUILD_COUNT,
    RUNTIME_META_ENGRAM_VECTOR_ANN_SEARCH_COUNT,
    RUNTIME_META_ENGRAM_VECTOR_EXACT_SCAN_COUNT,
    RUNTIME_META_ENGRAM_VECTOR_EXACT_BACKFILL_COUNT,
    RUNTIME_META_ENGRAM_VECTOR_EXACT_SCAN_FALLBACK_RATE,
    RUNTIME_META_ENGRAM_VECTOR_QUANTIZED_DECODE_FALLBACK_COUNT,
    RUNTIME_META_ENGRAM_VECTOR_QUANTIZED_DECODE_FALLBACK_RATE,
    RUNTIME_META_ENGRAM_VECTOR_TOMBSTONE_COUNT,
    RUNTIME_META_ENGRAM_VECTOR_TOMBSTONE_RATIO,
    RUNTIME_META_ENGRAM_VECTOR_SEARCH_LATENCY_BY_METRIC_AND_COLLECTION,
    RUNTIME_META_ENGRAM_SEARCH_TOTAL_DOCUMENTS,
    RUNTIME_META_ENGRAM_SEARCH_TOTAL_COLLECTIONS,
    RUNTIME_META_ENGRAM_SEARCH_TOTAL_UNVERIFIED,
    RUNTIME_META_ENGRAM_SESSION_ARCHIVE_COUNT,
    RUNTIME_META_ENGRAM_SESSION_RECOVERY_COUNT,
    RUNTIME_META_ENGRAM_SESSION_BACKGROUND_ARCHIVE_COUNT,
    RUNTIME_META_ENGRAM_SESSION_BACKGROUND_RECOVERY_COUNT,
    RUNTIME_META_ENGRAM_PRUNE_COUNTS_BY_REASON,
    RUNTIME_META_ENGRAM_RETENTION_POLICY,
    RUNTIME_META_ENGRAM_RETENTION_LAST_RUN,
    RUNTIME_META_ENGRAM_PROMOTION_OPERATION_COUNT,
    RUNTIME_META_ENGRAM_PROMOTION_DOCUMENT_COUNT,
    RUNTIME_META_ENGRAM_PROMOTION_LAST_SOURCE,
    RUNTIME_META_ENGRAM_PROMOTION_LAST_TARGET,
    RUNTIME_META_ENGRAM_PROMOTION_LAST_MODE,
    RUNTIME_META_ENGRAM_PROMOTION_LAST_POLICY_OWNER,
    RUNTIME_META_ENGRAM_PROMOTION_COUNTS_BY_SOURCE_TARGET,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_OUTCOME,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_CLASS,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_PROVIDER,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_DEVICE_TARGET,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_FALLBACK_MODE,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_STRATEGY,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_NOTE,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_OUTCOME,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_CLASS,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_PROVIDER,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_DEVICE_TARGET,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_FALLBACK_MODE,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_STRATEGY,
    RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_NOTE,
];

pub(crate) const ENGRAM_CONTRACT_ROLE_DURABLE_LONG_TERM_AUTHORITY_UNDER_BRAIN_POLICY: &str =
    "durable_long_term_authority_under_brain_policy";

pub(crate) const DOCUMENT_PERSISTENCE_SCOPE_DURABLE: &str = "durable";
pub(crate) const DOCUMENT_PERSISTENCE_SCOPE_TRANSIENT: &str = "transient";
pub(crate) const DOCUMENT_CONTEXT_ROLE_DURABLE_DOCUMENT: &str = "durable_document";
pub(crate) const DOCUMENT_CONTEXT_ROLE_TRANSIENT_CONTEXT: &str = "transient_context";
pub(crate) const DOCUMENT_POLICY_OWNER_BRAIN: &str = "brain";
pub(crate) const DOCUMENT_DURABLE_AUTHORITY_ENGRAM: &str = "engram";
pub(crate) const DOCUMENT_INGEST_SOURCE_BRAIN_MULTIMODAL_WRITEBACK: &str =
    "brain_multimodal_writeback";
pub(crate) const DOCUMENT_INGEST_SOURCE_BRAIN_STORE_KNOWLEDGE: &str = "brain_store_knowledge";
pub(crate) const SUMMARY_SOURCE_BRAIN_MEMORY_MANAGER: &str = "brain_memory_manager";
pub(crate) const DOCUMENT_LIFECYCLE_ACTIVE: &str = "active";
pub(crate) const DOCUMENT_LIFECYCLE_PENDING_REVIEW: &str = "pending_review";
pub(crate) const DOCUMENT_LIFECYCLE_ARCHIVED: &str = "archived";
pub(crate) const DOCUMENT_LIFECYCLE_SUMMARIZED: &str = "summarized";
pub(crate) const DOCUMENT_LIFECYCLE_MULTIMODAL_RECORDED: &str = "multimodal_recorded";
pub(crate) const DOCUMENT_LIFECYCLE_PRUNED: &str = "pruned";
pub(crate) const DOCUMENT_SUMMARY_STATE_READY: &str = "ready";

pub(crate) const META_AUDIT_KIND: &str = "audit_kind";
pub(crate) const META_SESSION_ID: &str = "session_id";
pub(crate) const META_SESSION_CONTRACT_VERSION: &str = "session_contract_version";
pub(crate) const META_SESSION_AUDIT_SOURCE: &str = "session_audit_source";
pub(crate) const META_SESSION_LIFECYCLE_STATE: &str = "session_lifecycle_state";
pub(crate) const META_SESSION_PRUNE_REASON: &str = "session_prune_reason";
pub(crate) const META_SESSION_PRUNED_AT_MS: &str = "session_pruned_at_ms";
pub(crate) const META_SESSION_UPDATED_AT_MS: &str = "session_updated_at_ms";
pub(crate) const META_SESSION_ARCHIVED_AT_MS: &str = "session_archived_at_ms";
pub(crate) const META_SESSION_RETENTION_UNTIL_MS: &str = "session_retention_until_ms";
pub(crate) const META_SESSION_ARCHIVE_REASON: &str = "session_archive_reason";
pub(crate) const META_SESSION_EVENT_AT_MS: &str = "session_event_at_ms";
pub(crate) const META_SESSION_AUDIT_REASON: &str = "session_audit_reason";
pub(crate) const META_SESSION_RECOVERED_FROM: &str = "session_recovered_from";
pub(crate) const META_SESSION_LAST_RECOVERED_AT_MS: &str = "session_last_recovered_at_ms";
pub(crate) const META_SESSION_BACKGROUND_PRESENT: &str = "session_background_present";
pub(crate) const META_SESSION_BACKGROUND_LIFECYCLE_STATE: &str =
    "session_background_lifecycle_state";
pub(crate) const META_SESSION_BACKGROUND_REVISION: &str = "session_background_revision";

pub(crate) const META_DOCUMENT_ID: &str = "document_id";
pub(crate) const META_DOCUMENT_COLLECTION: &str = "document_collection";
pub(crate) const META_DOCUMENT_PATH: &str = "document_path";
pub(crate) const META_DOCUMENT_UPDATED_AT_MS: &str = "document_updated_at_ms";
pub(crate) const META_DOCUMENT_AUDIT_REASON: &str = "document_audit_reason";
pub(crate) const META_DOCUMENT_EVENT_AT_MS: &str = "document_event_at_ms";

pub(crate) const COLLECTION_SESSION_AUDIT: &str = "session_audit";
pub(crate) const COLLECTION_DOCUMENT_AUDIT: &str = "document_audit";

pub(crate) const SESSION_LIFECYCLE_ACTIVE: &str = "active";
pub(crate) const SESSION_LIFECYCLE_ARCHIVED: &str = "archived";

pub(crate) const AUDIT_KIND_SESSION_PRUNED: &str = "session_pruned";
pub(crate) const AUDIT_KIND_SESSION_ARCHIVED: &str = "session_archived";
pub(crate) const AUDIT_KIND_SESSION_RECOVERED: &str = "session_recovered";
pub(crate) const AUDIT_KIND_DOCUMENT_AUTO_ARCHIVED: &str = "document_auto_archived";
pub(crate) const AUDIT_KIND_DOCUMENT_PRUNED: &str = "document_pruned";

pub(crate) const RETENTION_REASON_UNVERIFIED: &str = "unverified_retention_policy";
pub(crate) const RETENTION_REASON_ARCHIVED: &str = "archived_retention_policy";
pub(crate) const SESSION_AUDIT_SOURCE_ENGRAM_STORE_SESSION: &str = "engram_store_session";

pub(crate) fn runtime_metadata_value_from_stats(
    key: &str,
    stats: &HybridSearchStats,
    fact_contract_version: &str,
    document_contract_version: &str,
    session_contract_version: &str,
) -> Option<String> {
    match key {
        RUNTIME_META_ENGRAM_CONTRACT_ROLE => {
            Some(ENGRAM_CONTRACT_ROLE_DURABLE_LONG_TERM_AUTHORITY_UNDER_BRAIN_POLICY.to_string())
        }
        RUNTIME_META_ENGRAM_CONTRACT_FACT_VERSION => Some(fact_contract_version.to_string()),
        RUNTIME_META_ENGRAM_CONTRACT_DOCUMENT_VERSION => {
            Some(document_contract_version.to_string())
        }
        RUNTIME_META_ENGRAM_CONTRACT_SESSION_VERSION => Some(session_contract_version.to_string()),
        RUNTIME_META_ENGRAM_CONTRACT_PROMOTION_VERSION => {
            Some(PROMOTION_CONTRACT_VERSION.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_EXECUTION_PROFILE => {
            Some(stats.vector_execution_profile.clone())
        }
        RUNTIME_META_ENGRAM_VECTOR_LAST_EXECUTION_MODE => {
            Some(stats.vector_last_execution_mode.clone())
        }
        RUNTIME_META_ENGRAM_VECTOR_SNAPSHOT_LOAD_COUNT => {
            Some(stats.vector_snapshot_load_count.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_REBUILD_COUNT => Some(stats.vector_rebuild_count.to_string()),
        RUNTIME_META_ENGRAM_VECTOR_ANN_SEARCH_COUNT => {
            Some(stats.vector_ann_search_count.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_EXACT_SCAN_COUNT => {
            Some(stats.vector_exact_scan_count.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_EXACT_BACKFILL_COUNT => {
            Some(stats.vector_exact_backfill_count.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_EXACT_SCAN_FALLBACK_RATE => {
            Some(stats.vector_exact_scan_fallback_rate.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_QUANTIZED_DECODE_FALLBACK_COUNT => {
            Some(stats.vector_quantized_decode_fallback_count.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_QUANTIZED_DECODE_FALLBACK_RATE => {
            Some(stats.vector_quantized_decode_fallback_rate.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_TOMBSTONE_COUNT => {
            Some(stats.vector_tombstone_count.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_TOMBSTONE_RATIO => {
            Some(stats.vector_tombstone_ratio.to_string())
        }
        RUNTIME_META_ENGRAM_VECTOR_SEARCH_LATENCY_BY_METRIC_AND_COLLECTION => Some(
            stats
                .vector_search_latency_by_metric_and_collection_json
                .clone(),
        ),
        RUNTIME_META_ENGRAM_SEARCH_TOTAL_DOCUMENTS => Some(stats.total_documents.to_string()),
        RUNTIME_META_ENGRAM_SEARCH_TOTAL_COLLECTIONS => Some(stats.total_collections.to_string()),
        RUNTIME_META_ENGRAM_SEARCH_TOTAL_UNVERIFIED => Some(stats.total_unverified.to_string()),
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_OUTCOME => {
            Some(stats.windows_native_embed_outcome.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_CLASS => {
            Some(stats.windows_native_embed_class.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_PROVIDER => {
            Some(stats.windows_native_embed_provider.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_DEVICE_TARGET => {
            Some(stats.windows_native_embed_device_target.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_FALLBACK_MODE => {
            Some(stats.windows_native_embed_fallback_mode.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_STRATEGY => {
            Some(stats.windows_native_embed_strategy.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_EMBED_NOTE => {
            Some(stats.windows_native_embed_note.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_OUTCOME => {
            Some(stats.windows_native_rerank_outcome.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_CLASS => {
            Some(stats.windows_native_rerank_class.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_PROVIDER => {
            Some(stats.windows_native_rerank_provider.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_DEVICE_TARGET => {
            Some(stats.windows_native_rerank_device_target.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_FALLBACK_MODE => {
            Some(stats.windows_native_rerank_fallback_mode.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_STRATEGY => {
            Some(stats.windows_native_rerank_strategy.clone())
        }
        RUNTIME_META_ENGRAM_WINDOWS_NATIVE_RERANK_NOTE => {
            Some(stats.windows_native_rerank_note.clone())
        }
        RUNTIME_META_ENGRAM_SESSION_ARCHIVE_COUNT => Some(stats.session_archive_count.to_string()),
        RUNTIME_META_ENGRAM_SESSION_RECOVERY_COUNT => {
            Some(stats.session_recovery_count.to_string())
        }
        RUNTIME_META_ENGRAM_SESSION_BACKGROUND_ARCHIVE_COUNT => {
            Some(stats.session_background_archive_count.to_string())
        }
        RUNTIME_META_ENGRAM_SESSION_BACKGROUND_RECOVERY_COUNT => {
            Some(stats.session_background_recovery_count.to_string())
        }
        RUNTIME_META_ENGRAM_PRUNE_COUNTS_BY_REASON => {
            Some(stats.prune_count_by_reason_json.clone())
        }
        RUNTIME_META_ENGRAM_RETENTION_POLICY => Some(stats.retention_policy_json.clone()),
        RUNTIME_META_ENGRAM_RETENTION_LAST_RUN => Some(stats.retention_last_run_json.clone()),
        RUNTIME_META_ENGRAM_PROMOTION_OPERATION_COUNT => {
            Some(stats.promotion_operation_count.to_string())
        }
        RUNTIME_META_ENGRAM_PROMOTION_DOCUMENT_COUNT => {
            Some(stats.promotion_document_count.to_string())
        }
        RUNTIME_META_ENGRAM_PROMOTION_LAST_SOURCE => Some(stats.promotion_last_source.clone()),
        RUNTIME_META_ENGRAM_PROMOTION_LAST_TARGET => Some(stats.promotion_last_target.clone()),
        RUNTIME_META_ENGRAM_PROMOTION_LAST_MODE => Some(stats.promotion_last_mode.clone()),
        RUNTIME_META_ENGRAM_PROMOTION_LAST_POLICY_OWNER => {
            Some(stats.promotion_last_policy_owner.clone())
        }
        RUNTIME_META_ENGRAM_PROMOTION_COUNTS_BY_SOURCE_TARGET => {
            Some(stats.promotion_counts_by_source_target_json.clone())
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn runtime_metadata_snapshot_from_stats(
    stats: &HybridSearchStats,
    fact_contract_version: &str,
    document_contract_version: &str,
    session_contract_version: &str,
) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    for key in RUNTIME_METADATA_VIEW_KEYS {
        if let Some(value) = runtime_metadata_value_from_stats(
            key,
            stats,
            fact_contract_version,
            document_contract_version,
            session_contract_version,
        ) {
            metadata.insert((*key).to_string(), value);
        }
    }
    metadata
}

pub(crate) fn tier_promotion_metadata(
    target: &str,
    promoted_at_ms: &str,
    source: &str,
    mode: &str,
    policy_owner: &str,
) -> HashMap<String, String> {
    HashMap::from([
        (META_TIER_TARGET.to_string(), target.to_string()),
        (
            META_TIER_PROMOTED_AT_MS.to_string(),
            promoted_at_ms.to_string(),
        ),
        (META_TIER_PROMOTION_SOURCE.to_string(), source.to_string()),
        (
            META_TIER_PROMOTION_CONTRACT_VERSION.to_string(),
            PROMOTION_CONTRACT_VERSION.to_string(),
        ),
        (META_TIER_PROMOTION_MODE.to_string(), mode.to_string()),
        (META_TIER_POLICY_OWNER.to_string(), policy_owner.to_string()),
        (
            META_TIER_DURABLE_AUTHORITY.to_string(),
            TIER_DURABLE_AUTHORITY_ENGRAM.to_string(),
        ),
    ])
}

pub(crate) fn pending_review_metadata(
    updated_at_ms: i64,
    summary: Option<&str>,
) -> HashMap<String, String> {
    let mut metadata = HashMap::from([
        (
            META_FACT_STATUS.to_string(),
            "\"pending_review\"".to_string(),
        ),
        (
            META_FACT_LIFECYCLE_STATE.to_string(),
            DOCUMENT_LIFECYCLE_PENDING_REVIEW.to_string(),
        ),
        (
            META_FACT_REVIEW_REQUESTED_AT_MS.to_string(),
            updated_at_ms.to_string(),
        ),
        (
            META_DOCUMENT_LIFECYCLE_STATE.to_string(),
            DOCUMENT_LIFECYCLE_PENDING_REVIEW.to_string(),
        ),
    ]);
    if let Some(summary) = summary {
        metadata.insert(META_FACT_REVIEW_SUMMARY.to_string(), summary.to_string());
    }
    metadata
}

pub(crate) fn document_summary_metadata(updated_at_ms: i64) -> HashMap<String, String> {
    HashMap::from([
        (
            META_DOCUMENT_LIFECYCLE_STATE.to_string(),
            DOCUMENT_LIFECYCLE_SUMMARIZED.to_string(),
        ),
        (
            META_DOCUMENT_SUMMARY_STATE.to_string(),
            DOCUMENT_SUMMARY_STATE_READY.to_string(),
        ),
        (
            META_SUMMARY_UPDATED_AT_MS.to_string(),
            updated_at_ms.to_string(),
        ),
    ])
}

pub(crate) fn retention_archive_metadata(
    contract_version: u32,
    archive_reason: &str,
) -> HashMap<String, String> {
    HashMap::from([
        (
            META_DOCUMENT_ARCHIVE_REASON.to_string(),
            archive_reason.to_string(),
        ),
        (
            META_DOCUMENT_RETENTION_POLICY_VERSION.to_string(),
            contract_version.to_string(),
        ),
    ])
}

pub(crate) fn retention_prune_metadata(
    contract_version: u32,
    event_at_ms: i64,
    include_fact_contract: bool,
    prune_reason: &str,
) -> HashMap<String, String> {
    let mut metadata = HashMap::from([
        (
            META_DOCUMENT_LIFECYCLE_STATE.to_string(),
            DOCUMENT_LIFECYCLE_PRUNED.to_string(),
        ),
        (
            META_DOCUMENT_PRUNED_AT_MS.to_string(),
            event_at_ms.to_string(),
        ),
        (
            META_DOCUMENT_PRUNE_REASON.to_string(),
            prune_reason.to_string(),
        ),
        (
            META_DOCUMENT_RETENTION_POLICY_VERSION.to_string(),
            contract_version.to_string(),
        ),
    ]);
    if include_fact_contract {
        metadata.insert(
            META_FACT_LIFECYCLE_STATE.to_string(),
            FACT_LIFECYCLE_PRUNED.to_string(),
        );
        metadata.insert(META_FACT_PRUNE_REASON.to_string(), prune_reason.to_string());
    }
    metadata
}

pub(crate) fn session_prune_audit_metadata(
    reason: &str,
    pruned_at_ms: i64,
) -> HashMap<String, String> {
    HashMap::from([
        (
            META_AUDIT_KIND.to_string(),
            AUDIT_KIND_SESSION_PRUNED.to_string(),
        ),
        (META_SESSION_PRUNE_REASON.to_string(), reason.to_string()),
        (
            META_SESSION_PRUNED_AT_MS.to_string(),
            pruned_at_ms.to_string(),
        ),
    ])
}

pub(crate) fn session_event_audit_metadata(
    audit_kind: &str,
    audit_reason: Option<&str>,
    event_at_ms: i64,
) -> HashMap<String, String> {
    let mut metadata = HashMap::from([
        (META_AUDIT_KIND.to_string(), audit_kind.to_string()),
        (
            META_SESSION_EVENT_AT_MS.to_string(),
            event_at_ms.to_string(),
        ),
    ]);
    if let Some(reason) = audit_reason.filter(|value| !value.trim().is_empty()) {
        metadata.insert(META_SESSION_AUDIT_REASON.to_string(), reason.to_string());
    }
    metadata
}

pub(crate) fn document_event_audit_metadata(
    audit_kind: &str,
    audit_reason: &str,
    event_at_ms: i64,
) -> HashMap<String, String> {
    HashMap::from([
        (META_AUDIT_KIND.to_string(), audit_kind.to_string()),
        (
            META_DOCUMENT_AUDIT_REASON.to_string(),
            audit_reason.to_string(),
        ),
        (
            META_DOCUMENT_EVENT_AT_MS.to_string(),
            event_at_ms.to_string(),
        ),
    ])
}

#[cfg(test)]
pub(crate) struct PromotionMetadataView<'a> {
    metadata: &'a HashMap<String, String>,
}

#[cfg(test)]
impl<'a> PromotionMetadataView<'a> {
    pub(crate) fn new(metadata: &'a HashMap<String, String>) -> Self {
        Self { metadata }
    }

    pub(crate) fn target(&self) -> Option<&str> {
        self.metadata.get(META_TIER_TARGET).map(String::as_str)
    }

    pub(crate) fn source(&self) -> Option<&str> {
        self.metadata
            .get(META_TIER_PROMOTION_SOURCE)
            .map(String::as_str)
    }

    pub(crate) fn contract_version(&self) -> Option<&str> {
        self.metadata
            .get(META_TIER_PROMOTION_CONTRACT_VERSION)
            .map(String::as_str)
    }

    pub(crate) fn mode(&self) -> Option<&str> {
        self.metadata
            .get(META_TIER_PROMOTION_MODE)
            .map(String::as_str)
    }

    pub(crate) fn policy_owner(&self) -> Option<&str> {
        self.metadata
            .get(META_TIER_POLICY_OWNER)
            .map(String::as_str)
    }

    pub(crate) fn durable_authority(&self) -> Option<&str> {
        self.metadata
            .get(META_TIER_DURABLE_AUTHORITY)
            .map(String::as_str)
    }

    pub(crate) fn promoted_at_ms(&self) -> Option<i64> {
        self.metadata
            .get(META_TIER_PROMOTED_AT_MS)
            .and_then(|value| value.parse::<i64>().ok())
    }
}

pub(crate) struct FactMetadataView<'a> {
    metadata: &'a HashMap<String, String>,
    pub unverified: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

pub(crate) struct AuditMetadataView<'a> {
    metadata: &'a HashMap<String, String>,
}

#[cfg(test)]
pub(crate) struct RuntimeMetadataView<'a> {
    metadata: &'a HashMap<String, String>,
}

#[cfg(test)]
impl<'a> RuntimeMetadataView<'a> {
    pub(crate) fn new(metadata: &'a HashMap<String, String>) -> Self {
        Self { metadata }
    }

    pub(crate) fn value(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(String::as_str)
    }

    pub(crate) fn contract_role(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_CONTRACT_ROLE)
    }

    pub(crate) fn fact_version(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_CONTRACT_FACT_VERSION)
    }

    pub(crate) fn document_version(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_CONTRACT_DOCUMENT_VERSION)
    }

    pub(crate) fn session_version(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_CONTRACT_SESSION_VERSION)
    }

    pub(crate) fn promotion_version(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_CONTRACT_PROMOTION_VERSION)
    }

    pub(crate) fn vector_execution_profile(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_VECTOR_EXECUTION_PROFILE)
    }

    pub(crate) fn vector_snapshot_load_count(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_VECTOR_SNAPSHOT_LOAD_COUNT)
    }

    pub(crate) fn vector_exact_scan_fallback_rate(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_VECTOR_EXACT_SCAN_FALLBACK_RATE)
    }

    pub(crate) fn vector_quantized_decode_fallback_count(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_VECTOR_QUANTIZED_DECODE_FALLBACK_COUNT)
    }

    pub(crate) fn vector_quantized_decode_fallback_rate(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_VECTOR_QUANTIZED_DECODE_FALLBACK_RATE)
    }

    pub(crate) fn vector_tombstone_count(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_VECTOR_TOMBSTONE_COUNT)
    }

    pub(crate) fn vector_tombstone_ratio(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_VECTOR_TOMBSTONE_RATIO)
    }

    pub(crate) fn vector_search_latency_by_metric_and_collection(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_VECTOR_SEARCH_LATENCY_BY_METRIC_AND_COLLECTION)
    }

    pub(crate) fn search_total_documents(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_SEARCH_TOTAL_DOCUMENTS)
    }

    pub(crate) fn session_archive_count(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_SESSION_ARCHIVE_COUNT)
    }

    pub(crate) fn session_recovery_count(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_SESSION_RECOVERY_COUNT)
    }

    pub(crate) fn session_background_archive_count(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_SESSION_BACKGROUND_ARCHIVE_COUNT)
    }

    pub(crate) fn session_background_recovery_count(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_SESSION_BACKGROUND_RECOVERY_COUNT)
    }

    pub(crate) fn prune_counts_by_reason(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_PRUNE_COUNTS_BY_REASON)
    }

    pub(crate) fn retention_policy(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_RETENTION_POLICY)
    }

    pub(crate) fn retention_last_run(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_RETENTION_LAST_RUN)
    }

    pub(crate) fn promotion_operation_count(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_PROMOTION_OPERATION_COUNT)
    }

    pub(crate) fn promotion_document_count(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_PROMOTION_DOCUMENT_COUNT)
    }

    pub(crate) fn promotion_last_source(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_PROMOTION_LAST_SOURCE)
    }

    pub(crate) fn promotion_last_target(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_PROMOTION_LAST_TARGET)
    }

    pub(crate) fn promotion_last_mode(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_PROMOTION_LAST_MODE)
    }

    pub(crate) fn promotion_last_policy_owner(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_PROMOTION_LAST_POLICY_OWNER)
    }

    pub(crate) fn promotion_counts_by_source_target(&self) -> Option<&str> {
        self.value(RUNTIME_META_ENGRAM_PROMOTION_COUNTS_BY_SOURCE_TARGET)
    }
}

impl<'a> AuditMetadataView<'a> {
    pub(crate) fn new(metadata: &'a HashMap<String, String>) -> Self {
        Self { metadata }
    }

    pub(crate) fn audit_kind(&self) -> Option<&str> {
        self.metadata.get(META_AUDIT_KIND).map(String::as_str)
    }

    pub(crate) fn reason(&self) -> Option<&str> {
        self.metadata
            .get(META_DOCUMENT_AUDIT_REASON)
            .map(String::as_str)
    }
}

impl<'a> FactMetadataView<'a> {
    pub(crate) fn new(
        metadata: &'a HashMap<String, String>,
        unverified: bool,
        created_at_ms: i64,
        updated_at_ms: i64,
    ) -> Self {
        Self {
            metadata,
            unverified,
            created_at_ms,
            updated_at_ms,
        }
    }

    pub(crate) fn source(&self) -> Option<String> {
        self.metadata.get(META_FACT_SOURCE).cloned()
    }

    pub(crate) fn contract_version(&self) -> &str {
        self.metadata
            .get(META_FACT_CONTRACT_VERSION)
            .map(String::as_str)
            .unwrap_or("1")
    }

    pub(crate) fn has_contract_version(&self) -> bool {
        self.metadata.contains_key(META_FACT_CONTRACT_VERSION)
    }

    pub(crate) fn policy_owner(&self) -> &str {
        self.metadata
            .get(META_FACT_POLICY_OWNER)
            .map(String::as_str)
            .unwrap_or(FACT_POLICY_OWNER_BRAIN)
    }

    pub(crate) fn hot_authority(&self) -> &str {
        self.metadata
            .get(META_FACT_HOT_AUTHORITY)
            .map(String::as_str)
            .unwrap_or(FACT_HOT_AUTHORITY_BRAIN_HOT_MEMORY)
    }

    pub(crate) fn durable_authority(&self) -> &str {
        self.metadata
            .get(META_FACT_DURABLE_AUTHORITY)
            .map(String::as_str)
            .unwrap_or(FACT_DURABLE_AUTHORITY_ENGRAM)
    }

    pub(crate) fn persistence_scope(&self) -> &str {
        self.metadata
            .get(META_FACT_PERSISTENCE_SCOPE)
            .map(String::as_str)
            .unwrap_or(FACT_PERSISTENCE_SCOPE_DURABLE)
    }

    pub(crate) fn lifecycle_state(&self) -> &str {
        self.metadata
            .get(META_FACT_LIFECYCLE_STATE)
            .map(String::as_str)
            .unwrap_or(FACT_LIFECYCLE_ACTIVE)
    }

    pub(crate) fn prune_reason(&self) -> Option<&str> {
        self.metadata
            .get(META_FACT_PRUNE_REASON)
            .map(String::as_str)
    }

    pub(crate) fn created_at_ms(&self) -> i64 {
        self.metadata
            .get(META_FACT_CREATED_AT)
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(self.created_at_ms)
    }

    pub(crate) fn updated_at_ms(&self) -> i64 {
        self.metadata
            .get(META_FACT_UPDATED_AT)
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(self.updated_at_ms)
    }

    pub(crate) fn status_json(&self) -> Option<&str> {
        self.metadata.get(META_FACT_STATUS).map(String::as_str)
    }

    pub(crate) fn relations_json(&self) -> Option<&str> {
        self.metadata.get(META_FACT_RELATIONS).map(String::as_str)
    }

    pub(crate) fn protection_json(&self) -> Option<&str> {
        self.metadata.get(META_FACT_PROTECTION).map(String::as_str)
    }

    pub(crate) fn semantic_hash(&self) -> Option<String> {
        self.metadata
            .get(META_FACT_SEMANTIC_HASH)
            .filter(|value| !value.is_empty())
            .cloned()
    }

    pub(crate) fn confidence(&self) -> f32 {
        self.metadata
            .get(META_FACT_CONFIDENCE)
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(1.0)
    }
}

pub(crate) struct FactReviewMetadataView<'a> {
    metadata: &'a HashMap<String, String>,
}

impl<'a> FactReviewMetadataView<'a> {
    pub(crate) fn new(metadata: &'a HashMap<String, String>) -> Self {
        Self { metadata }
    }

    pub(crate) fn review_reason(&self) -> Option<String> {
        self.metadata.get(META_FACT_REVIEW_REASON).cloned()
    }

    pub(crate) fn challenger_summary(&self) -> Option<String> {
        self.metadata.get(META_FACT_REVIEW_SUMMARY).cloned()
    }

    pub(crate) fn challenger_source(&self) -> Option<String> {
        self.metadata.get(META_FACT_REVIEW_SOURCE).cloned()
    }

    pub(crate) fn review_requested_at_ms(&self) -> Option<i64> {
        self.metadata
            .get(META_FACT_REVIEW_REQUESTED_AT_MS)
            .and_then(|value| value.parse::<i64>().ok())
    }

    pub(crate) fn resolution_outcome(&self) -> Option<&str> {
        self.metadata
            .get(META_FACT_REVIEW_RESOLUTION_OUTCOME)
            .map(String::as_str)
    }

    pub(crate) fn resolution_reason(&self) -> Option<String> {
        self.metadata
            .get(META_FACT_REVIEW_RESOLUTION_REASON)
            .cloned()
    }

    pub(crate) fn resolution_basis(&self) -> Option<String> {
        self.metadata
            .get(META_FACT_REVIEW_RESOLUTION_BASIS)
            .cloned()
    }

    pub(crate) fn resolved_by(&self) -> Option<String> {
        self.metadata.get(META_FACT_REVIEW_RESOLVED_BY).cloned()
    }

    pub(crate) fn resolved_at_ms(&self) -> Option<i64> {
        self.metadata
            .get(META_FACT_REVIEW_RESOLVED_AT_MS)
            .and_then(|value| value.parse::<i64>().ok())
    }
}

pub(crate) struct DocumentMetadataView<'a> {
    metadata: &'a HashMap<String, String>,
    has_summary: bool,
    is_structural: bool,
}

#[cfg(test)]
pub(crate) struct MultimodalMetadataView<'a> {
    metadata: &'a HashMap<String, String>,
}

#[cfg(test)]
impl<'a> MultimodalMetadataView<'a> {
    pub(crate) fn new(metadata: &'a HashMap<String, String>) -> Self {
        Self { metadata }
    }

    pub(crate) fn contract_version(&self) -> Option<&str> {
        self.metadata
            .get(META_MULTIMODAL_CONTRACT_VERSION)
            .map(String::as_str)
    }

    pub(crate) fn modality(&self) -> Option<&str> {
        self.metadata
            .get(META_MULTIMODAL_MODALITY)
            .map(String::as_str)
    }

    pub(crate) fn route(&self) -> Option<&str> {
        self.metadata.get(META_MULTIMODAL_ROUTE).map(String::as_str)
    }

    pub(crate) fn ingest_source(&self) -> Option<&str> {
        self.metadata
            .get(META_DOCUMENT_INGEST_SOURCE)
            .map(String::as_str)
    }
}

pub(crate) struct SessionAuditMetadataView<'a> {
    metadata: &'a HashMap<String, String>,
}

impl<'a> SessionAuditMetadataView<'a> {
    pub(crate) fn new(metadata: &'a HashMap<String, String>) -> Self {
        Self { metadata }
    }

    pub(crate) fn audit_kind(&self) -> Option<&str> {
        self.metadata.get(META_AUDIT_KIND).map(String::as_str)
    }

    pub(crate) fn session_id(&self) -> Option<&str> {
        self.metadata.get(META_SESSION_ID).map(String::as_str)
    }

    pub(crate) fn event_at_ms(&self) -> Option<i64> {
        self.metadata
            .get(META_SESSION_EVENT_AT_MS)
            .and_then(|value| value.parse::<i64>().ok())
    }

    pub(crate) fn archive_reason(&self) -> Option<&str> {
        self.metadata
            .get(META_SESSION_ARCHIVE_REASON)
            .map(String::as_str)
    }

    pub(crate) fn recovered_from(&self) -> Option<&str> {
        self.metadata
            .get(META_SESSION_RECOVERED_FROM)
            .map(String::as_str)
    }

    pub(crate) fn prune_reason(&self) -> Option<&str> {
        self.metadata
            .get(META_SESSION_PRUNE_REASON)
            .map(String::as_str)
    }

    pub(crate) fn background_present(&self) -> Option<&str> {
        self.metadata
            .get(META_SESSION_BACKGROUND_PRESENT)
            .map(String::as_str)
    }

    pub(crate) fn background_lifecycle_state(&self) -> Option<&str> {
        self.metadata
            .get(META_SESSION_BACKGROUND_LIFECYCLE_STATE)
            .map(String::as_str)
    }

    pub(crate) fn background_revision(&self) -> Option<&str> {
        self.metadata
            .get(META_SESSION_BACKGROUND_REVISION)
            .map(String::as_str)
    }
}

impl<'a> DocumentMetadataView<'a> {
    pub(crate) fn new(
        metadata: &'a HashMap<String, String>,
        has_summary: bool,
        is_structural: bool,
    ) -> Self {
        Self {
            metadata,
            has_summary,
            is_structural,
        }
    }

    pub(crate) fn contract_version(&self) -> String {
        self.metadata
            .get(META_DOCUMENT_CONTRACT_VERSION)
            .cloned()
            .unwrap_or_else(|| "1".to_string())
    }

    pub(crate) fn policy_owner(&self) -> String {
        self.metadata
            .get(META_DOCUMENT_POLICY_OWNER)
            .cloned()
            .unwrap_or_else(|| DOCUMENT_POLICY_OWNER_BRAIN.to_string())
    }

    pub(crate) fn durable_authority(&self) -> String {
        self.metadata
            .get(META_DOCUMENT_DURABLE_AUTHORITY)
            .cloned()
            .unwrap_or_else(|| DOCUMENT_DURABLE_AUTHORITY_ENGRAM.to_string())
    }

    pub(crate) fn persistence_scope(&self) -> String {
        self.metadata
            .get(META_DOCUMENT_PERSISTENCE_SCOPE)
            .cloned()
            .unwrap_or_else(|| DOCUMENT_PERSISTENCE_SCOPE_DURABLE.to_string())
    }

    pub(crate) fn context_role(&self) -> String {
        if let Some(value) = self.metadata.get(META_DOCUMENT_CONTEXT_ROLE) {
            return value.clone();
        }
        if self.persistence_scope() == DOCUMENT_PERSISTENCE_SCOPE_TRANSIENT {
            DOCUMENT_CONTEXT_ROLE_TRANSIENT_CONTEXT.to_string()
        } else {
            DOCUMENT_CONTEXT_ROLE_DURABLE_DOCUMENT.to_string()
        }
    }

    pub(crate) fn lifecycle_state(&self) -> String {
        self.metadata
            .get(META_DOCUMENT_LIFECYCLE_STATE)
            .cloned()
            .unwrap_or_else(|| DOCUMENT_LIFECYCLE_ACTIVE.to_string())
    }

    pub(crate) fn archived(&self) -> bool {
        matches!(
            self.lifecycle_state().as_str(),
            DOCUMENT_LIFECYCLE_ARCHIVED | DOCUMENT_LIFECYCLE_PRUNED
        )
    }

    pub(crate) fn archived_at_ms(&self, fallback: i64) -> i64 {
        self.metadata
            .get(META_DOCUMENT_ARCHIVED_AT_MS)
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(fallback)
    }

    pub(crate) fn has_fact_contract_metadata(&self) -> bool {
        self.metadata.contains_key(META_FACT_CONTRACT_VERSION)
    }

    pub(crate) fn has_summary(&self) -> bool {
        self.has_summary
    }

    pub(crate) fn summary_state(&self) -> Option<&str> {
        self.metadata
            .get(META_DOCUMENT_SUMMARY_STATE)
            .map(String::as_str)
    }

    pub(crate) fn summary_source(&self) -> Option<&str> {
        self.metadata.get(META_SUMMARY_SOURCE).map(String::as_str)
    }

    pub(crate) fn archive_reason(&self) -> Option<&str> {
        self.metadata
            .get(META_DOCUMENT_ARCHIVE_REASON)
            .map(String::as_str)
    }

    pub(crate) fn prune_reason(&self) -> Option<&str> {
        self.metadata
            .get(META_DOCUMENT_PRUNE_REASON)
            .map(String::as_str)
    }

    pub(crate) fn is_structural(&self) -> bool {
        self.is_structural
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_helper_like_views_preserve_fact_and_document_defaults() {
        let fact_metadata = HashMap::new();
        let fact_view = FactMetadataView::new(&fact_metadata, false, 10, 20);
        assert_eq!(fact_view.contract_version(), "1");
        assert!(!fact_view.has_contract_version());

        let document_metadata = HashMap::new();
        let document_view = DocumentMetadataView::new(&document_metadata, false, false);
        assert_eq!(document_view.contract_version(), "1");
        assert_eq!(document_view.lifecycle_state(), DOCUMENT_LIFECYCLE_ACTIVE);
        assert_eq!(document_view.archived_at_ms(42), 42);
        assert!(!document_view.has_fact_contract_metadata());
    }

    #[test]
    fn session_audit_view_reads_contract_fields() {
        let mut metadata = HashMap::new();
        metadata.insert(
            META_AUDIT_KIND.to_string(),
            AUDIT_KIND_SESSION_RECOVERED.to_string(),
        );
        metadata.insert(META_SESSION_ID.to_string(), "session-1".to_string());
        metadata.insert(
            META_SESSION_BACKGROUND_PRESENT.to_string(),
            "true".to_string(),
        );
        metadata.insert(
            META_SESSION_BACKGROUND_LIFECYCLE_STATE.to_string(),
            "recovered".to_string(),
        );
        metadata.insert(
            META_SESSION_BACKGROUND_REVISION.to_string(),
            "4".to_string(),
        );

        let view = SessionAuditMetadataView::new(&metadata);
        assert_eq!(view.audit_kind(), Some(AUDIT_KIND_SESSION_RECOVERED));
        assert_eq!(view.session_id(), Some("session-1"));
        assert_eq!(view.background_present(), Some("true"));
        assert_eq!(view.background_lifecycle_state(), Some("recovered"));
        assert_eq!(view.background_revision(), Some("4"));
    }

    #[test]
    fn audit_views_expose_reason_fields() {
        let mut session_metadata = HashMap::new();
        session_metadata.insert(
            META_SESSION_PRUNE_REASON.to_string(),
            RETENTION_REASON_UNVERIFIED.to_string(),
        );
        let session_view = SessionAuditMetadataView::new(&session_metadata);
        assert_eq!(
            session_view.prune_reason(),
            Some(RETENTION_REASON_UNVERIFIED)
        );

        let mut document_metadata = HashMap::new();
        document_metadata.insert(
            META_DOCUMENT_AUDIT_REASON.to_string(),
            RETENTION_REASON_ARCHIVED.to_string(),
        );
        let document_view = AuditMetadataView::new(&document_metadata);
        assert_eq!(document_view.reason(), Some(RETENTION_REASON_ARCHIVED));
    }
}
