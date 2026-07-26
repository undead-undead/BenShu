use crate::backend::{InferenceError, OcrBackend, Result};
use async_trait::async_trait;
use std::process::Command;
use tokio::process::Command as TokioCommand;

pub struct TesseractBackend {
    lang: String,
}

impl TesseractBackend {
    pub fn new(lang: impl Into<String>) -> Self {
        Self { lang: lang.into() }
    }

    async fn check_available() -> bool {
        which::which("tesseract").is_ok()
    }
}

#[async_trait]
impl OcrBackend for TesseractBackend {
    fn model_info(&self) -> String {
        format!("Tesseract (lang: {})", self.lang)
    }

    async fn recognize(&self, image: &image::DynamicImage) -> Result<String> {
        let temp_dir = std::env::temp_dir();
        let id = uuid::Uuid::new_v4();
        let img_path = temp_dir.join(format!("ocr_{}.png", id));
        image
            .save(&img_path)
            .map_err(|e| InferenceError::Execution(e.to_string(), "ocr".to_string()))?;

        let output = TokioCommand::new("tesseract")
            .arg(&img_path)
            .arg("stdout")
            .arg("-l")
            .arg(&self.lang)
            .output()
            .await
            .map_err(|e| InferenceError::Execution(e.to_string(), "ocr".to_string()))?;

        let _ = std::fs::remove_file(&img_path);

        if !output.status.success() {
            return Err(InferenceError::Execution(
                String::from_utf8_lossy(&output.stderr).to_string(),
                "ocr".to_string(),
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn estimated_memory_usage(&self) -> u64 {
        // Tesseract CLI is relatively lightweight
        64 * 1024 * 1024
    }

    fn device_info(&self) -> crate::backend::DeviceType {
        crate::backend::DeviceType::Cpu
    }
}
