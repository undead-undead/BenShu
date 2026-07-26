pub mod calibration;
pub mod clip;
pub mod cloud;
pub mod ocr;
pub mod unified;
pub mod utils;
pub use calibration::ViewportScale;
pub use clip::ClipPlugin;
pub use ocr::WasmOCR;
pub use unified::UnifiedVisionPlugin;

use crate::protocol::{DetectedElement, SensoryOutput};
use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;

/// Standard interface for any Vision-based perception module.
/// Can be a LLaVA model, CLIP, or a simple OCR engine.
#[async_trait]
pub trait VisionPlugin: Send + Sync {
    /// Return the unique identifier for this plugin (e.g., "llava-v1.5", "clip-vit-b32").
    fn name(&self) -> &str;

    /// Process the image and return the structured output.
    async fn process(
        &self,
        image: &image::DynamicImage,
        prompt: Option<&str>,
    ) -> Result<SensoryOutput>;

    /// Load weights into memory/VRAM.
    async fn load(&self) -> Result<()>;

    /// Unload weights to free up resources.
    fn unload(&self);

    /// Check if weights are currently loaded.
    fn is_loaded(&self) -> bool;

    /// Estimate the VRAM/Memory usage of this plugin in bytes.
    /// Used by the Sensory Hub for resource arbitration.
    fn estimated_memory_usage(&self) -> u64;
}

/// Helper structure for common vision tasks.
pub enum VisionTask {
    Describe,          // General "What's in this image?"
    Grounding,         // Locate elements (SOM / Bounding Boxes)
    OCR,               // Extract text
    FeatureExtraction, // Get embedding vectors (CLIP)
}
