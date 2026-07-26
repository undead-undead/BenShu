use crate::adapter::{resolve_adapter, RequestMode};
use crate::bundle::BundleInfo;
use crate::config::RuntimeConfig;
use crate::plan::ExecutionPlan;
use crate::runtime::ImageRuntimeStatus;
use crate::types::{
    ImageData, ImageResponse, NormalizedImageRequest, PreparedImageRequest, ServiceError,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct ExecutionContext {
    pub config: RuntimeConfig,
    pub bundle: Arc<BundleInfo>,
    pub runtime: ImageRuntimeStatus,
}

#[async_trait]
pub trait ImageExecutor: Send + Sync {
    async fn execute(
        &self,
        ctx: &ExecutionContext,
        request: NormalizedImageRequest,
        editing: bool,
    ) -> Result<ImageResponse, ServiceError>;
}

pub struct NativeOnnxImageExecutor;

impl NativeOnnxImageExecutor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ImageExecutor for NativeOnnxImageExecutor {
    async fn execute(
        &self,
        ctx: &ExecutionContext,
        request: NormalizedImageRequest,
        editing: bool,
    ) -> Result<ImageResponse, ServiceError> {
        let prepared = request.prepare()?;
        let requested_mode = request_mode(&prepared, editing);
        let adapter = resolve_adapter(&ctx.bundle)?;
        let plan = ExecutionPlan::build(&ctx.bundle, adapter, requested_mode)?;
        let capability_known = ctx.bundle.supports(requested_mode.as_str());
        let adapter_supports_mode = adapter.supports_mode(requested_mode);

        if ctx.runtime.small_model_runtime_readiness != "windows_native_ready" {
            return Err(ServiceError::not_implemented(
                format!(
                    "windows-native image runtime is not ready yet (readiness={}, reason={})",
                    ctx.runtime.small_model_runtime_readiness,
                    ctx.runtime.small_model_runtime_reason,
                ),
                "onnx_directml_runtime_not_ready",
            ));
        }

        if !capability_known {
            return Err(ServiceError::not_implemented(
                format!(
                    "bundle does not declare support for {} (adapter={}, family={})",
                    requested_mode.as_str(),
                    adapter.id,
                    ctx.bundle.pipeline_family,
                ),
                "onnx_directml_image_capability_missing",
            ));
        }

        if !adapter_supports_mode {
            return Err(ServiceError::not_implemented(
                format!(
                    "registered adapter {} does not support {} yet",
                    adapter.id,
                    requested_mode.as_str(),
                ),
                "onnx_directml_image_mode_unsupported",
            ));
        }

        plan.ensure_ready()?;

        let adapter_note = format!(
            "Rust image execution layer is now the formal service boundary, but the native ONNX image executor for bundle class '{}' (family='{}', adapter='{}') is not wired yet. The request is normalized, decoded, and adapter-routed inside Rust so the execution adapter can be swapped in without changing any external contract.",
            ctx.bundle.model_class,
            ctx.bundle.pipeline_family,
            adapter.id,
        );

        let payload = json!({
            "requested_mode": requested_mode.as_str(),
            "capability_known": capability_known,
            "adapter_supports_mode": adapter_supports_mode,
            "adapter": adapter.id,
            "prompt_length": prepared.prompt.chars().count(),
            "has_input_image": prepared.source_image.is_some(),
            "has_mask_image": prepared.mask_image.is_some(),
            "response_format": prepared.response_format,
            "size": format!("{}x{}", prepared.width, prepared.height),
            "count": prepared.n,
            "model": ctx.config.model_name,
            "runtime": ctx.runtime,
            "bundle": ctx.bundle,
            "plan": plan,
            "note": adapter_note,
        });

        Err(ServiceError::not_implemented(
            payload.to_string(),
            "onnx_directml_image_adapter_unimplemented",
        ))
    }
}

fn request_mode(request: &PreparedImageRequest, editing: bool) -> RequestMode {
    if !editing {
        return RequestMode::TextToImage;
    }
    if request.mask_image.is_some() {
        RequestMode::Inpainting
    } else {
        RequestMode::ImageEdit
    }
}

#[allow(dead_code)]
pub fn empty_image_response() -> ImageResponse {
    ImageResponse {
        created: chrono::Utc::now().timestamp(),
        data: vec![ImageData {
            b64_json: String::new(),
        }],
    }
}
