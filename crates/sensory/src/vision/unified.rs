use crate::protocol::SensoryOutput;
use crate::vision::VisionPlugin;
use anyhow::{Context, Result};
use async_trait::async_trait;
use benshu_inference::backend::OcrBackend;
use image::DynamicImage;
use std::sync::Arc;

/// Unified Vision Plugin that can wrap ANY Inference-Factory OcrBackend or VisionModelBackend.
pub struct UnifiedVisionPlugin {
    name: String,
    ocr: Option<Arc<dyn OcrBackend>>,
}

impl UnifiedVisionPlugin {
    pub fn for_ocr(name: String, backend: Arc<dyn OcrBackend>) -> Self {
        Self {
            name,
            ocr: Some(backend),
        }
    }
}

#[async_trait]
impl VisionPlugin for UnifiedVisionPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self, image: &DynamicImage, prompt: Option<&str>) -> Result<SensoryOutput> {
        let ocr = self
            .ocr
            .as_ref()
            .context("Plugin is not configured for OCR")?;

        // 1. Validate image dimensions
        if image.width() == 0 || image.height() == 0 {
            anyhow::bail!(
                "[OCR: {}] Invalid image provided (zero dimensions)",
                self.name
            );
        }

        let info = format!("{}x{}", image.width(), image.height());

        // 2. Perform OCR
        let text = ocr.recognize(image).await.map_err(|e| {
            anyhow::anyhow!(
                "[OCR: {}] Extraction failed for image ({}): {}",
                self.name,
                info,
                e
            )
        })?;

        if text.is_empty() {
            tracing::warn!("[OCR: {}] No text detected in {} image", self.name, info);
        }

        Ok(SensoryOutput::Text(text))
    }

    async fn load(&self) -> Result<()> {
        Ok(())
    }
    fn unload(&self) {}
    fn is_loaded(&self) -> bool {
        true
    }

    fn estimated_memory_usage(&self) -> u64 {
        self.ocr
            .as_ref()
            .map(|o| o.estimated_memory_usage())
            .unwrap_or(0)
    }
}
