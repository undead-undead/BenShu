use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ImageRequest {
    pub prompt: String,
    #[serde(default = "default_image_count")]
    pub n: usize,
    #[serde(default = "default_image_size")]
    pub size: String,
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub image_b64: Option<String>,
    #[serde(default)]
    pub mask_b64: Option<String>,
}

fn default_image_count() -> usize {
    1
}

fn default_image_size() -> String {
    "1024x1024".to_string()
}

#[derive(Debug)]
pub struct NormalizedImageRequest {
    pub prompt: String,
    pub n: usize,
    pub width: u32,
    pub height: u32,
    pub response_format: String,
    pub image_b64: Option<String>,
    pub mask_b64: Option<String>,
}

#[derive(Debug)]
pub struct PreparedImageRequest {
    pub prompt: String,
    pub n: usize,
    pub width: u32,
    pub height: u32,
    pub response_format: String,
    pub source_image: Option<image::DynamicImage>,
    pub mask_image: Option<image::DynamicImage>,
}

impl ImageRequest {
    pub fn normalize(self, editing: bool) -> Result<NormalizedImageRequest, ServiceError> {
        if self.prompt.trim().is_empty() {
            return Err(ServiceError::bad_request("prompt is required"));
        }

        if editing && self.image_b64.as_deref().unwrap_or("").trim().is_empty() {
            return Err(ServiceError::bad_request(
                "image_b64 is required for /v1/images/edits",
            ));
        }

        let (width, height) = parse_size(&self.size)?;
        Ok(NormalizedImageRequest {
            prompt: self.prompt,
            n: self.n,
            width,
            height,
            response_format: self
                .response_format
                .unwrap_or_else(|| "b64_json".to_string()),
            image_b64: self.image_b64,
            mask_b64: self.mask_b64,
        })
    }
}

impl NormalizedImageRequest {
    pub fn prepare(self) -> Result<PreparedImageRequest, ServiceError> {
        Ok(PreparedImageRequest {
            prompt: self.prompt,
            n: self.n,
            width: self.width,
            height: self.height,
            response_format: validate_response_format(&self.response_format)?,
            source_image: self
                .image_b64
                .as_deref()
                .map(decode_b64_image)
                .transpose()?,
            mask_image: self.mask_b64.as_deref().map(decode_b64_image).transpose()?,
        })
    }
}

fn parse_size(size: &str) -> Result<(u32, u32), ServiceError> {
    let normalized = size.to_ascii_lowercase();
    let (width, height) = normalized
        .split_once('x')
        .ok_or_else(|| ServiceError::bad_request(format!("invalid size: {size}")))?;
    let width = width
        .parse()
        .map_err(|_| ServiceError::bad_request(format!("invalid size: {size}")))?;
    let height = height
        .parse()
        .map_err(|_| ServiceError::bad_request(format!("invalid size: {size}")))?;
    Ok((width, height))
}

fn validate_response_format(value: &str) -> Result<String, ServiceError> {
    let normalized = value.trim();
    if normalized.is_empty() || normalized == "b64_json" {
        return Ok("b64_json".to_string());
    }
    Err(ServiceError::bad_request(format!(
        "unsupported response_format: {value}"
    )))
}

fn decode_b64_image(payload: &str) -> Result<image::DynamicImage, ServiceError> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|err| ServiceError::bad_request(format!("invalid base64 image payload: {err}")))?;
    image::load_from_memory(&raw)
        .map_err(|err| ServiceError::bad_request(format!("invalid image payload: {err}")))
}

#[derive(Debug, Serialize)]
pub struct ImageResponse {
    pub created: i64,
    pub data: Vec<ImageData>,
}

#[derive(Debug, Serialize)]
pub struct ImageData {
    pub b64_json: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorPayload {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug)]
pub struct ServiceError {
    pub message: String,
    pub kind: String,
    pub status: axum::http::StatusCode,
}

impl ServiceError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: "invalid_request_error".to_string(),
            status: axum::http::StatusCode::BAD_REQUEST,
        }
    }

    pub fn not_implemented(message: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: kind.into(),
            status: axum::http::StatusCode::NOT_IMPLEMENTED,
        }
    }
}
