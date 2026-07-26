use super::core::Agent;
use crate::agent::provider::Provider;
use benshu_inference::detect_windows_native_runtime_status;
use benshu_telemetry::RuntimeStage;
use std::collections::HashMap;
use tracing::warn;

impl<P: Provider + 'static> Agent<P> {
    pub(crate) async fn apply_engram_windows_native_runtime_metadata(
        &self,
        metadata: &mut HashMap<String, String>,
    ) {
        const ENGRAM_WINDOWS_NATIVE_METADATA_MAPPINGS: [(&str, &str); 14] = [
            (
                "engram.windows_native.embed_outcome",
                "engram_windows_native_embed_outcome",
            ),
            (
                "engram.windows_native.embed_class",
                "engram_windows_native_embed_class",
            ),
            (
                "engram.windows_native.embed_provider",
                "engram_windows_native_embed_provider",
            ),
            (
                "engram.windows_native.embed_device_target",
                "engram_windows_native_embed_device_target",
            ),
            (
                "engram.windows_native.embed_fallback_mode",
                "engram_windows_native_embed_fallback_mode",
            ),
            (
                "engram.windows_native.embed_strategy",
                "engram_windows_native_embed_strategy",
            ),
            (
                "engram.windows_native.embed_note",
                "engram_windows_native_embed_note",
            ),
            (
                "engram.windows_native.rerank_outcome",
                "engram_windows_native_rerank_outcome",
            ),
            (
                "engram.windows_native.rerank_class",
                "engram_windows_native_rerank_class",
            ),
            (
                "engram.windows_native.rerank_provider",
                "engram_windows_native_rerank_provider",
            ),
            (
                "engram.windows_native.rerank_device_target",
                "engram_windows_native_rerank_device_target",
            ),
            (
                "engram.windows_native.rerank_fallback_mode",
                "engram_windows_native_rerank_fallback_mode",
            ),
            (
                "engram.windows_native.rerank_strategy",
                "engram_windows_native_rerank_strategy",
            ),
            (
                "engram.windows_native.rerank_note",
                "engram_windows_native_rerank_note",
            ),
        ];

        let Some(memory) = &self.memory else {
            return;
        };

        for (memory_key, metadata_key) in ENGRAM_WINDOWS_NATIVE_METADATA_MAPPINGS {
            match memory.get_metadata(memory_key).await {
                Ok(Some(value)) if !value.trim().is_empty() => {
                    metadata.insert(metadata_key.to_string(), value.trim().to_string());
                }
                Ok(_) => {}
                Err(err) => {
                    warn!(
                        memory_key,
                        error = %err,
                        "Failed to load engram Windows-native runtime metadata"
                    );
                }
            }
        }
    }

    pub(crate) fn apply_windows_native_trace_metadata(
        &self,
        metadata: &mut HashMap<String, String>,
    ) {
        let status = detect_windows_native_runtime_status();
        metadata.insert(
            "windows_native_host_runtime".to_string(),
            status.host_runtime,
        );
        metadata.insert(
            "windows_native_deployment_lane".to_string(),
            status.deployment_lane,
        );
        metadata.insert(
            "windows_native_deployment_strategy".to_string(),
            status.deployment_strategy,
        );
        metadata.insert(
            "windows_native_deployment_note".to_string(),
            status.deployment_note,
        );
        metadata.insert(
            "windows_native_product_mainline".to_string(),
            status.product_mainline,
        );
        metadata.insert(
            "windows_native_validation_tracks".to_string(),
            status.validation_tracks.join(","),
        );
        metadata.insert(
            "windows_native_priority".to_string(),
            status.windows_native_priority.to_string(),
        );
        metadata.insert(
            "windows_native_small_model_runtime_target".to_string(),
            status.small_model_runtime_target,
        );
        metadata.insert(
            "windows_native_small_model_execution_linked".to_string(),
            status.small_model_execution_linked.to_string(),
        );
        metadata.insert(
            "windows_native_small_model_execution_provider".to_string(),
            status.small_model_execution_provider,
        );
        metadata.insert(
            "windows_native_small_model_device_target".to_string(),
            status.small_model_device_target,
        );
        metadata.insert(
            "windows_native_small_model_fallback_mode".to_string(),
            status.small_model_fallback_mode,
        );
        metadata.insert(
            "windows_native_small_model_runtime_outcome".to_string(),
            status.small_model_runtime_outcome,
        );
        metadata.insert(
            "windows_native_small_model_runtime_strategy".to_string(),
            status.small_model_runtime_strategy,
        );
        metadata.insert(
            "windows_native_small_model_runtime_readiness".to_string(),
            status.small_model_runtime_readiness,
        );
        metadata.insert(
            "windows_native_small_model_runtime_reason".to_string(),
            status.small_model_runtime_reason,
        );
        metadata.insert(
            "windows_native_main_brain_runtime_target".to_string(),
            status.main_brain_runtime_target,
        );
        metadata.insert(
            "windows_native_runtime_contract_complete".to_string(),
            "true".to_string(),
        );
        metadata.insert(
            "windows_native_runtime_surface_note_complete".to_string(),
            "true".to_string(),
        );
    }

    pub(crate) fn apply_windows_native_stage_runtime_metadata(
        &self,
        stage: RuntimeStage,
        stage_metadata: &mut HashMap<String, String>,
        runtime_metadata: &HashMap<String, String>,
    ) {
        const EXECUTION_KEYS: &[&str] = &[
            "windows_native_host_runtime",
            "windows_native_deployment_lane",
            "windows_native_deployment_strategy",
            "windows_native_deployment_note",
            "windows_native_product_mainline",
            "windows_native_validation_tracks",
            "windows_native_priority",
            "windows_native_small_model_runtime_target",
            "windows_native_small_model_execution_linked",
            "windows_native_small_model_execution_provider",
            "windows_native_small_model_device_target",
            "windows_native_small_model_fallback_mode",
            "windows_native_small_model_runtime_outcome",
            "windows_native_small_model_runtime_strategy",
            "windows_native_small_model_runtime_readiness",
            "windows_native_small_model_runtime_reason",
            "windows_native_main_brain_runtime_target",
            "windows_native_runtime_contract_complete",
            "windows_native_runtime_surface_note_complete",
            "engram_windows_native_embed_outcome",
            "engram_windows_native_embed_class",
            "engram_windows_native_embed_provider",
            "engram_windows_native_embed_device_target",
            "engram_windows_native_embed_fallback_mode",
            "engram_windows_native_embed_strategy",
            "engram_windows_native_embed_note",
            "engram_windows_native_rerank_outcome",
            "engram_windows_native_rerank_class",
            "engram_windows_native_rerank_provider",
            "engram_windows_native_rerank_device_target",
            "engram_windows_native_rerank_fallback_mode",
            "engram_windows_native_rerank_strategy",
            "engram_windows_native_rerank_note",
            "engram_windows_native_surface_note_present",
            "engram_windows_native_surface_note_complete",
        ];
        const TRACE_AUDIT_KEYS: &[&str] = &[
            "windows_native_runtime_contract_complete",
            "windows_native_runtime_surface_note_complete",
            "engram_windows_native_surface_note_present",
            "engram_windows_native_surface_note_complete",
        ];

        let keys = match stage {
            RuntimeStage::Execution => EXECUTION_KEYS,
            RuntimeStage::TraceAudit => TRACE_AUDIT_KEYS,
            _ => return,
        };

        for key in keys {
            if let Some(value) = runtime_metadata.get(*key) {
                stage_metadata.insert((*key).to_string(), value.clone());
            }
        }
    }
}
