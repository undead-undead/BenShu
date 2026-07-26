use crate::protocol::SensoryOutput;
use crate::vision::VisionPlugin;
use anyhow::Result;
use async_trait::async_trait;
use benshu_runtimes::WasmRuntime;
use image::DynamicImage;
use std::fs;
use std::path::{Path, PathBuf};

/// Embedded OCR assets
const TESSERACT_WASM: &[u8] = include_bytes!("../../assets/ocr/tesseract.wasm");
const ENG_TRAINEDDATA: &[u8] = include_bytes!("../../assets/ocr/eng.traineddata");
const CHI_SIM_TRAINEDDATA: &[u8] = include_bytes!("../../assets/ocr/chi_sim.traineddata");

/// OCR Plugin using Tesseract WASM (migrated from engram)
pub struct WasmOCR {
    runtime: WasmRuntime,
    wasm_path: PathBuf,
}

impl WasmOCR {
    pub fn new() -> Result<Self> {
        let wasm_path = Self::ensure_assets()?;
        Ok(Self {
            runtime: WasmRuntime::new(),
            wasm_path,
        })
    }

    /// Primary asset extraction logic (Zero-Dependency)
    pub fn ensure_assets() -> Result<PathBuf> {
        let data_dir = if let Ok(dir) = std::env::var("BENSHU_DATA_DIR") {
            PathBuf::from(dir)
        } else {
            dirs::data_local_dir()
                .map(|d| d.join("benshu").join("data"))
                .unwrap_or_else(|| PathBuf::from("data"))
        };

        let models_dir = data_dir.join("models");
        if !models_dir.exists() {
            fs::create_dir_all(&models_dir)?;
        }

        let wasm_path = models_dir.join("tesseract.wasm");
        let eng_path = models_dir.join("eng.traineddata");
        let chi_path = models_dir.join("chi_sim.traineddata");

        let write_if_missing = |path: &Path, data: &[u8], name: &str| -> Result<()> {
            if !path.exists() {
                tracing::info!("Extracting sensory OCR asset: {}", name);
                fs::write(path, data)?;
            }
            Ok(())
        };

        write_if_missing(&wasm_path, TESSERACT_WASM, "tesseract.wasm")?;
        write_if_missing(&eng_path, ENG_TRAINEDDATA, "eng.traineddata")?;
        write_if_missing(&chi_path, CHI_SIM_TRAINEDDATA, "chi_sim.traineddata")?;

        Ok(wasm_path)
    }
}

#[async_trait]
impl VisionPlugin for WasmOCR {
    fn name(&self) -> &str {
        "tesseract-wasm"
    }

    async fn process(&self, image: &DynamicImage, prompt: Option<&str>) -> Result<SensoryOutput> {
        self.load().await?;
        // 1. Determine language
        let lang = prompt.unwrap_or("eng");

        // 2. Save image to temp for WASM (WASI model)
        let temp_dir = std::env::temp_dir().join("benshu_sensory_ocr");
        if !temp_dir.exists() {
            fs::create_dir_all(&temp_dir)?;
        }

        let img_id = uuid::Uuid::new_v4().to_string();
        let img_path = temp_dir.join(format!("{}.png", img_id));
        image.save(&img_path)?;

        // 3. Prepare WASM arguments
        let args_json = serde_json::json!({
            "image_path": img_path.to_string_lossy(),
            "lang": lang
        });

        // 4. Execute WASM
        let models_dir = self
            .wasm_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid WASM path"))?;

        // We mount models_dir as base_dir so WASM finds .traineddata at ./eng.traineddata
        let output = self
            .runtime
            .call(
                &self.wasm_path,
                &args_json.to_string(),
                &models_dir.to_path_buf(),
            )
            .await?;

        // 5. Cleanup temp image
        let _ = fs::remove_file(img_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("WASM OCR failed: {}", stderr);
        }

        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            anyhow::bail!("WASM OCR returned empty text");
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
        128 * 1024 * 1024 // ~128MB for Wasmtime + Tesseract
    }
}
