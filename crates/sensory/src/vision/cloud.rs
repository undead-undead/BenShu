use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;
use std::sync::Arc;

use crate::protocol::SensoryOutput;
use crate::vision::VisionPlugin;

/// A plugin that delegages vision tasks to a cloud provider.
/// In a real implementation, this delegates to the configured cloud vision model.
pub struct CloudVisionPlugin {
    name: String,
    // We use a closure or a trait object to avoid circular dependency on benshu-brain
    handler: Arc<dyn CloudVisionHandler>,
}

#[async_trait]
pub trait CloudVisionHandler: Send + Sync {
    async fn analyze(&self, image: &DynamicImage, prompt: Option<&str>) -> Result<String>;
}

impl CloudVisionPlugin {
    pub fn new(name: impl Into<String>, handler: Arc<dyn CloudVisionHandler>) -> Self {
        Self {
            name: name.into(),
            handler,
        }
    }
}

#[async_trait]
impl VisionPlugin for CloudVisionPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    async fn load(&self) -> Result<()> {
        Ok(())
    }
    fn unload(&self) {}
    fn is_loaded(&self) -> bool {
        true
    }

    async fn process(
        &self,
        image: &image::DynamicImage,
        prompt: Option<&str>,
    ) -> Result<SensoryOutput> {
        let raw_prompt = prompt.unwrap_or("Describe this image.");
        let res = self.handler.analyze(image, Some(raw_prompt)).await?;

        // Detect if it was a grounding task
        let grounding_keywords = [
            "@e", "position", "点击", "坐标", "位置", "SOM", "locate", "where",
        ];
        if grounding_keywords.iter().any(|k| raw_prompt.contains(k)) {
            if let Some(coords) = crate::vision::utils::CoordinateParser::parse_from_text(
                &res,
                image.width(),
                image.height(),
            ) {
                return Ok(coords);
            }
        }

        Ok(SensoryOutput::Text(res))
    }

    fn estimated_memory_usage(&self) -> u64 {
        0 // Cloud plugins don't use local VRAM
    }
}
