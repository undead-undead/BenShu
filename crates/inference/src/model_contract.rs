use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LocalModelArtifactKind {
    Unknown,
    ApiReference,
    GGUF,
    SafetensorsDirectory,
    OnnxDirectory,
    OnnxFile,
    ExternalRuntime,
    ImageBridge,
    DiffusersDirectory,
    ImageOnnxDirectory,
}

impl LocalModelArtifactKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LocalModelArtifactKind::Unknown => "unknown",
            LocalModelArtifactKind::ApiReference => "api_reference",
            LocalModelArtifactKind::GGUF => "gguf",
            LocalModelArtifactKind::SafetensorsDirectory => "safetensors_directory",
            LocalModelArtifactKind::OnnxDirectory => "onnx_directory",
            LocalModelArtifactKind::OnnxFile => "onnx_file",
            LocalModelArtifactKind::ExternalRuntime => "external_runtime",
            LocalModelArtifactKind::ImageBridge => "image_bridge",
            LocalModelArtifactKind::DiffusersDirectory => "diffusers_directory",
            LocalModelArtifactKind::ImageOnnxDirectory => "image_onnx_directory",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelContractDescriptor {
    pub kind: LocalModelArtifactKind,
    pub ready_for_windows_native_small_model_runtime: bool,
    pub reason: String,
}

pub fn describe_local_model_contract(path: &Path) -> LocalModelContractDescriptor {
    let path_str = path.to_string_lossy();

    if path_str.starts_with("api:") || path_str.starts_with("http") {
        return LocalModelContractDescriptor {
            kind: LocalModelArtifactKind::ApiReference,
            ready_for_windows_native_small_model_runtime: false,
            reason: "Cloud/API references are not local Windows-native small-model packages."
                .to_string(),
        };
    }

    if path_str.starts_with("bridge-image:") {
        return LocalModelContractDescriptor {
            kind: LocalModelArtifactKind::ImageBridge,
            ready_for_windows_native_small_model_runtime: false,
            reason: "Image bridge syntax is a valid image-generation contract that forwards requests into a dedicated image runtime (for example a Windows-hosted DirectML service)."
                .to_string(),
        };
    }

    if path_str.eq_ignore_ascii_case("tesseract") || path_str.eq_ignore_ascii_case("piper") {
        return LocalModelContractDescriptor {
            kind: LocalModelArtifactKind::ExternalRuntime,
            ready_for_windows_native_small_model_runtime: false,
            reason: "Configured model uses an external/specialized runtime instead of an ONNX small-model package."
                .to_string(),
        };
    }

    if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("gguf") {
        return LocalModelContractDescriptor {
            kind: LocalModelArtifactKind::GGUF,
            ready_for_windows_native_small_model_runtime: false,
            reason: "GGUF is reserved for the main llama.cpp brain path, not the Windows-native small-model runtime."
                .to_string(),
        };
    }

    if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("onnx") {
        return LocalModelContractDescriptor {
            kind: LocalModelArtifactKind::OnnxFile,
            ready_for_windows_native_small_model_runtime: true,
            reason: "Standalone ONNX artifact matches the Windows-native small-model packaging contract."
                .to_string(),
        };
    }

    if path.is_dir() {
        let has_safetensors = path.join("model.safetensors").exists();
        let has_onnx = path.join("model.onnx").exists();
        let has_tokenizer = path.join("tokenizer.json").exists();
        let has_config = path.join("config.json").exists();
        let has_model_index = path.join("model_index.json").exists();
        let has_image_pipeline_layout = path.join("unet").exists()
            || path.join("transformer").exists()
            || path.join("vae").exists()
            || path.join("text_encoder").exists()
            || path.join("text_encoder_2").exists();
        let has_nested_onnx = [
            path.join("unet/model.onnx"),
            path.join("text_encoder/model.onnx"),
            path.join("text_encoder_2/model.onnx"),
            path.join("vae_decoder/model.onnx"),
            path.join("vae_encoder/model.onnx"),
            path.join("transformer/model.onnx"),
        ]
        .into_iter()
        .any(|candidate| candidate.exists());

        if has_model_index && has_nested_onnx {
            return LocalModelContractDescriptor {
                kind: LocalModelArtifactKind::ImageOnnxDirectory,
                ready_for_windows_native_small_model_runtime: false,
                reason: "ONNX diffusion/image pipeline directory detected. This is suitable for a specialized image runtime such as Windows-native ONNX Runtime + DirectML, rather than the text small-model lane."
                    .to_string(),
            };
        }

        if has_model_index || has_image_pipeline_layout {
            return LocalModelContractDescriptor {
                kind: LocalModelArtifactKind::DiffusersDirectory,
                ready_for_windows_native_small_model_runtime: false,
                reason: "Diffusers-style image model directory detected. This is a valid image-generation asset package, but it targets a specialized image runtime instead of the Windows-native text small-model lane."
                    .to_string(),
            };
        }

        if has_onnx {
            return LocalModelContractDescriptor {
                kind: LocalModelArtifactKind::OnnxDirectory,
                ready_for_windows_native_small_model_runtime: true,
                reason: if has_tokenizer || has_config {
                    "ONNX directory with tokenizer/config is ready for the Windows-native small-model runtime."
                        .to_string()
                } else {
                    "ONNX directory detected; tokenizer/config metadata is optional but recommended."
                        .to_string()
                },
            };
        }

        if has_safetensors {
            return LocalModelContractDescriptor {
                kind: LocalModelArtifactKind::SafetensorsDirectory,
                ready_for_windows_native_small_model_runtime: false,
                reason: "Current package is a safetensors/Candle contract; it remains valid, but does not yet match the Windows-native ONNX small-model target."
                    .to_string(),
            };
        }
    }

    LocalModelContractDescriptor {
        kind: LocalModelArtifactKind::Unknown,
        ready_for_windows_native_small_model_runtime: false,
        reason: "Model contract could not be classified from the configured path.".to_string(),
    }
}
