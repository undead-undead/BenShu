use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::clip::{
    self,
    text_model::{Activation, ClipTextConfig},
    vision_model::ClipVisionConfig,
    ClipConfig,
};
use image::DynamicImage;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::protocol::SensoryOutput;
use crate::vision::VisionPlugin;

/// CLIP (Contrastive Language-Image Pre-training) plugin for zero-shot vision tasks
pub struct ClipPlugin {
    inner: Arc<Mutex<ClipInner>>,
    name: &'static str,
}

struct ClipInner {
    model: Option<clip::ClipModel>,
    config: ClipConfig,
    device: Device,
    model_path: std::path::PathBuf,
}

impl ClipPlugin {
    pub fn new<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let dir = dir.as_ref();
        let config_path = dir.join("config.json");
        let config_json = std::fs::read_to_string(&config_path)
            .map_err(|_| anyhow!("CLIP model config not found at {}", config_path.display()))?;
        let config_value: serde_json::Value = serde_json::from_str(&config_json)
            .map_err(|err| anyhow!("Invalid CLIP config {}: {err}", config_path.display()))?;
        let model_type = config_value
            .get("model_type")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let has_clip_architecture = config_value
            .get("architectures")
            .and_then(|value| value.as_array())
            .map(|items| {
                items.iter().any(|item| {
                    item.as_str()
                        .map(|name| name.to_ascii_lowercase().contains("clip"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if model_type != "clip"
            && !has_clip_architecture
            && config_value.get("vision_config").is_none()
        {
            return Err(anyhow!(
                "Unsupported CLIP config at {}: expected model_type=clip, CLIP architecture, or vision_config",
                config_path.display()
            ));
        }

        let config = clip_config_from_value(&config_value).map_err(|err| {
            anyhow!(
                "Unsupported CLIP config at {}: {err}. BenShu currently supports CLIP-family configs whose text_config and vision_config are explicit and QuickGELU-compatible.",
                config_path.display()
            )
        })?;

        let model_path = dir.join("model.safetensors");
        if !model_path.exists() {
            return Err(anyhow!(
                "CLIP weights not found at {}; expected model.safetensors",
                model_path.display()
            ));
        }

        let device = if candle_core::utils::cuda_is_available() {
            Device::new_cuda(0).unwrap_or(Device::Cpu)
        } else if candle_core::utils::metal_is_available() {
            Device::new_metal(0).unwrap_or(Device::Cpu)
        } else {
            Device::Cpu
        };

        Ok(Self {
            name: "clip-vit-large-patch14-336",
            inner: Arc::new(Mutex::new(ClipInner {
                model: None,
                config,
                device,
                model_path,
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::clip_config_from_value;
    use serde_json::json;

    #[test]
    fn parses_explicit_clip_family_config() {
        let config = json!({
            "model_type": "clip",
            "logit_scale_init_value": 2.6592,
            "text_config": {
                "vocab_size": 49408,
                "hidden_size": 768,
                "intermediate_size": 3072,
                "max_position_embeddings": 77,
                "num_hidden_layers": 12,
                "num_attention_heads": 12,
                "projection_dim": 768,
                "hidden_act": "quick_gelu"
            },
            "vision_config": {
                "hidden_size": 1024,
                "intermediate_size": 4096,
                "num_hidden_layers": 24,
                "num_attention_heads": 16,
                "projection_dim": 768,
                "num_channels": 3,
                "image_size": 336,
                "patch_size": 14,
                "hidden_act": "quick_gelu"
            }
        });

        let parsed = clip_config_from_value(&config).expect("valid clip config");
        assert_eq!(parsed.text_config.embed_dim, 768);
        assert_eq!(parsed.vision_config.embed_dim, 1024);
        assert_eq!(parsed.image_size, 336);
    }

    #[test]
    fn rejects_projection_mismatch_instead_of_silent_shape_fallback() {
        let config = json!({
            "model_type": "clip",
            "text_config": {
                "vocab_size": 49408,
                "hidden_size": 512,
                "intermediate_size": 2048,
                "max_position_embeddings": 77,
                "num_hidden_layers": 12,
                "num_attention_heads": 8,
                "projection_dim": 512
            },
            "vision_config": {
                "hidden_size": 1024,
                "intermediate_size": 4096,
                "num_hidden_layers": 24,
                "num_attention_heads": 16,
                "projection_dim": 768,
                "num_channels": 3,
                "image_size": 336,
                "patch_size": 14
            }
        });

        let err = clip_config_from_value(&config).unwrap_err().to_string();
        assert!(err.contains("projection_dim"));
    }
}

fn clip_config_from_value(config: &serde_json::Value) -> Result<ClipConfig> {
    let text = config
        .get("text_config")
        .ok_or_else(|| anyhow!("missing text_config"))?;
    let vision = config
        .get("vision_config")
        .ok_or_else(|| anyhow!("missing vision_config"))?;
    let vision_config = parse_clip_vision_config(vision)?;
    let text_config = parse_clip_text_config(text)?;
    let logit_scale_init_value = config
        .get("logit_scale_init_value")
        .and_then(|value| value.as_f64())
        .unwrap_or(2.6592) as f32;
    if text_config.projection_dim != vision_config.projection_dim {
        bail!(
            "text projection_dim {} does not match vision projection_dim {}",
            text_config.projection_dim,
            vision_config.projection_dim
        );
    }
    Ok(ClipConfig {
        image_size: vision_config.image_size,
        text_config,
        vision_config,
        logit_scale_init_value,
    })
}

fn parse_clip_vision_config(value: &serde_json::Value) -> Result<ClipVisionConfig> {
    ensure_quick_gelu(value)?;
    let embed_dim = read_usize(value, &["hidden_size", "embed_dim"])?;
    Ok(ClipVisionConfig {
        embed_dim,
        activation: Activation::QuickGelu,
        intermediate_size: read_usize(value, &["intermediate_size"])?,
        num_hidden_layers: read_usize(value, &["num_hidden_layers"])?,
        num_attention_heads: read_usize(value, &["num_attention_heads"])?,
        projection_dim: read_usize(value, &["projection_dim"])?,
        num_channels: read_usize(value, &["num_channels"])?,
        image_size: read_usize(value, &["image_size"])?,
        patch_size: read_usize(value, &["patch_size"])?,
    })
}

fn parse_clip_text_config(value: &serde_json::Value) -> Result<ClipTextConfig> {
    ensure_quick_gelu(value)?;
    let embed_dim = read_usize(value, &["hidden_size", "embed_dim"])?;
    Ok(ClipTextConfig {
        vocab_size: read_usize(value, &["vocab_size"])?,
        embed_dim,
        activation: Activation::QuickGelu,
        intermediate_size: read_usize(value, &["intermediate_size"])?,
        max_position_embeddings: read_usize(value, &["max_position_embeddings"])?,
        pad_with: value
            .get("pad_with")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        num_hidden_layers: read_usize(value, &["num_hidden_layers"])?,
        num_attention_heads: read_usize(value, &["num_attention_heads"])?,
        projection_dim: read_usize(value, &["projection_dim"])?,
    })
}

fn ensure_quick_gelu(value: &serde_json::Value) -> Result<()> {
    let Some(activation) = value
        .get("hidden_act")
        .or_else(|| value.get("activation"))
        .and_then(|value| value.as_str())
    else {
        return Ok(());
    };
    let normalized = activation
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if normalized == "quickgelu" {
        Ok(())
    } else {
        bail!("unsupported activation '{activation}', expected quick_gelu")
    }
}

fn read_usize(value: &serde_json::Value, keys: &[&str]) -> Result<usize> {
    for key in keys {
        if let Some(raw) = value.get(*key).and_then(|value| value.as_u64()) {
            return usize::try_from(raw).map_err(|_| anyhow!("{key} is outside usize range"));
        }
    }
    bail!("missing required numeric field {}", keys.join("/"))
}

#[async_trait]
impl VisionPlugin for ClipPlugin {
    fn name(&self) -> &str {
        self.name
    }

    async fn load(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if inner.model.is_some() {
            return Ok(());
        }

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(
                &[inner.model_path.clone()],
                candle_core::DType::F32,
                &inner.device,
            )?
        };
        let model = clip::ClipModel::new(vb, &inner.config)?;
        inner.model = Some(model);
        Ok(())
    }

    fn unload(&self) {
        if let Ok(mut inner) = self.inner.try_lock() {
            inner.model = None;
        }
    }

    fn is_loaded(&self) -> bool {
        if let Ok(inner) = self.inner.try_lock() {
            inner.model.is_some()
        } else {
            false
        }
    }

    async fn process(
        &self,
        image: &image::DynamicImage,
        prompt: Option<&str>,
    ) -> Result<SensoryOutput> {
        self.load().await?; // Ensure loaded
        let inner = self.inner.lock().await;
        let model = inner.model.as_ref().unwrap();

        // 1. Pre-process image to tensor
        let img_tensor =
            self.preprocess_image(image, &inner.config.vision_config, &inner.device)?;

        // 2. If prompt is provided, treat it as a list of labels
        if let Some(_labels_str) = prompt {
            return Ok(SensoryOutput::Text(
                "Zero-shot CLIP classification pending tokenizer integration".into(),
            ));
        }

        // 3. Otherwise, return image features (embeddings)
        let features = model.get_image_features(&img_tensor)?;
        let features_vec: Vec<f32> = features.flatten_all()?.to_vec1()?;

        Ok(SensoryOutput::Features(features_vec))
    }

    fn estimated_memory_usage(&self) -> u64 {
        // ViT-L/14 is larger than B/32
        1200 * 1024 * 1024
    }
}

impl ClipPlugin {
    fn preprocess_image(
        &self,
        img: &DynamicImage,
        config: &ClipVisionConfig,
        device: &Device,
    ) -> Result<Tensor> {
        let size = config.image_size;
        let img = img.resize_exact(
            size as u32,
            size as u32,
            image::imageops::FilterType::Triangle,
        );
        let rgb = img.to_rgb8();
        let data = rgb.into_raw();

        let tensor = Tensor::from_vec(data, (size, size, 3), device)?
            .permute((2, 0, 1))? // HWC to CHW
            .to_dtype(candle_core::DType::F32)?
            .affine(1.0 / 255.0, 0.0)?; // 0-1 range

        Ok(tensor.unsqueeze(0)?)
    }
}
