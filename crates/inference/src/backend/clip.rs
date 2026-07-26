use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::clip::text_model::{Activation, ClipTextConfig};
use candle_transformers::models::clip::vision_model::ClipVisionConfig;
use candle_transformers::models::clip::{self, ClipConfig};
use image::DynamicImage;
use std::path::Path;

/// Supported CLIP variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CLIPVariant {
    ViTB32,
    ViTL14,
    ViTL14_336, // Standard for LLaVA 1.5
    ViTG14,     // OpenCLIP bigG-14 (used by larger dual-encoder image pipelines)
    ViTB16,
}

impl CLIPVariant {
    pub fn config(&self) -> ClipConfig {
        match self {
            Self::ViTB32 => ClipConfig {
                text_config: ClipTextConfig::vit_base_patch32(),
                vision_config: ClipVisionConfig::vit_base_patch32(),
                image_size: 224,
                logit_scale_init_value: 2.6592,
            },
            Self::ViTB16 => {
                let vision_config = ClipVisionConfig {
                    embed_dim: 768,
                    intermediate_size: 3072,
                    num_hidden_layers: 12,
                    num_attention_heads: 12,
                    num_channels: 3,
                    image_size: 224,
                    patch_size: 16,
                    activation: Activation::QuickGelu,
                    projection_dim: 512,
                };
                ClipConfig {
                    text_config: ClipTextConfig::vit_base_patch32(),
                    vision_config,
                    image_size: 224,
                    logit_scale_init_value: 2.6592,
                }
            }
            Self::ViTL14 => {
                let vision_config = ClipVisionConfig {
                    embed_dim: 1024,
                    intermediate_size: 4096,
                    num_hidden_layers: 24,
                    num_attention_heads: 16,
                    num_channels: 3,
                    image_size: 224,
                    patch_size: 14,
                    activation: Activation::QuickGelu,
                    projection_dim: 768,
                };
                ClipConfig {
                    text_config: ClipTextConfig::vit_base_patch32(),
                    vision_config,
                    image_size: 224,
                    logit_scale_init_value: 2.6592,
                }
            }
            Self::ViTG14 => {
                let vision_config = ClipVisionConfig {
                    embed_dim: 1280,
                    intermediate_size: 5120,
                    num_hidden_layers: 32,
                    num_attention_heads: 20,
                    num_channels: 3,
                    image_size: 224,
                    patch_size: 14,
                    activation: Activation::QuickGelu, // Standard for candle CLIP
                    projection_dim: 1280,
                };
                ClipConfig {
                    text_config: ClipTextConfig {
                        vocab_size: 49408,
                        embed_dim: 1280,
                        intermediate_size: 5120,
                        num_hidden_layers: 32,
                        num_attention_heads: 20,
                        max_position_embeddings: 77,
                        activation:
                            candle_transformers::models::clip::text_model::Activation::QuickGelu,
                        projection_dim: 1280,
                        pad_with: None,
                    },
                    vision_config,
                    image_size: 224,
                    logit_scale_init_value: 2.6592,
                }
            }
            Self::ViTL14_336 => {
                let vision_config = ClipVisionConfig::clip_vit_large_patch14_336();
                let text_config = ClipTextConfig {
                    vocab_size: 49408,
                    embed_dim: 768,
                    intermediate_size: 3072,
                    num_hidden_layers: 12,
                    num_attention_heads: 12,
                    max_position_embeddings: 77,
                    activation:
                        candle_transformers::models::clip::text_model::Activation::QuickGelu,
                    projection_dim: 768,
                    pad_with: None,
                };
                ClipConfig {
                    text_config,
                    vision_config,
                    image_size: 336,
                    logit_scale_init_value: 2.6592,
                }
            }
        }
    }

    pub fn default_filename(&self) -> &'static str {
        match self {
            Self::ViTB32 => "clip_vit_b32.safetensors",
            Self::ViTB16 => "clip_vit_b16.safetensors",
            Self::ViTL14 => "clip_vit_l14.safetensors",
            Self::ViTL14_336 => "clip_vit_l14_336.safetensors",
            Self::ViTG14 => "clip_vit_g14.safetensors",
        }
    }
}

/// Unified CLIP Model for Vision and Text
pub struct CLIPModel {
    model: clip::ClipModel,
    vision_model: candle_transformers::models::clip::vision_model::ClipVisionTransformer,
    device: Device,
    variant: CLIPVariant,
    config: ClipConfig,
}

impl CLIPModel {
    /// Load a specific CLIP variant from a directory
    pub fn load<P: AsRef<Path>>(variant: CLIPVariant, model_dir: P) -> Result<Self> {
        let device = if candle_core::utils::cuda_is_available() {
            Device::new_cuda(0).unwrap_or(Device::Cpu)
        } else {
            Device::Cpu
        };

        let config = variant.config();
        let model_path = model_dir.as_ref().join(variant.default_filename());

        let effective_path = if model_path.exists() {
            model_path
        } else {
            let fallback_path = model_dir.as_ref().join("model.safetensors");
            if !fallback_path.exists() {
                return Err(anyhow!(
                    "CLIP weights not found for {:?} in {:?}. Need model.safetensors",
                    variant,
                    model_dir.as_ref()
                ));
            }
            fallback_path
        };

        let vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[effective_path], DType::F32, &device)? };

        let vision_model =
            candle_transformers::models::clip::vision_model::ClipVisionTransformer::new(
                vb.pp("vision_model"),
                &config.vision_config,
            )?;
        let model = clip::ClipModel::new(vb, &config)?;

        Ok(Self {
            model,
            vision_model,
            device,
            variant,
            config,
        })
    }

    pub fn new(vb: VarBuilder, config: &ClipConfig) -> Result<Self> {
        let device = vb.device().clone();
        let vision_model =
            candle_transformers::models::clip::vision_model::ClipVisionTransformer::new(
                vb.pp("vision_model"),
                &config.vision_config,
            )?;
        let model = clip::ClipModel::new(vb, config)?;
        Ok(Self {
            model,
            vision_model,
            device,
            variant: infer_variant_from_config(config),
            config: config.clone(),
        })
    }

    /// Primary entry point for image encoding
    pub fn encode_image(&self, img: &DynamicImage) -> Result<Tensor> {
        let size = self.config.image_size;
        let resized = img.resize_exact(
            size as u32,
            size as u32,
            image::imageops::FilterType::Triangle,
        );
        let rgb = resized.to_rgb8();
        let data = rgb.into_raw();

        let tensor = Tensor::from_vec(data, (size, size, 3), &self.device)?
            .permute((2, 0, 1))?
            .to_dtype(DType::F32)?
            .affine(1.0 / 255.0, 0.0)?
            .unsqueeze(0)?;

        let mean = Tensor::new(&[0.48145466f32, 0.4578275f32, 0.40821073f32], &self.device)?
            .reshape((1, 3, 1, 1))?;
        let std = Tensor::new(&[0.26862954f32, 0.26130258f32, 0.27577711f32], &self.device)?
            .reshape((1, 3, 1, 1))?;
        let normalized = tensor.broadcast_sub(&mean)?.broadcast_div(&std)?;

        let features = self.model.get_image_features(&normalized)?;
        Ok(features)
    }

    /// Get unpooled vision features (e.g. for LLaVA/VLM)
    pub fn extract_features(&self, img: &image::DynamicImage) -> Result<Tensor> {
        let size = self.config.image_size;
        let resized = img.resize_exact(
            size as u32,
            size as u32,
            image::imageops::FilterType::Triangle,
        );
        let rgb = resized.to_rgb8();
        let data = rgb.into_raw();

        let tensor = Tensor::from_vec(data, (size, size, 3), &self.device)?
            .permute((2, 0, 1))?
            .to_dtype(DType::F32)?
            .affine(1.0 / 255.0, 0.0)?
            .unsqueeze(0)?;

        let mean = Tensor::new(&[0.48145466f32, 0.4578275f32, 0.40821073f32], &self.device)?
            .reshape((1, 3, 1, 1))?;
        let std = Tensor::new(&[0.26862954f32, 0.26130258f32, 0.27577711f32], &self.device)?
            .reshape((1, 3, 1, 1))?;
        let normalized = tensor.broadcast_sub(&mean)?.broadcast_div(&std)?;

        // Stage 21.10: Unpooled forward for sequence-based VLM projection
        // We use the second-to-last item from output_hidden_states which is the unpooled encoder output
        let mut hidden_states = self.vision_model.output_hidden_states(&normalized)?;

        // Remove the pooled CLS token added by ClipVisionTransformer::output_hidden_states
        hidden_states.pop();

        // Return the last unpooled hidden state sequence
        let features = hidden_states
            .pop()
            .ok_or_else(|| anyhow!("No hidden states"))?;

        Ok(features)
    }

    /// Primary entry point for text encoding in image-generation pipelines
    pub fn encode_text(&self, tokens: &Tensor) -> Result<Tensor> {
        let features = self.model.get_text_features(tokens)?;
        Ok(features)
    }

    pub fn model(&self) -> &clip::ClipModel {
        &self.model
    }

    /// Load a default CLIP model for multimodal backends (Phase 21.10)
    /// Searches in: current dir, assets/, and models/
    pub fn load_default(device: &Device) -> Result<Self> {
        let variant = CLIPVariant::ViTL14_336; // LLaVA 1.5 Standard
        let search_paths = [
            std::path::PathBuf::from("."),
            std::path::PathBuf::from("assets"),
            std::path::PathBuf::from("models"),
            std::path::PathBuf::from("crates/inference/assets"),
        ];

        for path in search_paths {
            if !path.exists() {
                continue;
            }

            // Check for specific variant file or generic model.safetensors
            let target_file = path.join(variant.default_filename());
            let generic_file = path.join("model.safetensors");
            let encoder_file = path.join("vision_encoder.safetensors");

            let best_file = if target_file.exists() {
                Some(target_file)
            } else if generic_file.exists() {
                Some(generic_file)
            } else if encoder_file.exists() {
                Some(encoder_file)
            } else {
                None
            };

            if let Some(file) = best_file {
                let vb =
                    unsafe { VarBuilder::from_mmaped_safetensors(&[file], DType::F32, device)? };
                return Self::new(vb, &variant.config());
            }
        }

        Err(anyhow!(
            "CLIP weights not found for {:?} in any standard location.",
            variant
        ))
    }

    pub fn default_filename(&self) -> &'static str {
        self.variant.default_filename()
    }
}

pub type CLIPVisionModel = CLIPModel;
pub type CLIPTextModel = CLIPModel;
pub type ClipEncoder = CLIPModel;

fn infer_variant_from_config(config: &ClipConfig) -> CLIPVariant {
    let vision = &config.vision_config;
    match (
        config.image_size,
        vision.patch_size,
        vision.embed_dim,
        vision.projection_dim,
    ) {
        (336, 14, 1024, 768) => CLIPVariant::ViTL14_336,
        (_, 14, 1280, 1280) => CLIPVariant::ViTG14,
        (_, 14, 1024, 768) => CLIPVariant::ViTL14,
        (_, 16, 768, 512) => CLIPVariant::ViTB16,
        (_, 32, 768, 512) => CLIPVariant::ViTB32,
        _ => CLIPVariant::ViTL14,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_variant_preserves_large_patch_336_metadata() {
        let config = CLIPVariant::ViTL14_336.config();
        assert_eq!(infer_variant_from_config(&config), CLIPVariant::ViTL14_336);
    }

    #[test]
    fn infer_variant_distinguishes_clip_families() {
        assert_eq!(
            infer_variant_from_config(&CLIPVariant::ViTB32.config()),
            CLIPVariant::ViTB32
        );
        assert_eq!(
            infer_variant_from_config(&CLIPVariant::ViTB16.config()),
            CLIPVariant::ViTB16
        );
        assert_eq!(
            infer_variant_from_config(&CLIPVariant::ViTG14.config()),
            CLIPVariant::ViTG14
        );
    }
}
