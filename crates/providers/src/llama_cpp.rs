//! Llama.cpp provider implementation for local GGUF inference
//!
//! Fulfills Phase 14 of the CLAwv2 supplementary plan.
//! Redesigned to use the unified `benshu-inference` backend for robust GPU support.

use crate::utils::{
    audio_source_to_bytes, derive_child_session_id, image_source_to_bytes, resolve_root_session_id,
    video_source_to_bytes,
};
use async_trait::async_trait;
use benshu_inference::{
    GenerationConfig, InferenceConfig, KvEngine, LlamaCppBackend, ModelBackend,
};
use benshu_infra::error::{Error, Result};
use benshu_protocol_core::{AudioSource, Content, ContentPart, Role};
use benshu_provider_core::{ChatRequest, Provider, ProviderField, ProviderMetadata};
use benshu_provider_core::{FinishReason, ProviderTelemetry, StreamingChoice, StreamingResponse};
use parking_lot::RwLock;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

/// Local GGUF provider using the high-performance LlamaCppBackend from benshu-inference
pub struct LlamaCpp {
    backend: Arc<LlamaCppBackend>,
    kv_engine: Arc<RwLock<KvEngine>>,
    stt_backend: Option<Arc<dyn benshu_inference::backend::SttBackend>>,
}

impl LlamaCpp {
    /// Create from GGUF model paths (Main Brain & Optical mmproj)
    pub fn new(model_path: impl Into<PathBuf>, mmproj_path: Option<PathBuf>) -> Result<Self> {
        let backend = LlamaCppBackend::new(model_path, mmproj_path).map_err(|e| {
            Error::Internal(format!("Failed to load native LlamaCpp backend: {}", e))
        })?;

        let kv_config = InferenceConfig::default();
        let kv_engine = Arc::new(RwLock::new(KvEngine::new(kv_config)));

        Ok(Self {
            backend: Arc::new(backend),
            kv_engine,
            stt_backend: None,
        })
    }

    pub fn with_stt(mut self, stt: Arc<dyn benshu_inference::backend::SttBackend>) -> Self {
        self.stt_backend = Some(stt);
        self
    }

    fn append_provider_media_field(extra: &mut HashMap<String, String>, key: &str, value: &str) {
        if value.trim().is_empty() {
            return;
        }
        let new_value = if let Some(existing) = extra.get(key) {
            format!("{existing},{value}")
        } else {
            value.to_string()
        };
        extra.insert(key.to_string(), new_value);
    }

    fn append_provider_media_routes(
        extra: &mut HashMap<String, String>,
        key: &str,
        routes: &BTreeSet<String>,
    ) {
        if routes.is_empty() {
            return;
        }
        extra.insert(
            key.to_string(),
            routes.iter().cloned().collect::<Vec<_>>().join(","),
        );
    }

    fn requested_generation_limit(request: &ChatRequest, default_limit: u64) -> usize {
        let requested_limit = request.max_tokens.unwrap_or(default_limit);
        let shared_worker_reserve = request
            .extra_params
            .as_ref()
            .and_then(|extra| extra.get("shared_worker_response_reserve_tokens"))
            .and_then(|value| value.as_u64());
        requested_limit
            .min(shared_worker_reserve.unwrap_or(requested_limit))
            .max(64) as usize
    }
}

#[async_trait]
impl Provider for LlamaCpp {
    async fn stream_completion(&self, request: ChatRequest) -> Result<StreamingResponse> {
        let (tx_out, rx_out) = mpsc::channel(100);
        let (tx_gen, mut rx_gen) = mpsc::channel(100);
        let requested_generation_limit = Self::requested_generation_limit(&request, 512);

        let mut processed_messages = Vec::new();
        let mut images = Vec::new();
        let stt_backend = self.stt_backend.clone();
        let mut local_audio_preprocess_consumed = false;
        let mut local_video_frame_consumed = false;
        let mut provider_media_outcomes = BTreeSet::new();
        let mut provider_media_preprocess_failed_routes = BTreeSet::new();
        let mut provider_media_model_failed_routes = BTreeSet::new();
        let mut provider_media_result_insufficient_routes = BTreeSet::new();

        // 1.1 Configurable frame count
        let frame_count = request
            .extra_params
            .as_ref()
            .and_then(|e| e.get("video_frame_count"))
            .and_then(|v| v.as_u64())
            .unwrap_or(4) as usize;

        // 2. Process Multimodal content
        for msg in request.messages {
            let mut text = match &msg.content {
                Content::Parts(_) => String::new(),
                _ => msg.text(),
            };

            if let Content::Parts(parts) = &msg.content {
                for part in parts {
                    match part {
                        ContentPart::Audio { source } => {
                            if let Some(stt) = &stt_backend {
                                let mut audio_preprocess_ready = false;
                                let maybe_pcm = match audio_source_to_bytes(source).await {
                                    Ok(bytes) => {
                                        let media_type = match source {
                                            AudioSource::Base64 { media_type, .. } => {
                                                media_type.as_str()
                                            }
                                            AudioSource::Url { .. } => "audio/*",
                                        };
                                        match benshu_inference::backend::audio_preprocess::normalize_audio_bytes_to_pcm_f32(
                                            &bytes,
                                            media_type,
                                            16_000,
                                            1,
                                        )
                                        .await
                                        {
                                            Ok(pcm) => {
                                                audio_preprocess_ready = true;
                                                Some(pcm)
                                            }
                                            Err(_) => None,
                                        }
                                    }
                                    Err(error) => {
                                        tracing::warn!(
                                            "LlamaCpp: audio decode/load failed: {}",
                                            error
                                        );
                                        None
                                    }
                                };
                                if let Some(pcm) = maybe_pcm {
                                    match stt.transcribe(&pcm).await {
                                        Ok(transcription) => {
                                            local_audio_preprocess_consumed = true;
                                            if transcription.trim().is_empty() {
                                                provider_media_outcomes.insert(
                                                    "normalize_audio:model_result_insufficient"
                                                        .to_string(),
                                                );
                                                provider_media_result_insufficient_routes
                                                    .insert("normalize_audio".to_string());
                                            } else {
                                                text.push_str(&format!(
                                                    "\n[Voice Transcription: {}]\n",
                                                    transcription
                                                ));
                                            }
                                        }
                                        Err(_) => {
                                            provider_media_outcomes.insert(
                                                "normalize_audio:model_failed_after_preprocess"
                                                    .to_string(),
                                            );
                                            provider_media_model_failed_routes
                                                .insert("normalize_audio".to_string());
                                        }
                                    }
                                } else if !audio_preprocess_ready {
                                    provider_media_outcomes
                                        .insert("normalize_audio:preprocess_failed".to_string());
                                    provider_media_preprocess_failed_routes
                                        .insert("normalize_audio".to_string());
                                }
                            }
                        }
                        ContentPart::Image { source } => {
                            let image_bytes = match image_source_to_bytes(source).await {
                                Ok(bytes) => bytes,
                                Err(error) => {
                                    tracing::warn!("LlamaCpp: image decode/load failed: {}", error);
                                    Vec::new()
                                }
                            };
                            if let Ok(img) = image::load_from_memory(&image_bytes) {
                                images.push(img);
                            } else {
                                text.push_str("\n[Image Attachment]\n");
                            }
                        }
                        ContentPart::Video { source } => {
                            text.push_str("\n[Video Attachment: Processing frames...]\n");
                            let mut video_frames_loaded = false;
                            let video_bytes = match video_source_to_bytes(source).await {
                                Ok(bytes) => bytes,
                                Err(error) => {
                                    tracing::warn!("LlamaCpp: video decode/load failed: {}", error);
                                    Vec::new()
                                }
                            };
                            if !video_bytes.is_empty() {
                                match tempfile::tempdir() {
                                    Ok(temp_dir) => {
                                        let video_path = temp_dir.path().join("input.mp4");
                                        // Non-blocking async write
                                        if tokio::fs::write(&video_path, video_bytes).await.is_ok()
                                        {
                                            if let Ok(video_frames) =
                                                benshu_inference::backend::video::sample_frames(
                                                    &video_path,
                                                    frame_count,
                                                )
                                                .await
                                            {
                                                video_frames_loaded = true;
                                                if !video_frames.is_empty() {
                                                    local_video_frame_consumed = true;
                                                } else {
                                                    provider_media_outcomes.insert(
                                                        "extract_video_frames:preprocess_failed"
                                                            .to_string(),
                                                    );
                                                    provider_media_preprocess_failed_routes
                                                        .insert("extract_video_frames".to_string());
                                                }
                                                images.extend(video_frames);
                                                text.push_str(&format!(
                                                    "\n[Video Captured ({} frames)]\n",
                                                    frame_count
                                                ));
                                            }
                                        }
                                    }
                                    Err(e) => tracing::error!(
                                        "LlamaCpp: Failed to create temp dir: {}",
                                        e
                                    ),
                                }
                            }
                            if !video_frames_loaded {
                                provider_media_outcomes
                                    .insert("extract_video_frames:preprocess_failed".to_string());
                                provider_media_preprocess_failed_routes
                                    .insert("extract_video_frames".to_string());
                            }
                        }
                        ContentPart::Text { text: t } => {
                            if !t.trim().is_empty() {
                                if !text.trim().is_empty() {
                                    text.push('\n');
                                }
                                text.push_str(t);
                            }
                        }
                        _ => {}
                    }
                }
            }
            processed_messages.push((msg.role, text));
        }

        // 3. VLM Enrichment Fallback
        let mut prompt_prefix = String::new();
        let request_id = Uuid::new_v4().to_string();
        let root_session_id = resolve_root_session_id(
            request.session_id.as_deref(),
            format!("llama-cpp-{}", Uuid::new_v4()),
        );
        let backend_supports_native_vision = self.backend.supports_multimodal_vision();
        if !images.is_empty() && !backend_supports_native_vision {
            if let Ok(vision_summary) = self
                .backend
                .generate(
                    &format!("{}-vision", request_id),
                    "Describe these images or video frames in detail:",
                    Some(images.clone()),
                    GenerationConfig {
                        session_id: Some(derive_child_session_id(&root_session_id, "vision")),
                        ..Default::default()
                    },
                    self.kv_engine.clone(),
                )
                .await
            {
                if vision_summary.trim().is_empty() {
                    provider_media_outcomes
                        .insert("extract_video_frames:model_result_insufficient".to_string());
                    provider_media_result_insufficient_routes
                        .insert("extract_video_frames".to_string());
                } else {
                    prompt_prefix.push_str(&format!("\n[Vision Analysis: {}]\n", vision_summary));
                }
            } else if local_video_frame_consumed {
                provider_media_outcomes
                    .insert("extract_video_frames:model_failed_after_preprocess".to_string());
                provider_media_model_failed_routes.insert("extract_video_frames".to_string());
            }
        }

        // 3. Format prompt
        let mut prompt = String::new();
        if let Some(sys) = &request.system_prompt {
            prompt.push_str(&format!("<|system|>\n{}<|end|>\n", sys));
        }
        prompt.push_str(&prompt_prefix);
        for (role, text) in processed_messages {
            let role_name = match role {
                Role::User => "user",
                Role::Assistant => "assistant",
                _ => "user",
            };
            prompt.push_str(&format!("<|{}|>\n{}<|end|>\n", role_name, text));
        }
        prompt.push_str("<|assistant|>\n");

        let backend = self.backend.clone();
        let kv_engine = self.kv_engine.clone();
        let session_id = root_session_id;

        let config = GenerationConfig {
            max_new_tokens: requested_generation_limit,
            temperature: request.temperature.unwrap_or(0.7) as f32,
            stop_sequences: vec![
                "<|end|>".to_string(),
                "<|im_end|>".to_string(),
                "<|assistant|>".to_string(),
                "<|user|>".to_string(),
                "<|system|>".to_string(),
                "[CRITIQUE".to_string(),
                "Final Answer:".to_string(),
            ],
            session_id: Some(session_id),
            ..Default::default()
        };

        // 4. Run inference
        let model_name = self.backend.model_info();
        let started_at = Instant::now();
        tokio::spawn(async move {
            if let Err(e) = backend
                .stream_generate(
                    &request_id,
                    &prompt,
                    if backend_supports_native_vision && !images.is_empty() {
                        Some(images)
                    } else {
                        None
                    },
                    config,
                    kv_engine,
                    tx_gen,
                )
                .await
            {
                let _ = tx_out.send(Err(Error::Internal(e.to_string()))).await;
                return;
            }
            while let Some(res) = rx_gen.recv().await {
                match res {
                    Ok(text) => {
                        if tx_out
                            .send(Ok(StreamingChoice::Message(text)))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx_out.send(Err(Error::Internal(e.to_string()))).await;
                        break;
                    }
                }
            }
            let _ = tx_out
                .send(Ok(StreamingChoice::Finish(FinishReason::Stop)))
                .await;

            let mut extra = HashMap::new();
            extra.insert(
                "tool_contract_mode".to_string(),
                "prompt_json_tools".to_string(),
            );
            extra.insert("mainline_stability".to_string(), "transitional".to_string());
            if local_audio_preprocess_consumed {
                extra.insert(
                    "media_preprocess_consumed_by".to_string(),
                    "normalize_audio:stt".to_string(),
                );
                extra.insert(
                    "media_preprocess_consumption_routes".to_string(),
                    "normalize_audio:llama_cpp_local_stt".to_string(),
                );
            }
            if local_video_frame_consumed {
                Self::append_provider_media_field(
                    &mut extra,
                    "media_preprocess_consumed_by",
                    "extract_video_frames:vlm",
                );
                Self::append_provider_media_field(
                    &mut extra,
                    "media_preprocess_consumption_routes",
                    "extract_video_frames:llama_cpp_provider_vision",
                );
            }
            if !provider_media_outcomes.is_empty() {
                extra.insert(
                    "media_preprocess_outcomes".to_string(),
                    provider_media_outcomes
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            if !provider_media_preprocess_failed_routes.is_empty() {
                extra.insert(
                    "media_preprocess_preprocess_failed_routes".to_string(),
                    provider_media_preprocess_failed_routes
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            if !provider_media_model_failed_routes.is_empty() {
                extra.insert(
                    "media_preprocess_model_failed_routes".to_string(),
                    provider_media_model_failed_routes
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            if !provider_media_result_insufficient_routes.is_empty() {
                Self::append_provider_media_routes(
                    &mut extra,
                    "media_preprocess_result_insufficient_routes",
                    &provider_media_result_insufficient_routes,
                );
            }
            let mut provider_media_followup_strategies = BTreeSet::new();
            for route in &provider_media_preprocess_failed_routes {
                provider_media_followup_strategies.insert(format!("{route}:attachment_fallback"));
            }
            for route in &provider_media_model_failed_routes {
                provider_media_followup_strategies
                    .insert(format!("{route}:alternate_model_fallback"));
            }
            for route in &provider_media_result_insufficient_routes {
                provider_media_followup_strategies
                    .insert(format!("{route}:clarification_or_manual_review"));
            }
            if !provider_media_followup_strategies.is_empty() {
                extra.insert(
                    "media_preprocess_followup_strategies".to_string(),
                    provider_media_followup_strategies
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            Self::append_provider_media_routes(
                &mut extra,
                "media_preprocess_attachment_fallback_routes",
                &provider_media_preprocess_failed_routes,
            );
            Self::append_provider_media_routes(
                &mut extra,
                "media_preprocess_alternate_model_fallback_routes",
                &provider_media_model_failed_routes,
            );
            Self::append_provider_media_routes(
                &mut extra,
                "media_preprocess_clarification_routes",
                &provider_media_result_insufficient_routes,
            );
            if !provider_media_followup_strategies.is_empty()
                && provider_media_followup_strategies.len()
                    == provider_media_preprocess_failed_routes.len()
                        + provider_media_model_failed_routes.len()
                        + provider_media_result_insufficient_routes.len()
            {
                extra.insert(
                    "media_preprocess_strategy_note_complete".to_string(),
                    "true".to_string(),
                );
                extra.insert(
                    "media_preprocess_strategy_contract_complete".to_string(),
                    "true".to_string(),
                );
            }

            let _ = tx_out
                .send(Ok(StreamingChoice::Telemetry(ProviderTelemetry {
                    provider_name: Some("llama_cpp".to_string()),
                    model: Some(model_name),
                    latency_ms: Some(started_at.elapsed().as_millis() as u64),
                    continuation: None,
                    extra,
                })))
                .await;
            let _ = tx_out.send(Ok(StreamingChoice::Done)).await;
        });

        Ok(StreamingResponse::from_stream(ReceiverStream::new(rx_out)))
    }

    fn name(&self) -> &str {
        "llama_cpp"
    }

    fn is_local(&self) -> bool {
        true
    }

    fn tool_contract_mode(&self) -> &'static str {
        "prompt_json_tools"
    }

    fn mainline_stability(&self) -> &'static str {
        "transitional"
    }

    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            id: "llama_cpp".to_string(),
            name: "Llama.cpp (GGUF Mode)".to_string(),
            description:
                "High-performance local inference. Drag '.gguf' for brain and 'mmproj' for vision."
                    .to_string(),
            icon: "🧠".to_string(),
            fields: vec![
                ProviderField {
                    key: "llama_cpp_model_path".to_string(),
                    label: "Main Brain (.gguf)".to_string(),
                    field_type: "text".to_string(),
                    description: "Absolute path to your LLM file".to_string(),
                    required: true,
                    default: None,
                },
                ProviderField {
                    key: "llama_cpp_mmproj_path".to_string(),
                    label: "Vision Component (mmproj-*.gguf)".to_string(),
                    field_type: "text".to_string(),
                    description: "Optional: Load this to enable image/multimodal support"
                        .to_string(),
                    required: false,
                    default: None,
                },
            ],
            capabilities: vec![
                "streaming".to_string(),
                "gpu_acceleration".to_string(),
                "multimodal_vision".to_string(),
                "gguf_format".to_string(),
            ],
            preferred_models: vec![
                "llama3".to_string(),
                "qwen2.5".to_string(),
                "llava".to_string(),
            ],
        }
    }
}
