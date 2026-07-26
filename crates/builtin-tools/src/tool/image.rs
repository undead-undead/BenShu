//! Model-Agnostic Image Generation Tool (Phase 24)
//!
//! Provides tools for generating images using the OS-level ImageGenBackend.

use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use benshu_inference::backend::{DiffusionConfig, ImageGenBackend};
use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};

/// Tool for generating images through the configured image backend.
pub struct GenerateImageTool {
    backend: Arc<dyn ImageGenBackend>,
    output_dir: PathBuf,
}

impl GenerateImageTool {
    /// Create a new GenerateImageTool with a backend
    pub fn new(backend: Arc<dyn ImageGenBackend>, output_dir: impl Into<PathBuf>) -> Self {
        Self {
            backend,
            output_dir: output_dir.into(),
        }
    }
}

#[derive(Deserialize)]
struct GenerateImageArgs {
    prompt: String,
    #[serde(default = "default_size")]
    size: String,
    #[serde(default)]
    output_filename: Option<String>,
    #[serde(default)]
    input_image: Option<String>,
    #[serde(default)]
    mask_image: Option<String>,
}

fn default_size() -> String {
    "1024x1024".to_string()
}

#[async_trait]
impl Tool for GenerateImageTool {
    fn name(&self) -> String {
        "generate_image".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Generate or edit an image through the configured image backend. Supports text-to-image, image-to-image, and masked editing when the backend exposes them.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Detailed description of the image to generate"
                    },
                    "size": {
                        "type": "string",
                        "description": "Image size: '1024x1024', '1024x1792', '1792x1024' (defaults to 1024x1024)"
                    },
                    "output_filename": {
                        "type": "string",
                        "description": "Optional filename for saving (e.g. 'image.png')"
                    },
                    "input_image": {
                        "type": "string",
                        "description": "Optional local source image path for image-to-image or editing"
                    },
                    "mask_image": {
                        "type": "string",
                        "description": "Optional local mask image path. When provided together with input_image, requests masked editing/inpainting"
                    }
                },
                "required": ["prompt"]
            }),
            parameters_ts: Some("interface GenerateImageArgs { \n  prompt: string; \n  size?: '1024x1024'|'1024x1792'|'1792x1024'; \n  output_filename?: string; \n  input_image?: string; \n  mask_image?: string; \n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this to create a new image from text, or to edit an existing local image by supplying `input_image`. Add `mask_image` to request masked local editing when the backend supports it. Returns the saved image path.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: GenerateImageArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: format!("Invalid arguments: {}", e),
            })?;

        // Parse size
        let size_parts: Vec<u32> = args
            .size
            .split('x')
            .filter_map(|s| s.parse().ok())
            .collect();
        let size = if size_parts.len() == 2 {
            (size_parts[0], size_parts[1])
        } else {
            (1024, 1024)
        };

        // Execution via Unified Backend
        let diffusion = DiffusionConfig::default();
        let image = match (&args.input_image, &args.mask_image) {
            (Some(input_path), Some(mask_path)) => {
                let initial_image = image::open(input_path).map_err(|e| {
                    anyhow::anyhow!("Failed to open input_image '{}': {}", input_path, e)
                })?;
                let mask_image = image::open(mask_path).map_err(|e| {
                    anyhow::anyhow!("Failed to open mask_image '{}': {}", mask_path, e)
                })?;
                self.backend
                    .generate_image_inpainting(&args.prompt, &initial_image, &mask_image, diffusion)
                    .await
            }
            (Some(input_path), None) => {
                let initial_image = image::open(input_path).map_err(|e| {
                    anyhow::anyhow!("Failed to open input_image '{}': {}", input_path, e)
                })?;
                self.backend
                    .generate_image_img2img(&args.prompt, &initial_image, diffusion)
                    .await
            }
            (None, Some(_)) => {
                return Err(anyhow::anyhow!(
                    "`mask_image` requires `input_image` to be provided as well."
                ));
            }
            (None, None) => self.backend.generate_image(&args.prompt, size, diffusion).await,
        }
        .map_err(|e| {
            let message = e.to_string();
            if message.contains("No image generation backend available") {
                anyhow::anyhow!(
                    "Image generation is unavailable in the current runtime. Configure a real image backend (for example `sensory.image_gen_model`) before using `generate_image`."
                )
            } else {
                anyhow::anyhow!("Image Generation failed: {}", e)
            }
        })?;

        // Save to file
        let output_filename = args
            .output_filename
            .unwrap_or_else(|| format!("image_{}.png", chrono::Utc::now().timestamp()));

        let output_path = if Path::new(&output_filename).is_absolute() {
            PathBuf::from(output_filename)
        } else {
            self.output_dir.join(output_filename)
        };

        // Ensure parent dir exists
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        image
            .save(&output_path)
            .map_err(|e| anyhow::anyhow!("Failed to save image to disk: {}", e))?;

        Ok(format!(
            "🖼️ Image successfully generated (via {}) and saved to: {}",
            self.backend.model_info(),
            output_path.to_string_lossy()
        ))
    }
}
