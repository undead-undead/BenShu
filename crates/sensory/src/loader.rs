use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct ModelConfig {
    #[serde(rename = "model_type")]
    pub architecture: String,
    pub hidden_size: Option<usize>,
    // Add more common fields as needed
}

/// Utility for unified weight loading and configuration mapping
pub struct WeightLoader {
    base_dir: PathBuf,
}

impl WeightLoader {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Ensure model directory exists and contains necessary files
    pub fn resolve_model(&self, model_id: &str) -> Result<PathBuf> {
        let path = self.base_dir.join(model_id);
        if !path.exists() {
            // In a real system, this might trigger a download or return a specific error
            anyhow::bail!("Model not found: {}", model_id);
        }
        Ok(path)
    }

    /// Load config.json and map to standard architecture
    pub fn load_config(&self, model_path: &Path) -> Result<ModelConfig> {
        let config_file = model_path.join("config.json");
        let content = std::fs::read_to_string(config_file)?;
        let config: ModelConfig = serde_json::from_str(&content)?;
        Ok(config)
    }
}
