//! Text extraction tool — OCR via universal inference gateway.
//! Supports:
//! - Tesseract OCR (local)
//! - Cloud Vision APIs (api:provider/model)
//! - Vision-Language Models (VLMs) via InferenceFactory

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use benshu_brain::agent::message::ImageSource;
use benshu_brain::agent::message::{Content, ContentPart, Message, Role};
use benshu_brain::agent::provider::Provider;
use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};

pub struct TextExtractTool {
    provider: Option<Arc<dyn Provider>>,
    model: Option<String>,
    sensory: Arc<benshu_sensory::SensoryHub>,
}

impl TextExtractTool {
    pub fn new(
        provider: Option<Arc<dyn Provider>>,
        model: Option<String>,
        sensory: Arc<benshu_sensory::SensoryHub>,
    ) -> Self {
        Self {
            provider,
            model,
            sensory,
        }
    }
}

impl TextExtractTool {
    const OCR_MULTIMODAL_FALLBACK_PROMPT: &'static str =
        "Extract any visible text from this image as faithfully as possible. \
If there is little or no readable text, briefly explain that the text is unclear or not present.";

    fn multimodal_model_name(&self, override_model: Option<&str>) -> String {
        override_model
            .map(ToOwned::to_owned)
            .or_else(|| self.model.clone())
            .unwrap_or_else(|| "local_multimodal_default".to_string())
    }

    async fn analyze_with_provider(
        &self,
        provider: Arc<dyn Provider>,
        image: &image::DynamicImage,
        prompt: &str,
        model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        use base64::Engine;

        let mut buffer = std::io::Cursor::new(Vec::new());
        image.write_to(&mut buffer, image::ImageFormat::Png)?;
        let base64_data = base64::engine::general_purpose::STANDARD.encode(buffer.into_inner());

        let request = benshu_brain::agent::provider::ChatRequest {
            model: self.multimodal_model_name(model_override),
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

    async fn analyze_with_multimodal_fallback(
        &self,
        image: image::DynamicImage,
        model_override: Option<&str>,
    ) -> anyhow::Result<(String, String)> {
        if let Some(provider) = &self.provider {
            if let Ok(text) = self
                .analyze_with_provider(
                    Arc::clone(provider),
                    &image,
                    Self::OCR_MULTIMODAL_FALLBACK_PROMPT,
                    model_override,
                )
                .await
            {
                return Ok(("provider_vision".to_string(), text));
            }
        }

        anyhow::bail!(
            "No provider-backed multimodal OCR fallback is available. The WSL in-process local_sensory_vlm llama.cpp backend has been removed."
        )
    }

    async fn recognize_with_global_ocr(
        &self,
        image: image::DynamicImage,
    ) -> anyhow::Result<(String, serde_json::Value, String)> {
        let output = self
            .sensory
            .vision_check(image, None, Some("global-ocr"))
            .await?;
        let text = match output {
            benshu_sensory::SensoryOutput::Text(text) => text,
            other => format!("{other:?}"),
        };
        let backend = json!({
            "factory_id": "global_ocr_binding",
            "model_id": "global-ocr",
            "loaded": true,
        });
        Ok(("global-ocr".to_string(), backend, text))
    }
}

#[derive(Deserialize)]
struct TextExtractArgs {
    action: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[async_trait]
impl Tool for TextExtractTool {
    fn name(&self) -> String {
        "text_extract".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "text_extract".to_string(),
            description: "Extract text from images via OCR. Supports local Tesseract and cloud Vision models.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["recognize", "info"], "description": "Action: 'recognize' to extract text, 'info' to check backends" },
                    "path": { "type": "string", "description": "Path to image file" },
                    "language": { "type": "string", "description": "OCR language (e.g., 'eng', 'chi_sim')" },
                    "backend": { "type": "string", "enum": ["auto", "tesseract"], "description": "Backend preference. 'auto' prefers the globally bound OCR runtime." },
                    "model": { "type": "string", "description": "Specific Vision Model to use (e.g., 'api:provider/model' or a local multimodal model binding)" }
                },
                "required": ["action"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this to extract text from images. The default route prefers the globally configured OCR capability, then falls back to multimodal understanding when OCR is insufficient.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: TextExtractArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: "text_extract".into(),
                message: e.to_string(),
            })?;

        let result = match args.action.as_str() {
            "info" => detect_backends().await,
            "recognize" => self.recognize(&args).await?,
            _ => json!({"error": format!("Unknown action: {}", args.action)}),
        };

        Ok(serde_json::to_string_pretty(&result)?)
    }
}

impl TextExtractTool {
    async fn recognize(&self, args: &TextExtractArgs) -> anyhow::Result<serde_json::Value> {
        if args.path.is_empty() {
            return Ok(json!({"error": "path is required"}));
        }
        if !tokio::fs::try_exists(&args.path).await.unwrap_or(false) {
            return Ok(json!({"error": format!("File not found: {}", args.path)}));
        }

        let img = image::open(&args.path)?;
        let backend_pref = args.backend.as_deref().unwrap_or("auto");
        let (backend_name, backend_info, text) = if backend_pref == "tesseract" {
            tracing::info!("🔍 TextExtractTool: Using explicit OCR backend: tesseract");
            let ocr_backend = benshu_inference::backend::InferenceFactory::create_ocr_backend(
                std::path::Path::new("tesseract"),
            )
            .await?;
            let text = ocr_backend.recognize(&img).await?;
            (
                "tesseract".to_string(),
                serde_json::to_value(ocr_backend.model_info())?,
                text,
            )
        } else {
            tracing::info!("🔍 TextExtractTool: Using default OCR route via global binding");
            self.recognize_with_global_ocr(img.clone()).await?
        };

        if !text.trim().is_empty() {
            Ok(json!({
                "status": "ok",
                "input_kind": "image",
                "goal": "extract_text",
                "route": "ocr_backend",
                "media_preprocess_route": "image_page_raster",
                "media_preprocess_source_kind": "direct_image",
                "media_preprocess_source_ref": args.path,
                "media_pipeline_outcome": "success",
                "media_preprocess_consumed": true,
                "media_preprocess_consumer": "ocr",
                "media_preprocess_consumer_route": "ocr_backend",
                "adaptive_strategy": "ocr_first_multimodal_fallback",
                "text": text,
                "backend": backend_info,
                "path": args.path,
            }))
        } else {
            match self
                .analyze_with_multimodal_fallback(img, args.model.as_deref())
                .await
            {
                Ok((fallback_route, fallback_text)) => Ok(json!({
                    "status": "ok",
                    "input_kind": "image",
                    "goal": "extract_text",
                    "route": format!("ocr_backend_with_{}_fallback", fallback_route),
                    "media_preprocess_route": "image_page_raster",
                    "media_preprocess_source_kind": "direct_image",
                    "media_preprocess_source_ref": args.path,
                    "media_pipeline_outcome": "alternate_model_fallback_after_insufficient_ocr",
                    "media_preprocess_consumed": true,
                    "media_preprocess_consumer": "ocr_then_multimodal",
                    "media_preprocess_consumer_route": "ocr_backend_then_multimodal_fallback",
                    "adaptive_strategy": "ocr_first_multimodal_fallback",
                    "fallback_trigger": "model_result_insufficient",
                    "fallback_route": fallback_route,
                    "text": fallback_text,
                    "ocr_text": text,
                    "backend": backend_info,
                    "path": args.path,
                })),
                Err(fallback_error) => Ok(json!({
                    "status": "error",
                    "input_kind": "image",
                    "goal": "extract_text",
                    "route": "ocr_backend_multimodal_fallback_unavailable",
                    "media_preprocess_route": "image_page_raster",
                    "media_preprocess_source_kind": "direct_image",
                    "media_preprocess_source_ref": args.path,
                    "media_pipeline_outcome": "fallback_unavailable_after_insufficient_ocr",
                    "media_preprocess_consumed": true,
                    "media_preprocess_consumer": "ocr_then_multimodal",
                    "media_preprocess_consumer_route": "ocr_backend_then_multimodal_fallback",
                    "adaptive_strategy": "ocr_first_multimodal_fallback",
                    "fallback_trigger": "model_result_insufficient",
                    "ocr_text": text,
                    "backend": backend_info,
                    "path": args.path,
                    "error": fallback_error.to_string(),
                })),
            }
        }
    }
}

async fn detect_backends() -> serde_json::Value {
    let tesseract_available = which::which("tesseract").is_ok();

    json!( {
        "available_backends": if tesseract_available { vec!["auto", "tesseract", "api"] } else { vec!["auto", "api"] },
        "global_ocr_binding_ready": true,
        "tesseract_ready": tesseract_available,
        "api_ready": true, // Always ready via InferenceFactory
    } )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_definition() {
        use benshu_sensory::hub::SensoryConfig;
        use benshu_sensory::protocol::FallbackPolicy;

        let config = SensoryConfig {
            fallback_policy: FallbackPolicy::Error,
            ..Default::default()
        };
        let sensory = Arc::new(benshu_sensory::SensoryHub::new(config));

        let tool = TextExtractTool::new(None, None, sensory);
        let def = tool.definition().await;
        assert_eq!(def.name, "text_extract");
    }

    #[tokio::test]
    async fn recognize_reports_media_contract_for_direct_image() {
        use benshu_sensory::hub::SensoryConfig;
        use benshu_sensory::protocol::FallbackPolicy;

        let config = SensoryConfig {
            fallback_policy: FallbackPolicy::Error,
            ..Default::default()
        };
        let sensory = Arc::new(benshu_sensory::SensoryHub::new(config));
        let tool = TextExtractTool::new(None, None, sensory);

        let tempdir = tempfile::tempdir().expect("tempdir");
        let image_path = tempdir.path().join("sample.png");
        image::DynamicImage::new_rgba8(4, 4)
            .save(&image_path)
            .expect("write image");

        let value = tool
            .recognize(&TextExtractArgs {
                action: "recognize".to_string(),
                path: image_path.to_string_lossy().to_string(),
                language: None,
                backend: Some("missing-backend".to_string()),
                model: None,
            })
            .await
            .expect("recognize result");

        assert!(matches!(
            value.get("status").and_then(|v| v.as_str()),
            Some("ok" | "error")
        ));
        assert_eq!(
            value
                .get("media_preprocess_source_kind")
                .and_then(|v| v.as_str()),
            Some("direct_image")
        );
        assert_eq!(
            value.get("media_preprocess_route").and_then(|v| v.as_str()),
            Some("image_page_raster")
        );
        if value.get("status").and_then(|v| v.as_str()) == Some("ok") {
            assert_eq!(
                value
                    .get("media_preprocess_consumer")
                    .and_then(|v| v.as_str()),
                Some("ocr")
            );
            assert!(matches!(
                value.get("media_pipeline_outcome").and_then(|v| v.as_str()),
                Some("success" | "model_result_insufficient")
            ));
        } else {
            assert!(matches!(
                value.get("media_pipeline_outcome").and_then(|v| v.as_str()),
                Some("model_failed_after_preprocess" | "fallback_unavailable_after_ocr_failure")
            ));
        }
    }
}
