use std::path::Path;
use std::sync::Arc;

use crate::tool::media_runtime::{normalize_audio_bytes_for_stt, sample_video_frames_for_analysis};
use crate::tool::office_parse::OfficeParseTool;
use crate::tool::pdf_parse::{PdfOutput, PdfParseTool};
use crate::tool::ToolCleanup;
use async_trait::async_trait;
use base64::Engine;
use benshu_brain::agent::message::{
    AudioSource, Content, ContentPart, ImageSource, Message, Role, VideoSource,
};
use benshu_brain::agent::provider::{ChatRequest, Provider};
use benshu_compression::{head_with_notice, TruncationNotice};
use benshu_infra::bus::{MediaAttachment, MediaType};
use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};
use benshu_sensory::vision::{VisionPlugin, WasmOCR};
use benshu_sensory::{SensoryHub, SensoryInput, SensoryOutput, SensoryRequest};
use serde::Deserialize;
use serde_json::json;
use tracing::warn;

const MAX_ATTACHMENT_CONTEXT_CHARS: usize = 6000;
const MAX_TEXT_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
const MAX_INLINE_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

struct PreparedInput {
    kind: &'static str,
    resolved_path: String,
    _tempdir: Option<tempfile::TempDir>,
}

pub struct DocumentUnderstandTool {
    provider: Option<Arc<dyn Provider>>,
    model: Option<String>,
    sensory: Arc<SensoryHub>,
    prefer_prime_multimodal_for_visual_ingress: bool,
}

impl DocumentUnderstandTool {
    const OCR_MULTIMODAL_FALLBACK_PROMPT: &'static str =
        "Extract any visible text from this image as faithfully as possible. \
If there is little or no readable text, briefly explain that the text is unclear or not present.";

    pub fn new(
        provider: Option<Arc<dyn Provider>>,
        model: Option<String>,
        sensory: Arc<SensoryHub>,
    ) -> Self {
        Self {
            provider,
            model,
            sensory,
            prefer_prime_multimodal_for_visual_ingress: false,
        }
    }

    pub fn with_prime_multimodal_visual_ingress(mut self, enabled: bool) -> Self {
        self.prefer_prime_multimodal_for_visual_ingress = enabled;
        self
    }

    fn should_defer_ingress_attachment_to_prime_multimodal(&self, path: &str, goal: &str) -> bool {
        if !self.prefer_prime_multimodal_for_visual_ingress {
            return false;
        }

        if matches!(goal, "transcribe" | "parse_document") {
            return false;
        }

        let resolved_path = Self::resolve_local_file_uri(path).unwrap_or_else(|| path.to_string());
        matches!(Self::infer_kind(&resolved_path), "image" | "video")
    }

    fn infer_kind(path: &str) -> &'static str {
        match Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif") => "image",
            Some("pdf") => "pdf",
            Some("docx" | "xlsx" | "pptx") => "office",
            Some(
                "txt" | "md" | "markdown" | "json" | "rs" | "toml" | "yaml" | "yml" | "js" | "ts"
                | "tsx" | "py" | "sh" | "html" | "css" | "xml" | "csv",
            ) => "text",
            Some("mp3" | "wav" | "ogg" | "m4a" | "flac" | "aac") => "audio",
            Some("mp4" | "mov" | "avi" | "mkv" | "webm") => "video",
            _ => "unknown",
        }
    }

    fn resolve_local_file_uri(path: &str) -> Option<String> {
        if !path.starts_with("file://") {
            return None;
        }

        let url = reqwest::Url::parse(path).ok()?;
        let local_path = url.to_file_path().ok()?;
        Some(local_path.to_string_lossy().to_string())
    }

    async fn prepare_input(&self, path: &str) -> anyhow::Result<PreparedInput> {
        let resolved_local_path =
            Self::resolve_local_file_uri(path).unwrap_or_else(|| path.to_string());

        if tokio::fs::try_exists(&resolved_local_path)
            .await
            .unwrap_or(false)
        {
            return Ok(PreparedInput {
                kind: Self::infer_kind(&resolved_local_path),
                resolved_path: resolved_local_path,
                _tempdir: None,
            });
        }

        if path.starts_with("http://") || path.starts_with("https://") {
            let url = reqwest::Url::parse(path)?;
            let guessed_kind = Self::infer_kind(url.path());
            let response = reqwest::get(url.clone()).await?.error_for_status()?;
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let kind = match guessed_kind {
                "unknown" if content_type.starts_with("image/") => "image",
                "unknown" if content_type.contains("pdf") => "pdf",
                "unknown"
                    if content_type.contains("officedocument")
                        || content_type.contains("msword")
                        || content_type.contains("spreadsheet")
                        || content_type.contains("presentation") =>
                {
                    "office"
                }
                "unknown" if content_type.starts_with("text/") => "text",
                "unknown" if content_type.contains("json") || content_type.contains("xml") => {
                    "text"
                }
                "unknown" if content_type.starts_with("audio/") => "audio",
                "unknown" if content_type.starts_with("video/") => "video",
                _ => guessed_kind,
            };

            let extension = match kind {
                "pdf" => "pdf",
                "office" if url.path().ends_with(".docx") => "docx",
                "office" if url.path().ends_with(".xlsx") => "xlsx",
                "office" if url.path().ends_with(".pptx") => "pptx",
                "office" => "docx",
                "text" if url.path().ends_with(".md") => "md",
                "text" if url.path().ends_with(".json") => "json",
                "text" if url.path().ends_with(".xml") => "xml",
                "text" if url.path().ends_with(".csv") => "csv",
                "text" => "txt",
                "image" if content_type.contains("png") => "png",
                "image" if content_type.contains("webp") => "webp",
                "image" if content_type.contains("bmp") => "bmp",
                "image" if content_type.contains("gif") => "gif",
                "image" => "jpg",
                "audio" if content_type.contains("wav") => "wav",
                "audio" if content_type.contains("ogg") => "ogg",
                "audio" if content_type.contains("flac") => "flac",
                "audio" if content_type.contains("aac") => "aac",
                "audio" => "mp3",
                "video" if content_type.contains("quicktime") => "mov",
                "video" if content_type.contains("x-matroska") => "mkv",
                "video" if content_type.contains("webm") => "webm",
                "video" if content_type.contains("x-msvideo") => "avi",
                "video" => "mp4",
                _ => {
                    anyhow::bail!(
                        "Unsupported remote input type for document_understand: {}",
                        path
                    )
                }
            };

            let tempdir = tempfile::tempdir()?;
            let filename = tempdir.path().join(format!("document_input.{}", extension));
            let bytes = response.bytes().await?;
            tokio::fs::write(&filename, &bytes).await?;

            return Ok(PreparedInput {
                kind,
                resolved_path: filename.to_string_lossy().to_string(),
                _tempdir: Some(tempdir),
            });
        }

        anyhow::bail!("File not found: {}", path)
    }

    fn infer_goal(goal: Option<&str>, kind: &str) -> &'static str {
        match goal.unwrap_or("auto") {
            "understand" => "understand",
            "extract_text" => "extract_text",
            "transcribe" => "transcribe",
            _ => match kind {
                "image" => "understand",
                "pdf" | "office" | "text" => "parse_document",
                "audio" => "transcribe",
                "video" => "understand",
                _ => "understand",
            },
        }
    }

    fn model_name(&self, override_model: Option<&str>) -> Option<String> {
        override_model
            .map(ToOwned::to_owned)
            .or_else(|| self.model.clone())
    }

    fn display_model_name(&self, override_model: Option<&str>) -> String {
        self.model_name(override_model)
            .unwrap_or_else(|| "not_configured".to_string())
    }

    fn cleanup_for_prepared_input(prepared: &PreparedInput) -> ToolCleanup {
        if prepared._tempdir.is_some() {
            ToolCleanup::active(
                "ephemeral_remote_input_cache",
                "document_understand_remote_input_is_temp",
                "Remote input was downloaded into a temporary directory for routing and will be removed automatically after the call finishes.",
                "none",
                true,
            )
        } else {
            ToolCleanup::inactive()
        }
    }

    async fn analyze_dynamic_image(
        &self,
        image: image::DynamicImage,
        prompt: &str,
        model_override: Option<&str>,
        local_only: bool,
    ) -> anyhow::Result<(String, String)> {
        if !local_only {
            if let Some(provider) = &self.provider {
                match self
                    .analyze_with_provider(Arc::clone(provider), &image, prompt, model_override)
                    .await
                {
                    Ok(text) => return Ok(("provider_vision".to_string(), text)),
                    Err(error) => {
                        warn!(
                            "document_understand provider vision failed during multimodal analysis: {}. Local in-process sensory llama.cpp fallback has been removed.",
                            error
                        );
                    }
                }
            }
        }

        anyhow::bail!(
            "No provider/bridge-backed multimodal vision runtime is available. The WSL in-process local_sensory_vlm llama.cpp backend has been removed."
        )
    }

    async fn route_pdf(&self, args: &DocumentUnderstandArgs) -> anyhow::Result<serde_json::Value> {
        let goal = Self::infer_goal(args.goal.as_deref(), "pdf");
        let enrichment_ready = PdfParseTool::in_process_enrichment_ready();
        let parser = PdfParseTool::new(
            self.provider.clone(),
            args.model.clone().or_else(|| self.model.clone()),
            self.sensory.clone(),
        );
        let raw = parser
            .call(
                &json!({
                    "path": args.path,
                    "mode": "auto",
                    "format": "json"
                })
                .to_string(),
            )
            .await?;

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
            if parsed.get("error").is_some() {
                return Ok(json!({
                    "status": "error",
                    "input_kind": "pdf",
                    "goal": goal,
                    "route": "pdf_parse_tool",
                    "path": args.path,
                    "pdf_enrichment_ready": enrichment_ready,
                    "error": parsed.get("error").cloned().unwrap_or_else(|| json!("pdf_parse failed")),
                }));
            }
        }

        let parsed: PdfOutput = serde_json::from_str(&raw)?;
        let markdown = PdfParseTool::to_markdown(&parsed);

        Ok(json!({
            "status": "ok",
            "input_kind": "pdf",
            "goal": goal,
            "route": "pdf_parse_tool",
            "path": args.path,
            "pdf_enrichment_ready": enrichment_ready,
            "parser_mode": parsed.parser_mode,
            "page_routes": parsed.page_routes,
            "result_format": "markdown",
            "result": markdown,
        }))
    }

    async fn route_office(
        &self,
        args: &DocumentUnderstandArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let goal = Self::infer_goal(args.goal.as_deref(), "office");
        let parsed = OfficeParseTool::parse_path(&args.path)?;
        let markdown = OfficeParseTool::to_markdown(&parsed);

        Ok(json!({
            "status": "ok",
            "input_kind": "office",
            "goal": goal,
            "route": "office_parse_tool",
            "path": args.path,
            "document_type": parsed.document_type,
            "parser_mode": "office_open_xml",
            "warnings": parsed.warnings,
            "result_format": "markdown",
            "result": markdown,
        }))
    }

    async fn route_text(&self, args: &DocumentUnderstandArgs) -> anyhow::Result<serde_json::Value> {
        let goal = Self::infer_goal(args.goal.as_deref(), "text");
        let metadata = tokio::fs::metadata(&args.path).await?;
        if metadata.len() > MAX_TEXT_ATTACHMENT_BYTES {
            anyhow::bail!(
                "text attachment is larger than the 20MB single-file safety limit: {} bytes",
                metadata.len()
            );
        }
        let raw = tokio::fs::read_to_string(&args.path).await?;
        let result = truncate_attachment_context(&raw);

        Ok(json!({
            "status": "ok",
            "input_kind": "text",
            "goal": goal,
            "route": "plain_text_attachment",
            "path": args.path,
            "parser_mode": "utf8_text",
            "result_format": "text",
            "result": result,
        }))
    }

    async fn route_image_understanding(
        &self,
        args: &DocumentUnderstandArgs,
    ) -> anyhow::Result<serde_json::Value> {
        self.route_image_understanding_path(
            &args.path,
            args.prompt.as_deref(),
            args.model.as_deref(),
            args.local_only,
        )
        .await
    }

    async fn route_image_ocr(
        &self,
        args: &DocumentUnderstandArgs,
    ) -> anyhow::Result<serde_json::Value> {
        self.route_image_ocr_path(
            &args.path,
            args.backend.as_deref(),
            args.model.as_deref(),
            args.local_only,
        )
        .await
    }

    async fn route_image_understanding_path(
        &self,
        path: &str,
        prompt: Option<&str>,
        model_override: Option<&str>,
        local_only: bool,
    ) -> anyhow::Result<serde_json::Value> {
        let image = image::open(path)?;
        let ocr_hint = self.best_effort_image_ocr_hint(&image).await;
        let (route, result) = self
            .analyze_dynamic_image(
                image.clone(),
                prompt.unwrap_or("Understand this image and summarize the important information."),
                model_override,
                local_only,
            )
            .await?;

        let trimmed = result.trim();
        if trimmed.is_empty() {
            return Ok(json!({
                "status": "error",
                "input_kind": "image",
                "goal": "understand",
                "route": route,
                "path": path,
                "model": self.display_model_name(model_override),
                "error": "Local vision model returned an empty understanding result.",
            }));
        }

        let enriched_result =
            if Self::should_prefer_ocr_rescue_summary(trimmed, ocr_hint.as_deref()) {
                Self::ocr_rescue_summary(ocr_hint.as_deref().unwrap_or_default())
            } else {
                Self::merge_visual_understanding_with_ocr(trimmed, ocr_hint.as_deref())
            };

        Ok(json!({
            "status": "ok",
            "input_kind": "image",
            "goal": "understand",
            "route": route,
            "path": path,
            "model": self.display_model_name(model_override),
            "ocr_hint": ocr_hint,
            "result": enriched_result,
        }))
    }

    async fn route_image_ocr_path(
        &self,
        path: &str,
        backend_override: Option<&str>,
        model_override: Option<&str>,
        local_only: bool,
    ) -> anyhow::Result<serde_json::Value> {
        let img = image::open(path)?;
        match self
            .recognize_text_best_effort(&img, backend_override)
            .await
        {
            Ok(text) => {
                if !text.trim().is_empty() {
                    return Ok(json!({
                        "status": "ok",
                        "input_kind": "image",
                        "goal": "extract_text",
                        "route": "ocr_backend",
                        "path": path,
                        "media_preprocess_route": "image_page_raster",
                        "media_preprocess_source_kind": "direct_image",
                        "media_preprocess_source_ref": path,
                        "media_pipeline_outcome": "success",
                        "media_preprocess_consumed": true,
                        "media_preprocess_consumer": "ocr",
                        "media_preprocess_consumer_route": "ocr_backend",
                        "adaptive_strategy": "ocr_first_multimodal_fallback",
                        "backend": backend_override.unwrap_or("best_effort_ocr"),
                        "result": text,
                    }));
                }

                match self
                    .analyze_dynamic_image(
                        img,
                        Self::OCR_MULTIMODAL_FALLBACK_PROMPT,
                        model_override,
                        local_only,
                    )
                    .await
                {
                    Ok((fallback_route, fallback_text)) => Ok(json!({
                        "status": "ok",
                        "input_kind": "image",
                        "goal": "extract_text",
                        "route": format!("ocr_backend_with_{}_fallback", fallback_route),
                        "path": path,
                        "media_preprocess_route": "image_page_raster",
                        "media_preprocess_source_kind": "direct_image",
                        "media_preprocess_source_ref": path,
                        "media_pipeline_outcome": "alternate_model_fallback_after_insufficient_ocr",
                        "media_preprocess_consumed": true,
                        "media_preprocess_consumer": "ocr_then_multimodal",
                        "media_preprocess_consumer_route": "ocr_backend_then_multimodal_fallback",
                        "adaptive_strategy": "ocr_first_multimodal_fallback",
                        "fallback_trigger": "model_result_insufficient",
                        "fallback_route": fallback_route,
                        "backend": backend_override.unwrap_or("best_effort_ocr"),
                        "result": fallback_text,
                    })),
                    Err(fallback_error) => Ok(json!({
                        "status": "error",
                        "input_kind": "image",
                        "goal": "extract_text",
                        "route": "ocr_backend_multimodal_fallback_unavailable",
                        "path": path,
                        "media_preprocess_route": "image_page_raster",
                        "media_preprocess_source_kind": "direct_image",
                        "media_preprocess_source_ref": path,
                        "media_pipeline_outcome": "fallback_unavailable_after_insufficient_ocr",
                        "media_preprocess_consumed": true,
                        "media_preprocess_consumer": "ocr_then_multimodal",
                        "media_preprocess_consumer_route": "ocr_backend_then_multimodal_fallback",
                        "adaptive_strategy": "ocr_first_multimodal_fallback",
                        "fallback_trigger": "model_result_insufficient",
                        "backend": backend_override.unwrap_or("best_effort_ocr"),
                        "error": fallback_error.to_string(),
                    })),
                }
            }
            Err(error) => {
                match self
                    .analyze_dynamic_image(
                        img,
                        Self::OCR_MULTIMODAL_FALLBACK_PROMPT,
                        model_override,
                        local_only,
                    )
                    .await
                {
                    Ok((fallback_route, fallback_text)) => Ok(json!({
                        "status": "ok",
                        "input_kind": "image",
                        "goal": "extract_text",
                        "route": format!("ocr_backend_with_{}_fallback", fallback_route),
                        "path": path,
                        "media_preprocess_route": "image_page_raster",
                        "media_preprocess_source_kind": "direct_image",
                        "media_preprocess_source_ref": path,
                        "media_pipeline_outcome": "alternate_model_fallback_after_ocr_failure",
                        "media_preprocess_consumed": true,
                        "media_preprocess_consumer": "ocr_then_multimodal",
                        "media_preprocess_consumer_route": "ocr_backend_then_multimodal_fallback",
                        "adaptive_strategy": "ocr_first_multimodal_fallback",
                        "fallback_trigger": "model_failed_after_preprocess",
                        "fallback_route": fallback_route,
                        "backend": backend_override.unwrap_or("best_effort_ocr"),
                        "error": format!("OCR failed: {}", error),
                        "result": fallback_text,
                    })),
                    Err(fallback_error) => Ok(json!({
                        "status": "error",
                        "input_kind": "image",
                        "goal": "extract_text",
                        "route": "ocr_backend_multimodal_fallback_unavailable",
                        "path": path,
                        "media_preprocess_route": "image_page_raster",
                        "media_preprocess_source_kind": "direct_image",
                        "media_preprocess_source_ref": path,
                        "media_pipeline_outcome": "fallback_unavailable_after_ocr_failure",
                        "media_preprocess_consumed": true,
                        "media_preprocess_consumer": "ocr_then_multimodal",
                        "media_preprocess_consumer_route": "ocr_backend_then_multimodal_fallback",
                        "adaptive_strategy": "ocr_first_multimodal_fallback",
                        "fallback_trigger": "model_failed_after_preprocess",
                        "backend": backend_override.unwrap_or("best_effort_ocr"),
                        "error": format!("OCR failed: {}; fallback unavailable: {}", error, fallback_error),
                    })),
                }
            }
        }
    }

    async fn route_audio_transcription(
        &self,
        args: &DocumentUnderstandArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let bytes = match normalize_audio_bytes_for_stt(Path::new(&args.path), 16_000, 1).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(json!({
                    "status": "error",
                    "input_kind": "audio",
                    "goal": "transcribe",
                    "route": "media_runtime_audio_stt",
                    "path": args.path,
                    "media_preprocess_route": "normalize_audio",
                    "media_pipeline_outcome": "preprocess_failed",
                    "error": format!("Audio normalization failed: {}", error),
                }));
            }
        };
        let request = SensoryRequest::Audio {
            input: SensoryInput::Audio(bytes),
            plugin_hint: args.backend.clone(),
        };

        match self.sensory.dispatch(request).await? {
            SensoryOutput::Text(text) => {
                let outcome = if text.trim().is_empty() {
                    "model_result_insufficient"
                } else {
                    "success"
                };
                Ok(json!({
                "status": "ok",
                "input_kind": "audio",
                "goal": "transcribe",
                "route": "media_runtime_audio_stt",
                "path": args.path,
                "media_preprocess_route": "normalize_audio",
                "media_pipeline_outcome": outcome,
                "media_preprocess_consumed": true,
                "media_preprocess_consumer": "stt",
                "media_preprocess_consumer_route": "media_runtime_audio_stt",
                "normalized_audio_contract": {
                    "sample_rate": 16000,
                    "channels": 1,
                },
                "backend": args.backend.clone().unwrap_or_else(|| "sensory_default_stt".to_string()),
                "result": text,
                }))
            }
            other => Ok(json!({
                "status": "error",
                "input_kind": "audio",
                "goal": "transcribe",
                "route": "media_runtime_audio_stt",
                "path": args.path,
                "media_preprocess_route": "normalize_audio",
                "media_pipeline_outcome": "model_failed_after_preprocess",
                "error": format!("Unexpected sensory audio response: {:?}", other),
            })),
        }
    }

    fn video_frame_source_contracts(frame_count: usize) -> Vec<serde_json::Value> {
        (0..frame_count)
            .map(|index| {
                json!({
                    "source_contract_kind": "video_frame_image",
                    "source_contract_ref": format!("video_frame:{}", index + 1),
                })
            })
            .collect()
    }

    async fn route_video_ocr(
        &self,
        args: &DocumentUnderstandArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let frame_count = args.frame_count.unwrap_or(4).max(1);
        let frame_source_contracts = Self::video_frame_source_contracts(frame_count);
        let frames =
            match sample_video_frames_for_analysis(Path::new(&args.path), frame_count).await {
                Ok(frames) => frames,
                Err(error) => {
                    return Ok(json!({
                        "status": "error",
                        "input_kind": "video",
                        "goal": "extract_text",
                        "route": "media_runtime_video_frames_ocr",
                        "path": args.path,
                        "frame_count": frame_count,
                        "frame_source_contracts": frame_source_contracts,
                        "media_preprocess_route": "extract_video_frames",
                        "media_pipeline_outcome": "preprocess_failed",
                        "error": format!("Video frame extraction failed: {}", error),
                    }));
                }
            };

        if frames.is_empty() {
            return Ok(json!({
                "status": "error",
                "input_kind": "video",
                "goal": "extract_text",
                "route": "media_runtime_video_frames_ocr",
                "path": args.path,
                "frame_count": frame_count,
                "frame_source_contracts": frame_source_contracts,
                "media_preprocess_route": "extract_video_frames",
                "media_pipeline_outcome": "preprocess_failed",
                "error": "No frames could be extracted from video input",
            }));
        }

        let backend_name = args.backend.as_deref().unwrap_or("auto");

        let mut frame_texts = Vec::new();
        let mut frame_errors = Vec::new();

        for (index, frame) in frames.into_iter().enumerate() {
            match self
                .recognize_text_best_effort(&frame, Some(backend_name))
                .await
            {
                Ok(text) => {
                    if !text.trim().is_empty() {
                        frame_texts.push(format!("Frame {}:\n{}", index + 1, text.trim()));
                    }
                }
                Err(error) => frame_errors.push(format!("frame {}: {}", index + 1, error)),
            }
        }

        let combined_result = frame_texts.join("\n\n");
        if !combined_result.trim().is_empty() {
            return Ok(json!({
                "status": "ok",
                "input_kind": "video",
                "goal": "extract_text",
                "route": "media_runtime_video_frames_ocr",
                "path": args.path,
                "frame_count": frame_count,
                "frame_source_contracts": frame_source_contracts,
                "media_preprocess_route": "extract_video_frames",
                "media_preprocess_source_kind": "video_frame_image",
                "media_preprocess_source_ref": "video_frame:1",
                "media_pipeline_outcome": "success",
                "media_preprocess_consumed": true,
                "media_preprocess_consumer": "ocr",
                "media_preprocess_consumer_route": "media_runtime_video_frames_ocr",
                "adaptive_strategy": "ocr_first_multimodal_fallback",
                "backend": backend_name,
                "frame_errors": frame_errors,
                "result": combined_result,
            }));
        }

        let fallback_prompt =
            "Extract any visible text from this representative video frame. If little or no readable text exists, say so briefly.";
        let mut frame_summaries = Vec::new();
        let mut frame_routes = Vec::new();
        let fallback_frames =
            sample_video_frames_for_analysis(Path::new(&args.path), frame_count).await?;
        for frame in fallback_frames {
            let (route, summary) = self
                .analyze_dynamic_image(
                    frame,
                    fallback_prompt,
                    args.model.as_deref(),
                    args.local_only,
                )
                .await?;
            frame_routes.push(route);
            if !summary.trim().is_empty() {
                frame_summaries.push(summary.trim().to_string());
            }
        }
        let fallback_route = "media_runtime_video_frames_provider_vision_fallback";
        let fallback_trigger = if frame_errors.is_empty() {
            "model_result_insufficient"
        } else {
            "model_failed_after_preprocess"
        };

        Ok(json!({
            "status": "ok",
            "input_kind": "video",
            "goal": "extract_text",
            "route": fallback_route,
            "path": args.path,
            "frame_count": frame_count,
            "frame_source_contracts": frame_source_contracts,
            "media_preprocess_route": "extract_video_frames",
            "media_preprocess_source_kind": "video_frame_image",
            "media_preprocess_source_ref": "video_frame:1",
            "media_pipeline_outcome": format!("alternate_model_fallback_after_{}", fallback_trigger),
            "media_preprocess_consumed": true,
            "media_preprocess_consumer": "ocr_then_multimodal",
            "media_preprocess_consumer_route": "media_runtime_video_frames_ocr_then_multimodal_fallback",
            "adaptive_strategy": "ocr_first_multimodal_fallback",
            "fallback_trigger": fallback_trigger,
            "frame_errors": frame_errors,
            "frame_routes": frame_routes,
            "backend": backend_name,
            "result": frame_summaries.join("\n\n"),
        }))
    }

    async fn route_video_understanding(
        &self,
        args: &DocumentUnderstandArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let frame_count = args.frame_count.unwrap_or(4).max(1);
        let frames =
            match sample_video_frames_for_analysis(Path::new(&args.path), frame_count).await {
                Ok(frames) => frames,
                Err(error) => {
                    return Ok(json!({
                        "status": "error",
                        "input_kind": "video",
                        "goal": "understand",
                        "route": "media_runtime_video_frame_sampling",
                        "path": args.path,
                        "media_preprocess_route": "extract_video_frames",
                        "media_pipeline_outcome": "preprocess_failed",
                        "error": format!("Video frame extraction failed: {}", error),
                    }));
                }
            };

        if frames.is_empty() {
            return Ok(json!({
                "status": "error",
                "input_kind": "video",
                "goal": "understand",
                "route": "media_runtime_video_frame_sampling",
                "path": args.path,
                "media_preprocess_route": "extract_video_frames",
                "media_pipeline_outcome": "preprocess_failed",
                "error": "No frames could be extracted from video input",
            }));
        }

        let mut frame_summaries = Vec::new();
        let mut frame_routes = Vec::new();
        let prompt = args
            .prompt
            .as_deref()
            .unwrap_or("Summarize this representative video frame for the current task.");

        for (index, frame) in frames.into_iter().enumerate() {
            let (route, summary) = self
                .analyze_dynamic_image(frame, prompt, args.model.as_deref(), args.local_only)
                .await?;
            frame_routes.push(route);
            frame_summaries.push(format!("Frame {}:\n{}", index + 1, summary.trim()));
        }

        let aggregate_route = "media_runtime_video_frames_provider_vision";
        let combined_result = frame_summaries.join("\n\n");
        let outcome = if combined_result.trim().is_empty() {
            "model_result_insufficient"
        } else {
            "success"
        };

        Ok(json!({
            "status": "ok",
            "input_kind": "video",
            "goal": "understand",
            "route": aggregate_route,
            "path": args.path,
            "frame_count": frame_count,
            "frame_routes": frame_routes,
            "media_preprocess_route": "extract_video_frames",
            "media_pipeline_outcome": outcome,
            "media_preprocess_consumed": true,
            "media_preprocess_consumer": "vlm",
            "media_preprocess_consumer_route": aggregate_route,
            "result": combined_result,
        }))
    }

    async fn analyze_with_provider(
        &self,
        provider: Arc<dyn Provider>,
        image: &image::DynamicImage,
        prompt: &str,
        model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        image.write_to(&mut buffer, image::ImageFormat::Png)?;
        let base64_data = base64::engine::general_purpose::STANDARD.encode(buffer.into_inner());

        let request = ChatRequest {
            model: self.model_name(model_override).ok_or_else(|| {
                anyhow::anyhow!(
                    "document_understand needs an explicitly configured vision model; refusing to default to a cloud model"
                )
            })?,
            messages: vec![Message::new(
                Role::User,
                Content::Parts(vec![
                    ContentPart::Text {
                        text: prompt.to_string(),
                    },
                    ContentPart::Image {
                        source: ImageSource::Base64 {
                            media_type: "image/png".to_string(),
                            data: base64_data,
                        },
                    },
                ]),
            )],
            ..Default::default()
        };

        let stream = provider.stream_completion(request).await?;
        Ok(stream.collect_text().await?)
    }

    fn sensory_to_text(output: SensoryOutput) -> String {
        match output {
            SensoryOutput::Text(text) => text,
            SensoryOutput::Coordinates { x, y, label } => format!(
                "Point of Interest: [{}, {}] - Context: {}",
                x,
                y,
                label.unwrap_or_else(|| "unlabeled".to_string())
            ),
            _ => "[Unsupported sensory output]".to_string(),
        }
    }

    async fn best_effort_image_ocr_hint(&self, image: &image::DynamicImage) -> Option<String> {
        let text = self.recognize_text_best_effort(image, None).await.ok()?;
        let normalized = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    }

    async fn recognize_text_best_effort(
        &self,
        image: &image::DynamicImage,
        backend_override: Option<&str>,
    ) -> anyhow::Result<String> {
        let requested_backend = backend_override.unwrap_or("auto").trim();

        if requested_backend.eq_ignore_ascii_case("auto") || requested_backend.is_empty() {
            if let Ok(output) = self
                .sensory
                .vision_check(image.clone(), None, Some("global-ocr"))
                .await
            {
                if let SensoryOutput::Text(text) = output {
                    if !text.trim().is_empty() {
                        return Ok(text);
                    }
                }
            }
        }

        if !requested_backend.eq_ignore_ascii_case("auto")
            && !requested_backend.eq_ignore_ascii_case("tesseract")
            && !requested_backend.is_empty()
        {
            let ocr_backend = benshu_inference::backend::InferenceFactory::create_ocr_backend(
                Path::new(requested_backend),
            )
            .await?;
            return Ok(ocr_backend.recognize(image).await?);
        }

        if which::which("tesseract").is_ok() {
            let ocr_backend = benshu_inference::backend::InferenceFactory::create_ocr_backend(
                Path::new("tesseract"),
            )
            .await?;
            return Ok(ocr_backend.recognize(image).await?);
        }

        let wasm_ocr = WasmOCR::new()?;
        match wasm_ocr.process(image, Some("eng+chi_sim")).await? {
            SensoryOutput::Text(text) => Ok(text),
            other => anyhow::bail!("Unexpected OCR output: {:?}", other),
        }
    }

    fn merge_visual_understanding_with_ocr(visual: &str, ocr_hint: Option<&str>) -> String {
        let visual = visual.trim();
        let Some(ocr_hint) = ocr_hint.map(str::trim).filter(|hint| !hint.is_empty()) else {
            return visual.to_string();
        };

        let lowered_visual = visual.to_lowercase();
        let lowered_ocr = ocr_hint.to_lowercase();
        if lowered_visual.contains(&lowered_ocr) {
            return visual.to_string();
        }

        format!("{visual} 可见文字可能是：{ocr_hint}")
    }

    fn should_prefer_ocr_rescue_summary(visual: &str, ocr_hint: Option<&str>) -> bool {
        let Some(ocr_hint) = ocr_hint.map(str::trim).filter(|hint| !hint.is_empty()) else {
            return false;
        };

        let lowered = visual.trim().to_lowercase();
        let low_value_markers = [
            "请再试一次",
            "多模态交付没有稳定落成",
            "直接回答图片里有什么",
            "如果看不清",
            "如果确实无法判断",
            "你是一个有用的助手",
            "describe the image",
            "cannot see the image",
            "cannot view the image",
        ];

        low_value_markers
            .iter()
            .any(|marker| lowered.contains(marker))
            && !ocr_hint.is_empty()
    }

    fn ocr_rescue_summary(ocr_hint: &str) -> String {
        format!("这张图片里有可读文字，可见文字可能是：{ocr_hint}")
    }
}

fn truncate_attachment_context(text: &str) -> String {
    let trimmed = text.trim();
    head_with_notice(
        trimmed,
        MAX_ATTACHMENT_CONTEXT_CHARS,
        TruncationNotice::DocumentUnderstand,
    )
    .content
}

fn should_defer_attachment_to_prime_multimodal(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .to_ascii_lowercase()
        .contains("no vision plugins registered")
}

pub fn attachment_fallback(label: &str, url: &str) -> ContentPart {
    let display_source = display_attachment_source(url);
    ContentPart::Text {
        text: format!("\n[{} Attachment: {}]", label, display_source),
    }
}

fn display_attachment_source(url: &str) -> String {
    if url.starts_with("file://") {
        let name = reqwest::Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.to_file_path().ok())
            .and_then(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned)
            });
        return name
            .map(|name| format!("[local-file:{}]", name))
            .unwrap_or_else(|| "[local-file]".to_string());
    }

    let path = Path::new(url);
    if path.is_absolute() {
        return path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("[local-file:{}]", name))
            .unwrap_or_else(|| "[local-file]".to_string());
    } else {
        url.to_string()
    }
}

fn infer_local_media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

async fn normalize_image_source(url: &str) -> ImageSource {
    let path = if url.starts_with("file://") {
        let parsed = match reqwest::Url::parse(url) {
            Ok(parsed) => parsed,
            Err(_) => {
                return ImageSource::Url {
                    url: url.to_string(),
                };
            }
        };

        match parsed.to_file_path() {
            Ok(path) => path,
            Err(_) => {
                return ImageSource::Url {
                    url: url.to_string(),
                };
            }
        }
    } else {
        let path = Path::new(url);
        if path.is_absolute() && path.exists() {
            path.to_path_buf()
        } else {
            return ImageSource::Url {
                url: url.to_string(),
            };
        }
    };

    if tokio::fs::metadata(&path)
        .await
        .map(|metadata| metadata.len() > MAX_INLINE_IMAGE_BYTES)
        .unwrap_or(false)
    {
        warn!(
            path = %path.display(),
            "local image is too large to inline into provider request; falling back to URL reference"
        );
        return ImageSource::Url {
            url: url.to_string(),
        };
    }

    match tokio::fs::read(&path).await {
        Ok(bytes) => ImageSource::Base64 {
            media_type: infer_local_media_type(&path).to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        },
        Err(_) => ImageSource::Url {
            url: url.to_string(),
        },
    }
}

pub fn build_attachment_context(url: &str, value: &serde_json::Value) -> Option<ContentPart> {
    let status_ok = value
        .get("status")
        .and_then(|value| value.as_str())
        .map(|value| value == "ok")
        .unwrap_or(false);
    if !status_ok {
        return None;
    }

    let result = value.get("result").and_then(|value| value.as_str())?.trim();
    if result.is_empty() {
        return None;
    }

    let input_kind = value
        .get("input_kind")
        .and_then(|value| value.as_str())
        .unwrap_or("document");
    let route = value
        .get("route")
        .and_then(|value| value.as_str())
        .unwrap_or("document_understand");
    let parser_mode = value
        .get("parser_mode")
        .and_then(|value| value.as_str())
        .map(|value| format!("\nparser_mode: {}", value))
        .unwrap_or_default();
    let context = truncate_attachment_context(result);
    let display_source = display_attachment_source(url);

    Some(ContentPart::Text {
        text: format!(
            "\n[Parsed {} Attachment via {}]\nsource: {}\n{}{}",
            input_kind, route, display_source, context, parser_mode
        ),
    })
}

async fn preprocess_attachment_context(
    document_router: &DocumentUnderstandTool,
    url: &str,
    goal: &str,
) -> Option<ContentPart> {
    if document_router.should_defer_ingress_attachment_to_prime_multimodal(url, goal) {
        tracing::info!(
            "document_understand explicitly deferred {} to the prime multimodal brain based on ingress routing policy",
            url
        );
        return None;
    }

    let args = serde_json::json!({
        "action": "analyze",
        "path": url,
        "goal": goal
    })
    .to_string();

    match document_router.call(&args).await {
        Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(value) => build_attachment_context(url, &value),
            Err(error) => {
                warn!(
                    "Failed to decode document_understand response for {}: {}",
                    url, error
                );
                None
            }
        },
        Err(error) => {
            if should_defer_attachment_to_prime_multimodal(&error) {
                tracing::info!(
                    "document_understand deferred attachment {} directly to the prime multimodal brain because no standalone vision plugin is registered",
                    url
                );
                return None;
            }
            warn!(
                "document_understand failed for attachment {}: {}",
                url, error
            );
            None
        }
    }
}

pub async fn normalize_media_attachments(
    document_router: Arc<DocumentUnderstandTool>,
    media: Option<Vec<MediaAttachment>>,
) -> Vec<ContentPart> {
    let mut parts = Vec::new();

    if let Some(media) = media {
        for attachment in media {
            match attachment.media_type {
                MediaType::Image => {
                    if let Some(part) = preprocess_attachment_context(
                        document_router.as_ref(),
                        &attachment.url,
                        "understand",
                    )
                    .await
                    {
                        parts.push(part);
                    }
                    parts.push(ContentPart::Image {
                        source: normalize_image_source(&attachment.url).await,
                    });
                }
                MediaType::Document => {
                    if let Some(part) = preprocess_attachment_context(
                        document_router.as_ref(),
                        &attachment.url,
                        "auto",
                    )
                    .await
                    {
                        parts.push(part);
                    } else {
                        parts.push(attachment_fallback("Document", &attachment.url));
                    }
                }
                MediaType::Voice => {
                    if let Some(part) = preprocess_attachment_context(
                        document_router.as_ref(),
                        &attachment.url,
                        "transcribe",
                    )
                    .await
                    {
                        parts.push(part);
                    } else {
                        parts.push(attachment_fallback("Audio", &attachment.url));
                    }
                    parts.push(ContentPart::Audio {
                        source: AudioSource::Url {
                            url: attachment.url,
                        },
                    });
                }
                MediaType::Video => {
                    if let Some(part) = preprocess_attachment_context(
                        document_router.as_ref(),
                        &attachment.url,
                        "understand",
                    )
                    .await
                    {
                        parts.push(part);
                    } else {
                        parts.push(attachment_fallback("Video", &attachment.url));
                    }
                    parts.push(ContentPart::Video {
                        source: VideoSource::Url {
                            url: attachment.url,
                        },
                    });
                }
            }
        }
    }

    parts
}

#[derive(Deserialize)]
struct DocumentUnderstandArgs {
    action: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    goal: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    local_only: bool,
    #[serde(default)]
    frame_count: Option<usize>,
}

#[async_trait]
impl Tool for DocumentUnderstandTool {
    fn name(&self) -> String {
        "document_understand".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "document_understand".to_string(),
            description: "Unified multimodal understanding router. Routes image, PDF, Office, text, audio, and video inputs through a single entry point.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["analyze", "info"], "description": "Run document routing or inspect available routes." },
                    "path": { "type": "string", "description": "Path or URL to an image, PDF, Office, text, audio, or video file." },
                    "goal": { "type": "string", "enum": ["auto", "understand", "extract_text", "transcribe"], "description": "Task intent. 'auto' lets the router choose." },
                    "prompt": { "type": "string", "description": "Optional task-specific instruction for image understanding." },
                    "backend": { "type": "string", "description": "Optional OCR backend override. Defaults to the globally configured OCR route; explicit values may request a specific backend such as 'tesseract'." },
                    "model": { "type": "string", "description": "Optional provider or VLM model override." },
                    "local_only": { "type": "boolean", "description": "Prefer local VLM/OCR and skip cloud/provider routing.", "default": false },
                    "frame_count": { "type": "integer", "description": "Optional number of representative video frames to sample.", "default": 4 }
                },
                "required": ["action"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this as the default multimodal entry point for chat attachments. It will route images, PDFs, Office files, text files, audio, and video through provider, sensory, OCR, STT, or parsing paths as appropriate without importing them into the knowledge base.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: DocumentUnderstandArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: "document_understand".into(),
                message: e.to_string(),
            })?;

        let response = match args.action.as_str() {
            "info" => json!({
                "supported_inputs": ["image", "pdf", "office", "text", "audio", "video"],
                "supported_goals": ["understand", "extract_text", "transcribe"],
                "image_routes": [
                    "provider_vision",
                    "ocr_backend"
                ],
                "audio_routes": [
                    "media_runtime_audio_stt"
                ],
                "video_routes": [
                    "media_runtime_video_frames_ocr",
                    "media_runtime_video_frames_provider_vision"
                ],
                "pdf_routes": [
                    "pdf_parse_tool"
                ],
                "office_routes": [
                    "office_parse_tool"
                ],
                "text_routes": [
                    "plain_text_attachment"
                ],
                "pdf_parser_ready": true,
                "pdf_enrichment_ready": PdfParseTool::in_process_enrichment_ready(),
                "video_render_ready": which::which("ffmpeg").is_ok(),
                "media_runtime_routes": [
                    "normalize_audio",
                    "extract_video_frames",
                    "render_video_thumbnail",
                    "probe_media"
                ],
                "cleanup": ToolCleanup::active(
                    "ephemeral_remote_input_cache",
                    "document_understand_remote_input_is_temp",
                    "When you pass a remote URL, document_understand downloads it into a temporary directory for routing and removes the temporary copy automatically after the call finishes.",
                    "none",
                    true,
                ).as_json(),
                "notes": [
                    "Images can be understood through provider vision or local sensory fallback.",
                    "Image OCR is routed through a local OCR backend.",
                    "Audio is normalized through media runtime before entering the sensory STT path.",
                    "Video is sampled through media runtime frame extraction before provider vision or local sensory analysis.",
                    "Video text extraction is routed through media runtime frame extraction before OCR.",
                    "PDF parsing is routed through the dedicated pdf_parse tool.",
                    "Office parsing is routed through the dedicated office_parse tool for .docx, .xlsx, and .pptx.",
                    "Plain text and source files are read as transient chat context.",
                    "pdf_parse prefers native text layers and uses in-process page-image enrichment when a page is image-dominant."
                ]
            }),
            "analyze" => {
                if args.path.is_empty() {
                    json!({"status": "error", "error": "path is required"})
                } else {
                    let prepared = self.prepare_input(&args.path).await?;
                    let cleanup = Self::cleanup_for_prepared_input(&prepared).as_json();
                    let kind = prepared.kind;
                    let path = prepared.resolved_path;
                    let goal = Self::infer_goal(args.goal.as_deref(), kind);
                    let routed_args = DocumentUnderstandArgs { path, ..args };
                    let mut response = match (kind, goal) {
                        ("image", "extract_text") => self.route_image_ocr(&routed_args).await?,
                        ("image", _) => self.route_image_understanding(&routed_args).await?,
                        ("audio", "transcribe") => {
                            self.route_audio_transcription(&routed_args).await?
                        }
                        ("audio", _) => self.route_audio_transcription(&routed_args).await?,
                        ("video", "extract_text") => self.route_video_ocr(&routed_args).await?,
                        ("video", _) => self.route_video_understanding(&routed_args).await?,
                        ("pdf", _) => self.route_pdf(&routed_args).await?,
                        ("office", _) => self.route_office(&routed_args).await?,
                        ("text", _) => self.route_text(&routed_args).await?,
                        _ => json!({
                            "status": "error",
                            "path": routed_args.path,
                            "error": "Unsupported input type. document_understand currently supports images, PDF, Office, text, audio, and video."
                        }),
                    };
                    if let Some(object) = response.as_object_mut() {
                        object.insert("cleanup".to_string(), cleanup);
                    }
                    response
                }
            }
            _ => json!({"status": "error", "error": format!("Unknown action: {}", args.action)}),
        };

        Ok(serde_json::to_string_pretty(&response)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::TOOL_CLEANUP_SCHEMA_VERSION;
    use benshu_brain::agent::message::ContentPart;
    use benshu_brain::agent::streaming::MockStreamBuilder;
    use benshu_brain::testing::SequenceMockProvider;
    use benshu_sensory::hub::SensoryConfig;
    use benshu_sensory::protocol::FallbackPolicy;
    use tempfile::tempdir;

    fn sensory() -> Arc<SensoryHub> {
        Arc::new(SensoryHub::new(SensoryConfig {
            fallback_policy: FallbackPolicy::Error,
            ..Default::default()
        }))
    }

    #[tokio::test]
    async fn document_understand_info_reports_pdf_tool() {
        let tool = DocumentUnderstandTool::new(None, None, sensory());
        let result = tool.call(r#"{"action":"info"}"#).await.expect("call");
        let json: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert!(json["pdf_routes"].as_array().is_some());
        assert!(json["pdf_routes"]
            .as_array()
            .expect("pdf_routes array")
            .iter()
            .any(|item| item == "pdf_parse_tool"));
        assert!(json["audio_routes"]
            .as_array()
            .expect("audio_routes array")
            .iter()
            .any(|item| item == "media_runtime_audio_stt"));
        assert!(json["video_routes"]
            .as_array()
            .expect("video_routes array")
            .iter()
            .any(|item| item == "media_runtime_video_frames_provider_vision"));
        assert!(json["media_runtime_routes"]
            .as_array()
            .expect("media_runtime_routes array")
            .iter()
            .any(|item| item == "normalize_audio"));
        assert_eq!(
            json["cleanup"]["schema_version"].as_str(),
            Some(TOOL_CLEANUP_SCHEMA_VERSION)
        );
    }

    #[tokio::test]
    async fn document_understand_uses_provider_for_image_understanding() {
        let temp = tempdir().expect("tempdir");
        let image_path = temp.path().join("sample.png");
        image::DynamicImage::new_rgba8(2, 2)
            .save(&image_path)
            .expect("save image");

        let provider: Arc<dyn Provider> =
            Arc::new(SequenceMockProvider::new(vec![MockStreamBuilder::new()
                .message("provider vision response")
                .done()
                .build()]));
        let tool =
            DocumentUnderstandTool::new(Some(provider), Some("test-model".to_string()), sensory());
        let args = json!({
            "action": "analyze",
            "path": image_path.to_string_lossy().to_string(),
            "goal": "understand"
        });
        let result = tool.call(&args.to_string()).await.expect("call");
        let json: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(json["route"], "provider_vision");
        assert_eq!(json["result"], "provider vision response");
    }

    #[test]
    fn build_attachment_context_is_bounded_and_annotated() {
        let value = json!({
            "status": "ok",
            "input_kind": "audio",
            "route": "sensory_audio_stt",
            "result": "transcribed text"
        });

        let part = build_attachment_context("https://example.com/demo.mp3", &value)
            .expect("attachment context");

        let text = match part {
            ContentPart::Text { text } => text,
            _ => panic!("expected text content part"),
        };

        assert!(text.contains("[Parsed audio Attachment via sensory_audio_stt]"));
        assert!(text.contains("source: https://example.com/demo.mp3"));
        assert!(text.contains("transcribed text"));
    }

    #[test]
    fn cleanup_contract_marks_remote_inputs_as_ephemeral() {
        let prepared = PreparedInput {
            kind: "image",
            resolved_path: "/tmp/document_input.png".to_string(),
            _tempdir: Some(tempdir().expect("tempdir")),
        };

        let cleanup = DocumentUnderstandTool::cleanup_for_prepared_input(&prepared);
        assert_eq!(cleanup.schema_version, TOOL_CLEANUP_SCHEMA_VERSION);
        assert!(cleanup.active);
        assert_eq!(cleanup.reason, "document_understand_remote_input_is_temp");
        assert!(cleanup.auto_cleanup_performed);
    }

    #[tokio::test]
    async fn direct_image_ocr_reports_direct_image_source_contract() {
        let tool = DocumentUnderstandTool::new(
            None,
            None,
            Arc::new(SensoryHub::new(benshu_sensory::SensoryConfig::default())),
        );
        let tempdir = tempdir().expect("tempdir");
        let image_path = tempdir.path().join("sample.png");
        image::DynamicImage::new_rgba8(4, 4)
            .save(&image_path)
            .expect("write image");

        let value = tool
            .route_image_ocr_path(
                image_path.to_str().expect("utf8 path"),
                Some("missing-backend"),
                None,
                false,
            )
            .await
            .expect("ocr value");

        assert_eq!(
            value
                .get("media_preprocess_source_kind")
                .and_then(|v| v.as_str()),
            Some("direct_image")
        );
        assert_eq!(
            value
                .get("media_preprocess_source_ref")
                .and_then(|v| v.as_str()),
            image_path.to_str()
        );
    }

    #[tokio::test]
    async fn prepare_input_normalizes_file_uri_paths() {
        let tool = DocumentUnderstandTool::new(None, None, sensory());
        let temp = tempdir().expect("tempdir");
        let image_path = temp.path().join("sample.png");
        image::DynamicImage::new_rgba8(2, 2)
            .save(&image_path)
            .expect("save image");

        let file_uri = format!("file://{}", image_path.to_string_lossy());
        let prepared = tool.prepare_input(&file_uri).await.expect("prepare_input");

        assert_eq!(prepared.kind, "image");
        assert_eq!(prepared.resolved_path, image_path.to_string_lossy());
    }

    #[tokio::test]
    async fn preprocess_attachment_context_defers_when_prime_multimodal_should_handle_image() {
        let tool = DocumentUnderstandTool::new(None, None, sensory());
        let temp = tempdir().expect("tempdir");
        let image_path = temp.path().join("sample.png");
        image::DynamicImage::new_rgba8(2, 2)
            .save(&image_path)
            .expect("save image");

        let file_uri = format!("file://{}", image_path.to_string_lossy());
        let part = preprocess_attachment_context(&tool, &file_uri, "understand").await;

        assert!(
            part.is_none(),
            "gateway ingress should defer image understanding to the prime multimodal brain when no standalone vision plugin is registered"
        );
    }

    #[test]
    fn video_frame_source_contracts_mark_frame_image_origin() {
        let contracts = DocumentUnderstandTool::video_frame_source_contracts(3);
        assert_eq!(contracts.len(), 3);
        assert_eq!(
            contracts[0]
                .get("source_contract_kind")
                .and_then(|v| v.as_str()),
            Some("video_frame_image")
        );
        assert_eq!(
            contracts[0]
                .get("source_contract_ref")
                .and_then(|v| v.as_str()),
            Some("video_frame:1")
        );
        assert_eq!(
            contracts[2]
                .get("source_contract_ref")
                .and_then(|v| v.as_str()),
            Some("video_frame:3")
        );
    }
}
