use crate::backend::{DeviceType, DiffusionConfig, ImageGenBackend, InferenceError, Result};
use async_trait::async_trait;
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;

const BRIDGE_PREFIX: &str = "bridge-image:";
const API_KEY_ENV: &str = "BENSHU_IMAGE_BRIDGE_API_KEY";

#[derive(Clone)]
pub struct OpenAiImageBridgeBackend {
    client: reqwest::Client,
    base_url: String,
    model_id: String,
    api_key: Option<String>,
}

impl OpenAiImageBridgeBackend {
    pub fn can_handle_path(path: &std::path::Path) -> bool {
        path.to_string_lossy().starts_with(BRIDGE_PREFIX)
    }

    pub fn from_path(path: &std::path::Path) -> Result<Self> {
        let raw = path.to_string_lossy();
        let spec = raw
            .strip_prefix(BRIDGE_PREFIX)
            .ok_or_else(|| InferenceError::InvalidInput("Invalid image bridge path".into()))?;
        let (base_url, model_id) = spec.split_once('|').ok_or_else(|| {
            InferenceError::InvalidInput(
                "Invalid image bridge path. Use bridge-image:http://host:port/v1|model-name".into(),
            )
        })?;

        let base_url = base_url.trim().trim_end_matches('/').to_string();
        let model_id = model_id.trim().to_string();

        if base_url.is_empty() || model_id.is_empty() {
            return Err(InferenceError::InvalidInput(
                "Image bridge base URL and model name must both be non-empty".into(),
            ));
        }

        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| InferenceError::BackendError(e.to_string()))?;

        Ok(Self {
            client,
            base_url,
            model_id,
            api_key: std::env::var(API_KEY_ENV)
                .ok()
                .filter(|v| !v.trim().is_empty()),
        })
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(api_key) = &self.api_key {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", api_key))
                    .map_err(|e| InferenceError::BackendError(e.to_string()))?,
            );
        }

        Ok(headers)
    }

    async fn request_image_bytes(
        &self,
        endpoint: &str,
        request: serde_json::Value,
    ) -> Result<Vec<u8>> {
        let request_id = format!("image-bridge:{endpoint}");

        let response = self
            .client
            .post(format!("{}/{}", self.base_url, endpoint))
            .headers(self.build_headers()?)
            .json(&request)
            .send()
            .await
            .map_err(|e| InferenceError::BackendError(e.to_string()))?;

        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| InferenceError::BackendError(e.to_string()))?;

        if !status.is_success() {
            let preview = String::from_utf8_lossy(&body);
            return Err(InferenceError::Execution(
                format!("Image bridge returned HTTP {}: {}", status, preview),
                request_id,
            ));
        }

        let payload: ImageGenerationResponse = serde_json::from_slice(&body)
            .map_err(|e| InferenceError::FormatError(e.to_string()))?;

        let image_entry = payload.data.into_iter().next().ok_or_else(|| {
            InferenceError::FormatError(
                "Image bridge response did not include any image data".into(),
            )
        })?;

        if let Some(b64_json) = image_entry.b64_json {
            return base64::engine::general_purpose::STANDARD
                .decode(b64_json)
                .map_err(|e| InferenceError::FormatError(e.to_string()));
        }

        if let Some(url) = image_entry.url {
            let bytes = self
                .client
                .get(url)
                .send()
                .await
                .map_err(|e| InferenceError::BackendError(e.to_string()))?
                .bytes()
                .await
                .map_err(|e| InferenceError::BackendError(e.to_string()))?;
            return Ok(bytes.to_vec());
        }

        Err(InferenceError::FormatError(
            "Image bridge response had neither b64_json nor url".into(),
        ))
    }

    fn encode_image(image: &image::DynamicImage) -> Result<String> {
        let mut buf = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buf);
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| InferenceError::FormatError(format!("Failed to encode image: {e}")))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(buf))
    }
}

#[derive(Debug, Deserialize)]
struct ImageGenerationResponse {
    data: Vec<ImageData>,
}

#[derive(Debug, Deserialize)]
struct ImageData {
    b64_json: Option<String>,
    url: Option<String>,
}

#[async_trait]
impl ImageGenBackend for OpenAiImageBridgeBackend {
    fn model_info(&self) -> String {
        format!(
            "Bridge-Image(OpenAI-compatible): {} @ {}",
            self.model_id, self.base_url
        )
    }

    async fn generate_image(
        &self,
        prompt: &str,
        size: (u32, u32),
        _config: DiffusionConfig,
    ) -> Result<image::DynamicImage> {
        let request = serde_json::json!({
            "model": self.model_id,
            "prompt": prompt,
            "n": 1,
            "size": format!("{}x{}", size.0, size.1),
            "response_format": "b64_json"
        });
        let bytes = self
            .request_image_bytes("images/generations", request)
            .await?;
        image::load_from_memory(&bytes)
            .map_err(|e| InferenceError::FormatError(format!("Failed to decode bridge image: {e}")))
    }

    async fn generate_image_img2img(
        &self,
        prompt: &str,
        initial_image: &image::DynamicImage,
        _config: DiffusionConfig,
    ) -> Result<image::DynamicImage> {
        let request = serde_json::json!({
            "model": self.model_id,
            "prompt": prompt,
            "n": 1,
            "response_format": "b64_json",
            "image_b64": Self::encode_image(initial_image)?,
        });
        let bytes = self.request_image_bytes("images/edits", request).await?;
        image::load_from_memory(&bytes).map_err(|e| {
            InferenceError::FormatError(format!("Failed to decode bridge edited image: {e}"))
        })
    }

    async fn generate_image_inpainting(
        &self,
        prompt: &str,
        initial_image: &image::DynamicImage,
        mask: &image::DynamicImage,
        _config: DiffusionConfig,
    ) -> Result<image::DynamicImage> {
        let request = serde_json::json!({
            "model": self.model_id,
            "prompt": prompt,
            "n": 1,
            "response_format": "b64_json",
            "image_b64": Self::encode_image(initial_image)?,
            "mask_b64": Self::encode_image(mask)?,
        });
        let bytes = self.request_image_bytes("images/edits", request).await?;
        image::load_from_memory(&bytes).map_err(|e| {
            InferenceError::FormatError(format!("Failed to decode bridge inpainted image: {e}"))
        })
    }

    fn estimated_memory_usage(&self) -> u64 {
        0
    }

    fn device_info(&self) -> DeviceType {
        DeviceType::Cloud
    }
}
