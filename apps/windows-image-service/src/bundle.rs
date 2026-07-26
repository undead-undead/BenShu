use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
pub struct BundleInfo {
    pub bundle_dir: String,
    pub model_index_path: String,
    pub model_class: String,
    pub pipeline_family: String,
    pub editing_mode: String,
    pub capabilities: Vec<String>,
    pub adapter: Option<String>,
    pub source_pipeline_class: Option<String>,
    pub runtime_pipeline_class: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BundleManifest {
    adapter: Option<String>,
    pipeline_family: Option<String>,
    source_pipeline_class: Option<String>,
    runtime_pipeline_class: Option<String>,
    capabilities: Option<BundleCapabilities>,
}

#[derive(Debug, Deserialize)]
struct BundleCapabilities {
    text_to_image: Option<bool>,
    image_edit: Option<bool>,
    mask_edit: Option<bool>,
}

impl BundleInfo {
    pub fn inspect(model_dir: &Path) -> anyhow::Result<Self> {
        let model_index_path = model_dir.join("model_index.json");
        let payload = std::fs::read_to_string(&model_index_path)?;
        let json: Value = serde_json::from_str(&payload)?;
        let model_class = json
            .get("_class_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let manifest = read_manifest(model_dir)?;
        let (pipeline_family, editing_mode, fallback_capabilities) =
            classify_model_class(&model_class);
        let capabilities = manifest
            .as_ref()
            .and_then(manifest_capabilities)
            .unwrap_or_else(|| {
                fallback_capabilities
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });

        Ok(Self {
            bundle_dir: model_dir.to_string_lossy().into_owned(),
            model_index_path: model_index_path.to_string_lossy().into_owned(),
            model_class,
            pipeline_family: manifest
                .as_ref()
                .and_then(|m| m.pipeline_family.clone())
                .unwrap_or_else(|| pipeline_family.to_string()),
            editing_mode: editing_mode.to_string(),
            capabilities,
            adapter: manifest.as_ref().and_then(|m| m.adapter.clone()),
            source_pipeline_class: manifest
                .as_ref()
                .and_then(|m| m.source_pipeline_class.clone()),
            runtime_pipeline_class: manifest
                .as_ref()
                .and_then(|m| m.runtime_pipeline_class.clone()),
        })
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|cap| cap == capability)
    }
}

fn read_manifest(model_dir: &Path) -> anyhow::Result<Option<BundleManifest>> {
    let manifest_path = model_dir.join("benshu_image_bundle.json");
    if !manifest_path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(manifest_path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

fn manifest_capabilities(manifest: &BundleManifest) -> Option<Vec<String>> {
    let caps = manifest.capabilities.as_ref()?;
    let mut result = Vec::new();
    if caps.text_to_image.unwrap_or(false) {
        result.push("text_to_image".to_string());
    }
    if caps.image_edit.unwrap_or(false) {
        result.push("image_edit".to_string());
    }
    if caps.mask_edit.unwrap_or(false) {
        result.push("inpainting".to_string());
    }
    Some(result)
}

fn classify_model_class(model_class: &str) -> (&'static str, &'static str, Vec<&'static str>) {
    let normalized = model_class.trim();
    if normalized.contains("Inpaint") {
        return (
            "stable-diffusion-family",
            "native-mask-edit",
            vec!["text_to_image", "image_edit", "inpainting"],
        );
    }
    if normalized.contains("Img2Img") {
        return (
            "stable-diffusion-family",
            "native-image-edit",
            vec!["text_to_image", "image_edit"],
        );
    }
    if normalized.contains("StableDiffusionXL") || normalized.contains("StableDiffusion") {
        return (
            "stable-diffusion-family",
            "best_effort",
            vec!["text_to_image", "image_edit"],
        );
    }
    if normalized.contains("Flux") || normalized.contains("Kontext") {
        return (
            "flux-family",
            "adapter_required",
            vec!["text_to_image", "image_edit", "inpainting"],
        );
    }
    ("generic-onnx-image", "adapter_required", vec!["unknown"])
}
