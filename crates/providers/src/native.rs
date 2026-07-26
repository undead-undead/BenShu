//! Native provider implementation using benshu-inference (Candle)

use crate::utils::{
    audio_source_to_bytes, derive_child_session_id, image_source_to_bytes, resolve_root_session_id,
    video_source_to_bytes,
};
use async_trait::async_trait;
use benshu_inference::{
    AccelerationProfile, GenerationConfig, GpuProbeConfidence, GpuProbeSource, GpuVendor,
    HardwareStatus, HardwareTelemetry, KvEngine, LlamaCppBackend, MemoryTopology, ModelBackend,
};
use benshu_infra::error::{Error, Result};
use benshu_infra::traits::tool::ToolDefinition;
use benshu_protocol_core::{AudioSource, Content, ContentPart, Role};
use benshu_provider_core::{ChatRequest, Provider, ProviderField, ProviderMetadata};
use benshu_provider_core::{
    FinishReason, ProviderTelemetry, StreamingChoice, StreamingResponse, Usage,
};
use parking_lot::RwLock;
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

/// Bridge from InferenceError to Brain Error
fn map_inference_error(e: benshu_inference::backend::InferenceError) -> Error {
    match e {
        benshu_inference::backend::InferenceError::NotFound(m) => {
            Error::Internal(format!("Model not found: {}", m))
        }
        benshu_inference::backend::InferenceError::Execution(m, _) => {
            Error::Internal(format!("Inference failed: {}", m))
        }
        other => Error::Internal(other.to_string()),
    }
}

/// Native inference provider with multimodal support
pub struct NativeProvider {
    backend: Arc<dyn ModelBackend>,
    kv_engine: Arc<RwLock<KvEngine>>,
    stt_backend: Option<Arc<dyn benshu_inference::backend::SttBackend>>,
    tts_backend: Option<Arc<dyn benshu_inference::backend::TtsBackend>>,
}

impl NativeProvider {
    const LOCAL_TOOL_CALL_TAG_OPEN: &'static str = "<tool_call_json>";
    const LOCAL_TOOL_CALL_TAG_CLOSE: &'static str = "</tool_call_json>";
    const LOCAL_TOOL_CONTRACT_MODE: &'static str = "tagged_json_tool_calls";
    const LOCAL_MAINLINE_STABILITY: &'static str = "stable";

    pub fn new(backend: Arc<dyn ModelBackend>, kv_engine: Arc<RwLock<KvEngine>>) -> Self {
        Self {
            backend,
            kv_engine,
            stt_backend: None,
            tts_backend: None,
        }
    }

    pub fn with_stt(mut self, stt: Arc<dyn benshu_inference::backend::SttBackend>) -> Self {
        self.stt_backend = Some(stt);
        self
    }

    pub fn with_tts(mut self, tts: Arc<dyn benshu_inference::backend::TtsBackend>) -> Self {
        self.tts_backend = Some(tts);
        self
    }

    fn static_capabilities() -> Vec<String> {
        vec![
            "paged_attention".to_string(),
            "vision".to_string(),
            "streaming".to_string(),
            "tools".to_string(),
            format!("contract:{}", Self::LOCAL_TOOL_CONTRACT_MODE),
            format!("mainline:{}", Self::LOCAL_MAINLINE_STABILITY),
        ]
    }

    fn append_provider_media_field(
        extra: &mut std::collections::HashMap<String, String>,
        key: &str,
        value: &str,
    ) {
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
        extra: &mut std::collections::HashMap<String, String>,
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

    fn runtime_capabilities(
        &self,
        status: &HardwareStatus,
        telemetry: &HardwareTelemetry,
    ) -> Vec<String> {
        let mut capabilities = Self::static_capabilities();
        capabilities.push("runtime:local".into());
        capabilities.push(format!(
            "runtime:profile:{}",
            acceleration_profile_tag(telemetry.acceleration_profile)
        ));
        capabilities.push(format!(
            "runtime:probe-confidence:{}",
            probe_confidence_tag(telemetry.gpu_probe_confidence)
        ));
        capabilities.push(format!(
            "runtime:memory-topology:{}",
            memory_topology_tag(telemetry.memory_topology)
        ));
        capabilities.push("runtime:session-authority:backend-local-cache".into());
        capabilities.push("runtime:priority-authority:backend-local-session".into());
        capabilities.push("runtime:degradation-surface:provider-metadata".into());

        if let Some(vendor) = telemetry.gpu_vendor {
            capabilities.push(format!("runtime:gpu-vendor:{}", gpu_vendor_tag(vendor)));
        }
        if let Some(source) = telemetry.gpu_probe_source {
            capabilities.push(format!("runtime:probe-source:{}", probe_source_tag(source)));
        }
        if status.cuda_available {
            capabilities.push("runtime:cuda-available".into());
        }
        if status.rocm_available {
            capabilities.push("runtime:rocm-available".into());
        }
        if status.vulkan_supported {
            capabilities.push("runtime:vulkan-available".into());
        }
        if status.supports_tensorrt() {
            capabilities.push("runtime:tensorrt-eligible".into());
        }
        if telemetry.vram_budget_mb.is_some() {
            capabilities.push("runtime:vram-budget".into());
        }
        if telemetry.shared_memory_budget_mb.is_some() {
            capabilities.push("runtime:shared-memory-budget".into());
        }

        capabilities
    }

    fn runtime_description(&self, telemetry: &HardwareTelemetry) -> String {
        let profile = acceleration_profile_tag(telemetry.acceleration_profile);
        let vendor = telemetry
            .gpu_vendor
            .map(gpu_vendor_tag)
            .unwrap_or("cpu-only");
        let probe = telemetry
            .gpu_probe_source
            .map(probe_source_tag)
            .unwrap_or("unavailable");

        format!(
            "Native local inference bridge (model={}, profile={}, vendor={}, probe={}, topology={})",
            self.backend.model_info(),
            profile,
            vendor,
            probe,
            memory_topology_tag(telemetry.memory_topology)
        )
    }

    fn backend_supports_native_vision(&self) -> bool {
        self.backend
            .as_any()
            .downcast_ref::<LlamaCppBackend>()
            .map(|backend| backend.supports_multimodal_vision())
            .unwrap_or(false)
    }

    fn request_priority(extra_params: Option<&serde_json::Value>) -> i8 {
        extra_params
            .and_then(|params| params.get("inference_priority"))
            .and_then(|value| value.as_i64())
            .map(|value| value.clamp(i8::MIN as i64, i8::MAX as i64) as i8)
            .unwrap_or(0)
    }

    fn render_local_tool_prompt(tools: &[ToolDefinition]) -> String {
        let mut content = String::from(
            "## Local Tool Calling Contract\n\
             You may call tools when they are genuinely needed.\n\
             If you decide to call a tool, do not narrate what you are about to do.\n\
             Instead, wrap the JSON payload in the exact envelope below and do not add extra prose.\n\
             Use one of these exact formats:\n\
             <tool_call_json>{\"tool_call\":{\"name\":\"tool_name\",\"arguments\":{}}}</tool_call_json>\n\
             <tool_call_json>{\"tool_calls\":[{\"name\":\"tool_name\",\"arguments\":{}}]}</tool_call_json>\n\
             Fallback compatibility: plain JSON or ```json fenced JSON is still accepted, but the tagged form is preferred.\n\
             If no tool is needed, answer the user normally.\n\
             Available tools:\n",
        );

        for tool in tools {
            content.push_str(&format!(
                "\n- {}: {}\n",
                tool.name.trim(),
                tool.description.trim()
            ));

            if let Some(guidelines) = tool.usage_guidelines.as_deref() {
                let trimmed = guidelines.trim();
                if !trimmed.is_empty() {
                    content.push_str(&format!("  Usage: {}\n", trimmed));
                }
            }

            if let Some(parameters_ts) = tool.parameters_ts.as_deref() {
                let trimmed = parameters_ts.trim();
                if !trimmed.is_empty() {
                    content.push_str("  Parameters (TypeScript):\n");
                    content.push_str("  ```ts\n");
                    for line in trimmed.lines() {
                        content.push_str("  ");
                        content.push_str(line);
                        content.push('\n');
                    }
                    content.push_str("  ```\n");
                }
            }
        }

        content
    }

    #[cfg(test)]
    fn parse_local_tool_calls(output: &str) -> Vec<(String, serde_json::Value)> {
        Self::parse_local_tool_calls_with_mode(output).0
    }

    fn parse_local_tool_calls_with_mode(
        output: &str,
    ) -> (Vec<(String, serde_json::Value)>, &'static str) {
        let trimmed = output.trim();
        if trimmed.is_empty() {
            return (Vec::new(), "empty");
        }

        let tagged_candidate = trimmed
            .split_once(Self::LOCAL_TOOL_CALL_TAG_OPEN)
            .and_then(|(_, rest)| rest.split_once(Self::LOCAL_TOOL_CALL_TAG_CLOSE))
            .map(|(json, _)| json.trim());

        let fenced_candidate = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```JSON"))
            .map(|rest| rest.trim())
            .and_then(|rest| rest.strip_suffix("```"))
            .map(str::trim);

        let (candidate, parser_mode) = if let Some(tagged) = tagged_candidate {
            (tagged, "tagged_json")
        } else if let Some(fenced) = fenced_candidate {
            (fenced, "fenced_json")
        } else {
            (trimmed, "raw_json")
        };

        let parsed = serde_json::from_str::<serde_json::Value>(candidate)
            .or_else(|_| serde_json::from_str::<serde_json::Value>(trimmed));

        let Ok(value) = parsed else {
            return (Vec::new(), "plain_text");
        };

        if let Some(single) = value.get("tool_call") {
            if let Some(parsed) = Self::parse_named_tool_call(single) {
                return (vec![parsed], parser_mode);
            }
        }

        if let Some(many) = value.get("tool_calls").and_then(|calls| calls.as_array()) {
            let parsed = many
                .iter()
                .filter_map(Self::parse_named_tool_call)
                .collect::<Vec<_>>();
            if !parsed.is_empty() {
                return (parsed, parser_mode);
            }
        }

        if let Some(parsed) = Self::parse_named_tool_call(&value) {
            return (vec![parsed], parser_mode);
        }

        (Vec::new(), parser_mode)
    }

    fn parse_named_tool_call(value: &serde_json::Value) -> Option<(String, serde_json::Value)> {
        let name = value.get("name")?.as_str()?.trim();
        if name.is_empty() {
            return None;
        }

        let arguments = value
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
        Some((name.to_string(), arguments))
    }
}

#[async_trait]
impl Provider for NativeProvider {
    async fn get_dynamic_metadata(&self) -> Result<ProviderMetadata> {
        let status = HardwareStatus::detect();
        let telemetry = status.telemetry();
        Ok(ProviderMetadata {
            id: "native".to_string(),
            name: "Native (BenShu Engine)".to_string(),
            description: self.runtime_description(&telemetry),
            icon: "🚀".to_string(),
            fields: vec![ProviderField {
                key: "model_id".to_string(),
                label: "HuggingFace Model ID".to_string(),
                field_type: "text".to_string(),
                description: "Supports Llama/Mistral/Qwen".to_string(),
                required: true,
                default: Some("meta-llama/Meta-Llama-3-8B".to_string()),
            }],
            capabilities: self.runtime_capabilities(&status, &telemetry),
            preferred_models: vec![self.backend.model_info()],
        })
    }

    async fn stream_completion(&self, request: ChatRequest) -> Result<StreamingResponse> {
        let (tx, rx) = mpsc::channel(100);
        let backend = self.backend.clone();
        let kv_engine = self.kv_engine.clone();
        let stt_backend = self.stt_backend.clone();
        let runtime_priority = Self::request_priority(request.extra_params.as_ref());
        let requested_generation_limit = Self::requested_generation_limit(&request, 1024);

        // 1. Pre-process messages (STT, OCR, etc.)
        let mut processed_messages = Vec::new();
        let mut images = Vec::new();
        let mut local_audio_preprocess_consumed = false;
        let mut local_video_frame_consumed = false;
        let mut provider_media_outcomes = BTreeSet::new();
        let mut provider_media_preprocess_failed_routes = BTreeSet::new();
        let mut provider_media_model_failed_routes = BTreeSet::new();
        let mut provider_media_result_insufficient_routes = BTreeSet::new();

        // 1.1 Configurable frame count from extra_params
        let frame_count = request
            .extra_params
            .as_ref()
            .and_then(|e| e.get("video_frame_count"))
            .and_then(|v| v.as_u64())
            .unwrap_or(4) as usize;

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
                                        tracing::warn!("Audio decode/load failed: {}", error);
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
                                    tracing::warn!("Image decode/load failed: {}", error);
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
                                    tracing::warn!("Video decode/load failed: {}", error);
                                    Vec::new()
                                }
                            };
                            if !video_bytes.is_empty() {
                                let temp_dir = tempfile::tempdir().unwrap();
                                let video_path = temp_dir.path().join("input.mp4");
                                // Async write for non-blocking IO
                                if tokio::fs::write(&video_path, video_bytes).await.is_ok() {
                                    if let Ok(video_frames) =
                                        benshu_inference::backend::video::sample_frames(
                                            &video_path,
                                            frame_count,
                                        )
                                        .await
                                    {
                                        video_frames_loaded = true;
                                        if video_frames.is_empty() {
                                            provider_media_outcomes.insert(
                                                "extract_video_frames:preprocess_failed"
                                                    .to_string(),
                                            );
                                            provider_media_preprocess_failed_routes
                                                .insert("extract_video_frames".to_string());
                                        } else {
                                            local_video_frame_consumed = true;
                                            images.extend(video_frames);
                                            text.push_str(&format!(
                                                "\n[Video Frames Extracted (Count: {}) Successfully]\n",
                                                frame_count
                                            ));
                                        }
                                    }
                                }
                                // temp_dir will be cleaned up on drop
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

        // 2. VLM Enrichment (Optional fallback)
        let mut prompt_prefix = String::new();
        let request_id = Uuid::new_v4().to_string();
        let session_id = resolve_root_session_id(
            request.session_id.as_deref(),
            format!("native-ephemeral-{}", request_id),
        );
        let backend_supports_native_vision = self.backend_supports_native_vision();

        if !images.is_empty() && !backend_supports_native_vision {
            if let Ok(vision_summary) = backend
                .generate(
                    &format!("{}-vision", request_id),
                    "Describe these images or video frames in detail:",
                    Some(images.clone()),
                    GenerationConfig {
                        session_id: Some(derive_child_session_id(&session_id, "vision")),
                        priority: runtime_priority,
                        ..Default::default()
                    },
                    kv_engine.clone(),
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
        let has_tool_contract = !request.tools.is_empty();
        if has_tool_contract {
            prompt.push_str("<|system|>\n");
            prompt.push_str(&Self::render_local_tool_prompt(&request.tools));
            prompt.push_str("<|end|>\n");
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

        // 4. Inference
        let gen_config = GenerationConfig {
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
            session_id: Some(session_id.clone()),
            priority: runtime_priority,
            ..Default::default()
        };

        let (tx_gen, mut rx_gen) = mpsc::channel(100);
        let model_name = self.backend.model_info();

        let started_at = std::time::Instant::now();
        let request_id_for_stream = request_id.clone();
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
                    gen_config,
                    kv_engine,
                    tx_gen,
                )
                .await
            {
                let _ = tx
                    .send(Err(map_inference_error_with_context(
                        e,
                        &request_id,
                        "providers::native",
                        &model_name,
                    )))
                    .await;
                return;
            }
            let input_tokens = (prompt.len() / 4) as u32;
            let mut output_tokens = 0u32;
            let mut buffered_text = String::new();

            while let Some(res) = rx_gen.recv().await {
                match res {
                    Ok(text) => {
                        output_tokens += (text.len() / 4).max(1) as u32;
                        if has_tool_contract {
                            buffered_text.push_str(&text);
                        } else if tx.send(Ok(StreamingChoice::Message(text))).await.is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(map_inference_error(e))).await;
                        return;
                    }
                }
            }

            let mut tool_call_count = 0usize;
            let mut extra_parser_mode = if has_tool_contract {
                "plain_text"
            } else {
                "disabled"
            };
            let finish_reason;

            if has_tool_contract {
                let (tool_calls, parser_mode) =
                    Self::parse_local_tool_calls_with_mode(&buffered_text);
                if tool_calls.is_empty() {
                    if !buffered_text.is_empty() {
                        let _ = tx.send(Ok(StreamingChoice::Message(buffered_text))).await;
                    }
                    let _ = tx
                        .send(Ok(StreamingChoice::Finish(FinishReason::Stop)))
                        .await;
                    extra_parser_mode = parser_mode;
                    finish_reason = FinishReason::Stop;
                } else {
                    tool_call_count = tool_calls.len();
                    for (index, (name, arguments)) in tool_calls.into_iter().enumerate() {
                        if tx
                            .send(Ok(StreamingChoice::ToolCall {
                                id: format!("native-tool-call-{}", index + 1),
                                name,
                                arguments,
                            }))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    let _ = tx
                        .send(Ok(StreamingChoice::Finish(FinishReason::ToolCalls)))
                        .await;
                    extra_parser_mode = parser_mode;
                    finish_reason = FinishReason::ToolCalls;
                }
            } else {
                let _ = tx
                    .send(Ok(StreamingChoice::Finish(FinishReason::Stop)))
                    .await;
                finish_reason = FinishReason::Stop;
            }

            let _ = tx
                .send(Ok(StreamingChoice::Usage(Usage {
                    prompt_tokens: input_tokens,
                    completion_tokens: output_tokens,
                    total_tokens: input_tokens + output_tokens,
                })))
                .await;

            let mut extra = std::collections::HashMap::new();
            extra.insert("backend".to_string(), model_name.clone());
            extra.insert("request_id".to_string(), request_id_for_stream);
            extra.insert(
                "finish_reason".to_string(),
                finish_reason.as_str().to_string(),
            );
            extra.insert("tool_call_count".to_string(), tool_call_count.to_string());
            extra.insert(
                "tool_contract_mode".to_string(),
                Self::LOCAL_TOOL_CONTRACT_MODE.to_string(),
            );
            extra.insert(
                "tool_contract_parser_mode".to_string(),
                extra_parser_mode.to_string(),
            );
            extra.insert(
                "mainline_stability".to_string(),
                Self::LOCAL_MAINLINE_STABILITY.to_string(),
            );
            if local_audio_preprocess_consumed {
                extra.insert(
                    "media_preprocess_consumed_by".to_string(),
                    "normalize_audio:stt".to_string(),
                );
                extra.insert(
                    "media_preprocess_consumption_routes".to_string(),
                    "normalize_audio:native_local_stt".to_string(),
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
                    "extract_video_frames:native_provider_vision",
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
            let _ = tx
                .send(Ok(StreamingChoice::Telemetry(ProviderTelemetry {
                    provider_name: Some("native".to_string()),
                    model: Some(model_name.clone()),
                    latency_ms: Some(started_at.elapsed().as_millis() as u64),
                    continuation: None,
                    extra,
                })))
                .await;

            let _ = tx.send(Ok(StreamingChoice::Done)).await;
        });

        Ok(StreamingResponse::from_stream(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "native"
    }

    fn is_local(&self) -> bool {
        true
    }

    fn tool_contract_mode(&self) -> &'static str {
        Self::LOCAL_TOOL_CONTRACT_MODE
    }

    fn mainline_stability(&self) -> &'static str {
        Self::LOCAL_MAINLINE_STABILITY
    }

    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            id: "native".to_string(),
            name: "Native (BenShu Engine)".to_string(),
            description: "Incremental local inference using optimized Candle/KvEngine".to_string(),
            icon: "🚀".to_string(),
            fields: vec![ProviderField {
                key: "model_id".to_string(),
                label: "HuggingFace Model ID".to_string(),
                field_type: "text".to_string(),
                description: "Supports Llama/Mistral/Qwen".to_string(),
                required: true,
                default: Some("meta-llama/Meta-Llama-3-8B".to_string()),
            }],
            capabilities: Self::static_capabilities(),
            preferred_models: vec!["llama3".to_string(), "qwen2.5".to_string()],
        }
    }
}

fn map_inference_error_with_context(
    e: benshu_inference::backend::InferenceError,
    request_id: &str,
    module: &str,
    model: &str,
) -> Error {
    match e {
        benshu_inference::backend::InferenceError::NotFound(m) => Error::Internal(format!(
            "Model not found [{} req={} model={}]: {}",
            module, request_id, model, m
        )),
        benshu_inference::backend::InferenceError::Execution(m, _) => Error::Internal(format!(
            "Inference failed [{} req={} model={}]: {}",
            module, request_id, model, m
        )),
        other => Error::Internal(format!(
            "Inference failed [{} req={} model={}]: {}",
            module, request_id, model, other
        )),
    }
}

fn gpu_vendor_tag(vendor: GpuVendor) -> &'static str {
    match vendor {
        GpuVendor::Nvidia => "nvidia",
        GpuVendor::Amd => "amd",
        GpuVendor::Apple => "apple",
        GpuVendor::Intel => "intel",
        GpuVendor::Unknown => "unknown",
    }
}

fn probe_confidence_tag(confidence: GpuProbeConfidence) -> &'static str {
    match confidence {
        GpuProbeConfidence::Native => "native",
        GpuProbeConfidence::Tooling => "tooling",
        GpuProbeConfidence::Heuristic => "heuristic",
        GpuProbeConfidence::Unavailable => "unavailable",
    }
}

fn probe_source_tag(source: GpuProbeSource) -> &'static str {
    match source {
        GpuProbeSource::Dxgi => "dxgi",
        GpuProbeSource::Wmic => "wmic",
        GpuProbeSource::NvidiaSmi => "nvidia-smi",
        GpuProbeSource::RocmSmi => "rocm-smi",
        GpuProbeSource::RocmInfo => "rocminfo",
        GpuProbeSource::Lspci => "lspci",
        GpuProbeSource::AppleUnifiedMemory => "apple-unified-memory",
    }
}

fn acceleration_profile_tag(profile: AccelerationProfile) -> &'static str {
    match profile {
        AccelerationProfile::CudaPreferred => "cuda-preferred",
        AccelerationProfile::VulkanPreferred => "vulkan-preferred",
        AccelerationProfile::MetalPreferred => "metal-preferred",
        AccelerationProfile::CpuOnly => "cpu-only",
    }
}

fn memory_topology_tag(topology: MemoryTopology) -> &'static str {
    match topology {
        MemoryTopology::DedicatedGpu => "dedicated-gpu",
        MemoryTopology::SharedGpu => "shared-gpu",
        MemoryTopology::UnifiedMemory => "unified-memory",
        MemoryTopology::CpuOnly => "cpu-only",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use benshu_inference::backend::{DeviceType, Result as InferenceResult};
    use benshu_infra::traits::tool::ToolDefinition;
    use benshu_protocol_core::Message;
    use benshu_protocol_core::{Content, ContentPart, ImageSource};
    use benshu_provider_core::StreamingChoice;
    use parking_lot::Mutex;

    struct CaptureBackend {
        seen: Arc<Mutex<Vec<GenerationConfig>>>,
    }

    #[async_trait]
    impl ModelBackend for CaptureBackend {
        async fn generate(
            &self,
            _request_id: &str,
            _prompt: &str,
            _images: Option<Vec<image::DynamicImage>>,
            config: GenerationConfig,
            _kv_engine: Arc<RwLock<KvEngine>>,
        ) -> InferenceResult<String> {
            self.seen.lock().push(config);
            Ok("ok".into())
        }

        async fn stream_generate(
            &self,
            _request_id: &str,
            _prompt: &str,
            _images: Option<Vec<image::DynamicImage>>,
            config: GenerationConfig,
            _kv_engine: Arc<RwLock<KvEngine>>,
            tx: mpsc::Sender<InferenceResult<String>>,
        ) -> InferenceResult<()> {
            self.seen.lock().push(config);
            let _ = tx.send(Ok("done".into())).await;
            Ok(())
        }

        fn model_info(&self) -> String {
            "capture-backend".into()
        }

        fn device_info(&self) -> DeviceType {
            DeviceType::Cpu
        }

        fn estimated_memory_usage(&self) -> u64 {
            0
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[tokio::test]
    async fn native_provider_uses_ephemeral_session_when_request_has_none() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let backend = Arc::new(CaptureBackend { seen: seen.clone() });
        let kv = Arc::new(RwLock::new(KvEngine::new(Default::default())));
        let provider = NativeProvider::new(backend, kv);

        let response = provider
            .stream_completion(ChatRequest {
                messages: vec![Message::user("hello")],
                ..Default::default()
            })
            .await
            .expect("stream completion");
        let _ = response.collect_text().await.expect("collect text");

        let configs = seen.lock();
        let config = configs.last().expect("captured config");
        let session = config.session_id.as_ref().expect("session id");
        assert!(session.starts_with("native-ephemeral-"));
    }

    #[tokio::test]
    async fn native_provider_forwards_runtime_priority() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let backend = Arc::new(CaptureBackend { seen: seen.clone() });
        let kv = Arc::new(RwLock::new(KvEngine::new(Default::default())));
        let provider = NativeProvider::new(backend, kv);

        let response = provider
            .stream_completion(ChatRequest {
                messages: vec![Message::user("hello")],
                extra_params: Some(serde_json::json!({ "inference_priority": -21 })),
                ..Default::default()
            })
            .await
            .expect("stream completion");
        let _ = response.collect_text().await.expect("collect text");

        let configs = seen.lock();
        let config = configs.last().expect("captured config");
        assert_eq!(config.priority, -21);
    }

    #[tokio::test]
    async fn native_provider_uses_stable_child_session_for_vision_fallback() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let backend = Arc::new(CaptureBackend { seen: seen.clone() });
        let kv = Arc::new(RwLock::new(KvEngine::new(Default::default())));
        let provider = NativeProvider::new(backend, kv);

        let png_bytes = {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                1,
                1,
                image::Rgb([0, 0, 0]),
            ));
            let mut bytes = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut bytes);
            image
                .write_to(&mut cursor, image::ImageFormat::Png)
                .expect("encode png");
            bytes
        };

        let response = provider
            .stream_completion(ChatRequest {
                messages: vec![Message::user(Content::Parts(vec![ContentPart::Image {
                    source: ImageSource::Base64 {
                        data: base64::prelude::BASE64_STANDARD.encode(png_bytes),
                        media_type: "image/png".to_string(),
                    },
                }]))],
                session_id: Some("session-root".to_string()),
                ..Default::default()
            })
            .await
            .expect("stream completion");
        let _ = response.collect_text().await.expect("collect text");

        let configs = seen.lock();
        assert_eq!(configs.len(), 2, "expected vision fallback + main stream");
        let vision_session = configs[0].session_id.as_ref().expect("vision session id");
        let root_session = configs[1].session_id.as_ref().expect("root session id");
        assert_eq!(vision_session, "session-root::vision");
        assert_eq!(root_session, "session-root");
    }

    #[test]
    fn native_runtime_capabilities_expose_governance_contract() {
        let backend = Arc::new(CaptureBackend {
            seen: Arc::new(Mutex::new(Vec::new())),
        });
        let kv = Arc::new(RwLock::new(KvEngine::new(Default::default())));
        let provider = NativeProvider::new(backend, kv);
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("NVIDIA GeForce RTX 4090".into()),
            gpu_vendor: Some(GpuVendor::Nvidia),
            gpu_probe_confidence: GpuProbeConfidence::Native,
            gpu_probe_source: Some(GpuProbeSource::Dxgi),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: Some(20 * 1024),
            vram_used_mb: 0,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
            vulkan_supported: true,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: true,
            rocm_available: false,
            gpu_compute_capability: Some((8, 9)),
        };
        let telemetry = status.telemetry();
        let capabilities = provider.runtime_capabilities(&status, &telemetry);

        assert!(capabilities.contains(&"runtime:session-authority:backend-local-cache".to_string()));
        assert!(
            capabilities.contains(&"runtime:priority-authority:backend-local-session".to_string())
        );
        assert!(capabilities.contains(&"runtime:degradation-surface:provider-metadata".to_string()));
        assert!(capabilities.contains(&"runtime:gpu-vendor:nvidia".to_string()));
    }

    #[test]
    fn native_provider_parses_local_tool_call_contract() {
        let calls = NativeProvider::parse_local_tool_calls(
            r#"{"tool_call":{"name":"shell","arguments":{"command":"echo hi"}}}"#,
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "shell");
        assert_eq!(calls[0].1["command"], "echo hi");
    }

    struct ToolCallBackend;

    #[async_trait]
    impl ModelBackend for ToolCallBackend {
        async fn generate(
            &self,
            _request_id: &str,
            _prompt: &str,
            _images: Option<Vec<image::DynamicImage>>,
            _config: GenerationConfig,
            _kv_engine: Arc<RwLock<KvEngine>>,
        ) -> InferenceResult<String> {
            Ok("ok".into())
        }

        async fn stream_generate(
            &self,
            _request_id: &str,
            _prompt: &str,
            _images: Option<Vec<image::DynamicImage>>,
            _config: GenerationConfig,
            _kv_engine: Arc<RwLock<KvEngine>>,
            tx: mpsc::Sender<InferenceResult<String>>,
        ) -> InferenceResult<()> {
            let _ = tx
                .send(Ok(
                    r#"{"tool_call":{"name":"shell","arguments":{"command":"echo hi"}}}"#
                        .to_string(),
                ))
                .await;
            Ok(())
        }

        fn model_info(&self) -> String {
            "tool-call-backend".into()
        }

        fn device_info(&self) -> DeviceType {
            DeviceType::Cpu
        }

        fn estimated_memory_usage(&self) -> u64 {
            0
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[tokio::test]
    async fn native_provider_emits_tool_contract_when_tools_are_present() {
        let backend = Arc::new(ToolCallBackend);
        let kv = Arc::new(RwLock::new(KvEngine::new(Default::default())));
        let provider = NativeProvider::new(backend, kv);

        let mut stream = provider
            .stream_completion(ChatRequest {
                messages: vec![Message::user("run a command")],
                tools: vec![ToolDefinition {
                    name: "shell".to_string(),
                    description: "Run shell commands".to_string(),
                    parameters: serde_json::json!({"type":"object"}),
                    parameters_ts: None,
                    is_binary: false,
                    is_verified: false,
                    safety_level: benshu_infra::agent::SafetyLevel::Yellow,
                    usage_guidelines: None,
                }],
                ..Default::default()
            })
            .await
            .expect("stream completion")
            .into_inner();

        use futures::StreamExt;
        let mut saw_tool_call = false;
        let mut saw_finish = false;
        let mut saw_telemetry = false;
        while let Some(chunk) = stream.next().await {
            match chunk.expect("chunk") {
                StreamingChoice::ToolCall {
                    name, arguments, ..
                } => {
                    saw_tool_call = true;
                    assert_eq!(name, "shell");
                    assert_eq!(arguments["command"], "echo hi");
                }
                StreamingChoice::Finish(FinishReason::ToolCalls) => {
                    saw_finish = true;
                }
                StreamingChoice::Telemetry(telemetry) => {
                    saw_telemetry = true;
                    assert!(telemetry.continuation.is_none());
                    assert_eq!(
                        telemetry.extra.get("finish_reason").map(String::as_str),
                        Some("tool_calls")
                    );
                    assert_eq!(
                        telemetry.extra.get("tool_call_count").map(String::as_str),
                        Some("1")
                    );
                    assert_eq!(
                        telemetry
                            .extra
                            .get("tool_contract_mode")
                            .map(String::as_str),
                        Some("tagged_json_tool_calls")
                    );
                    assert_eq!(
                        telemetry
                            .extra
                            .get("tool_contract_parser_mode")
                            .map(String::as_str),
                        Some("raw_json")
                    );
                    assert_eq!(
                        telemetry
                            .extra
                            .get("mainline_stability")
                            .map(String::as_str),
                        Some("stable")
                    );
                }
                _ => {}
            }
        }

        assert!(saw_tool_call);
        assert!(saw_finish);
        assert!(saw_telemetry);
    }

    #[test]
    fn parse_local_tool_calls_accepts_tagged_payload() {
        let calls = NativeProvider::parse_local_tool_calls(
            r#"<tool_call_json>{"tool_calls":[{"name":"shell","arguments":{"command":"echo hi"}}]}</tool_call_json>"#,
        );

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "shell");
        assert_eq!(calls[0].1["command"], "echo hi");
    }
}
