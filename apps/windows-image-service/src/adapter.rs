use crate::bundle::BundleInfo;
use crate::types::ServiceError;

#[derive(Clone, Debug)]
pub struct AdapterInfo {
    pub id: &'static str,
    pub pipeline_family: &'static str,
    pub supports_text_to_image: bool,
    pub supports_image_edit: bool,
    pub supports_inpainting: bool,
}

impl AdapterInfo {
    pub fn supports_mode(&self, mode: RequestMode) -> bool {
        match mode {
            RequestMode::TextToImage => self.supports_text_to_image,
            RequestMode::ImageEdit => self.supports_image_edit,
            RequestMode::Inpainting => self.supports_inpainting,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RequestMode {
    TextToImage,
    ImageEdit,
    Inpainting,
}

impl RequestMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestMode::TextToImage => "text_to_image",
            RequestMode::ImageEdit => "image_edit",
            RequestMode::Inpainting => "inpainting",
        }
    }
}

const DIFFUSERS_ORT_SD: AdapterInfo = AdapterInfo {
    id: "diffusers_ort_stable_diffusion",
    pipeline_family: "stable-diffusion",
    supports_text_to_image: true,
    supports_image_edit: false,
    supports_inpainting: false,
};

const DIFFUSERS_ORT_SDXL: AdapterInfo = AdapterInfo {
    id: "diffusers_ort_stable_diffusion_xl",
    pipeline_family: "stable-diffusion-xl",
    supports_text_to_image: true,
    supports_image_edit: false,
    supports_inpainting: false,
};

const FLUX_KONTEXT_ORT: AdapterInfo = AdapterInfo {
    id: "flux_kontext_ort",
    pipeline_family: "flux-family",
    supports_text_to_image: true,
    supports_image_edit: true,
    supports_inpainting: true,
};

pub fn resolve_adapter(bundle: &BundleInfo) -> Result<&'static AdapterInfo, ServiceError> {
    match bundle.adapter.as_deref() {
        Some("diffusers_ort_stable_diffusion") => Ok(&DIFFUSERS_ORT_SD),
        Some("diffusers_ort_stable_diffusion_xl") => Ok(&DIFFUSERS_ORT_SDXL),
        Some("flux_kontext_ort") => Ok(&FLUX_KONTEXT_ORT),
        Some(other) => Err(ServiceError::not_implemented(
            format!("unsupported image adapter declared by bundle: {other}"),
            "onnx_directml_image_adapter_unsupported",
        )),
        None => {
            if bundle.pipeline_family == DIFFUSERS_ORT_SD.pipeline_family {
                return Ok(&DIFFUSERS_ORT_SD);
            }
            if bundle.pipeline_family == DIFFUSERS_ORT_SDXL.pipeline_family {
                return Ok(&DIFFUSERS_ORT_SDXL);
            }
            if bundle.pipeline_family == FLUX_KONTEXT_ORT.pipeline_family {
                return Ok(&FLUX_KONTEXT_ORT);
            }
            Err(ServiceError::not_implemented(
                format!(
                    "no compatible image adapter registered for pipeline_family={}",
                    bundle.pipeline_family
                ),
                "onnx_directml_image_adapter_missing",
            ))
        }
    }
}
