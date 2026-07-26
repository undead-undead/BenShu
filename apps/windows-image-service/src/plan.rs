use crate::adapter::{AdapterInfo, RequestMode};
use crate::bundle::BundleInfo;
use crate::types::ServiceError;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
pub struct PlannedArtifact {
    pub role: String,
    pub relative_path: String,
    pub exists: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecutionPlan {
    pub adapter: String,
    pub requested_mode: String,
    pub pipeline_family: String,
    pub artifacts: Vec<PlannedArtifact>,
}

impl ExecutionPlan {
    pub fn build(
        bundle: &BundleInfo,
        adapter: &AdapterInfo,
        mode: RequestMode,
    ) -> Result<Self, ServiceError> {
        let bundle_root = Path::new(&bundle.bundle_dir);
        let artifact_specs = expected_artifacts(adapter, mode);
        let artifacts = artifact_specs
            .into_iter()
            .map(|(role, relative_path)| PlannedArtifact {
                role: role.to_string(),
                relative_path: relative_path.to_string(),
                exists: bundle_root.join(relative_path).exists(),
            })
            .collect();

        Ok(Self {
            adapter: adapter.id.to_string(),
            requested_mode: mode.as_str().to_string(),
            pipeline_family: bundle.pipeline_family.clone(),
            artifacts,
        })
    }

    pub fn missing_artifacts(&self) -> Vec<&PlannedArtifact> {
        self.artifacts
            .iter()
            .filter(|artifact| !artifact.exists)
            .collect()
    }

    pub fn ensure_ready(&self) -> Result<(), ServiceError> {
        let missing = self.missing_artifacts();
        if missing.is_empty() {
            return Ok(());
        }

        let details = missing
            .iter()
            .map(|artifact| format!("{} -> {}", artifact.role, artifact.relative_path))
            .collect::<Vec<_>>()
            .join(", ");

        Err(ServiceError::not_implemented(
            format!(
                "onnx image bundle is incomplete for {} via {}: missing {}",
                self.requested_mode, self.adapter, details
            ),
            "onnx_directml_bundle_incomplete",
        ))
    }
}

fn expected_artifacts(
    adapter: &AdapterInfo,
    mode: RequestMode,
) -> Vec<(&'static str, &'static str)> {
    match adapter.id {
        "diffusers_ort_stable_diffusion" => stable_diffusion_artifacts(mode, false),
        "diffusers_ort_stable_diffusion_xl" => stable_diffusion_artifacts(mode, true),
        "flux_kontext_ort" => flux_kontext_artifacts(mode),
        _ => vec![("model_index", "model_index.json")],
    }
}

fn stable_diffusion_artifacts(mode: RequestMode, xl: bool) -> Vec<(&'static str, &'static str)> {
    let mut artifacts = vec![
        ("model_index", "model_index.json"),
        ("text_encoder", "text_encoder/model.onnx"),
        ("unet", "unet/model.onnx"),
        ("vae_decoder", "vae_decoder/model.onnx"),
    ];

    if xl {
        artifacts.push(("text_encoder_2", "text_encoder_2/model.onnx"));
    }

    match mode {
        RequestMode::TextToImage => {}
        RequestMode::ImageEdit | RequestMode::Inpainting => {
            artifacts.push(("vae_encoder", "vae_encoder/model.onnx"));
        }
    }

    artifacts
}

fn flux_kontext_artifacts(mode: RequestMode) -> Vec<(&'static str, &'static str)> {
    let mut artifacts = vec![
        ("model_index", "model_index.json"),
        ("text_encoder", "text_encoder/model.onnx"),
        ("transformer", "transformer/model.onnx"),
        ("vae_decoder", "vae_decoder/model.onnx"),
    ];

    match mode {
        RequestMode::TextToImage => {}
        RequestMode::ImageEdit | RequestMode::Inpainting => {
            artifacts.push(("vae_encoder", "vae_encoder/model.onnx"));
        }
    }

    artifacts
}
