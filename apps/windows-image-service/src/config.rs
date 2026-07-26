use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeConfig {
    pub model_dir: String,
    pub source_model_dir: String,
    pub host: String,
    pub port: u16,
    pub model_name: String,
    pub steps: usize,
    pub guidance_scale: f32,
    pub negative_prompt: String,
    pub device_id: u32,
}

impl RuntimeConfig {
    pub fn from_env() -> Self {
        Self {
            model_dir: env_or_legacy(
                "BENSHU_ONNX_IMAGE_MODEL_DIR",
                "BENSHU_ONNX_DIFFUSION_MODEL_DIR",
                "",
            ),
            source_model_dir: env_or_legacy(
                "BENSHU_ONNX_IMAGE_SOURCE_MODEL_DIR",
                "BENSHU_ONNX_DIFFUSION_SOURCE_MODEL_DIR",
                "",
            ),
            host: env_or_legacy(
                "BENSHU_ONNX_IMAGE_HOST",
                "BENSHU_ONNX_DIFFUSION_HOST",
                "127.0.0.1",
            ),
            port: env_or_legacy(
                "BENSHU_ONNX_IMAGE_PORT",
                "BENSHU_ONNX_DIFFUSION_PORT",
                "8022",
            )
            .parse()
            .unwrap_or(8022),
            model_name: env_or_legacy(
                "BENSHU_ONNX_IMAGE_MODEL_NAME",
                "BENSHU_ONNX_DIFFUSION_MODEL_NAME",
                "local-image-model",
            ),
            steps: env_or_legacy(
                "BENSHU_ONNX_IMAGE_STEPS",
                "BENSHU_ONNX_DIFFUSION_STEPS",
                "4",
            )
            .parse()
            .unwrap_or(4),
            guidance_scale: env_or_legacy(
                "BENSHU_ONNX_IMAGE_GUIDANCE_SCALE",
                "BENSHU_ONNX_DIFFUSION_GUIDANCE_SCALE",
                "0.0",
            )
            .parse()
            .unwrap_or(0.0),
            negative_prompt: env_or_legacy(
                "BENSHU_ONNX_IMAGE_NEGATIVE_PROMPT",
                "BENSHU_ONNX_DIFFUSION_NEGATIVE_PROMPT",
                "blurry, low quality, distorted, bad anatomy, deformed",
            ),
            device_id: env_or_legacy(
                "BENSHU_ONNX_IMAGE_DEVICE_ID",
                "BENSHU_ONNX_DIFFUSION_DEVICE_ID",
                "0",
            )
            .parse()
            .unwrap_or(0),
        }
    }
}

fn env_or_legacy(primary: &str, legacy: &str, default: &str) -> String {
    std::env::var(primary)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var(legacy).ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| default.to_string())
}
