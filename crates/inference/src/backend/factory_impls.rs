use crate::backend::{
    BackendCapability, BackendFactory, BackendSource, EmbeddingBackend, ImageGenBackend,
    InferenceError, InferenceFactory, ModelBackend, ModelRole, OcrBackend, RerankBackend, Result,
    SttBackend, TtsBackend,
};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

/// Generic Cloud Factory (Phase 22)
/// Handles GPT-4, Claude, Gemini, etc. via the 'api:provider/model' syntax.
pub struct CloudBackendFactory {
    pub cap: BackendCapability,
}

impl CloudBackendFactory {
    pub fn new(cap: BackendCapability) -> Self {
        Self { cap }
    }
}

#[async_trait]
impl BackendFactory for CloudBackendFactory {
    fn capability(&self) -> BackendCapability {
        self.cap
    }

    fn source(&self) -> BackendSource {
        BackendSource::Cloud
    }

    fn registered_roles(&self) -> Vec<ModelRole> {
        match self.cap {
            BackendCapability::LLM => vec![ModelRole::Llm, ModelRole::Slm],
            BackendCapability::Vision => vec![ModelRole::Vlm],
            BackendCapability::Embedding => vec![ModelRole::Embedding],
            BackendCapability::Rerank => vec![ModelRole::Rerank],
            BackendCapability::STT => vec![ModelRole::Stt],
            BackendCapability::TTS => vec![ModelRole::Tts],
            BackendCapability::OCR => vec![ModelRole::Ocr],
            BackendCapability::NLU
            | BackendCapability::FactCheck
            | BackendCapability::ImageGeneration => Vec::new(),
        }
    }

    fn can_handle(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        path_str.starts_with("api:") || path_str.starts_with("http")
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (0, 0)
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let path_str = path.to_string_lossy();
        let backend = Arc::new(crate::backend::cloud::CloudBackend::from_path(&path_str)?);

        match self.cap {
            BackendCapability::ImageGeneration => {
                let gen: Arc<dyn ImageGenBackend> = backend;
                Ok(Box::new(gen))
            }
            BackendCapability::TTS => {
                let tts: Arc<dyn TtsBackend> = backend;
                Ok(Box::new(tts))
            }
            BackendCapability::STT => {
                let stt: Arc<dyn SttBackend> = backend;
                Ok(Box::new(stt))
            }
            BackendCapability::Embedding => {
                let embed: Arc<dyn EmbeddingBackend> = backend;
                Ok(Box::new(embed))
            }
            BackendCapability::Rerank => {
                let rerank: Arc<dyn RerankBackend> = backend;
                Ok(Box::new(rerank))
            }
            _ => {
                let model: Arc<dyn ModelBackend> = backend;
                Ok(Box::new(model))
            }
        }
    }
}

/// OpenAI-compatible image bridge factory.
/// Handles `bridge-image:http://host:port/v1|model-name`.
pub struct OpenAiImageBridgeFactory;

#[async_trait]
impl BackendFactory for OpenAiImageBridgeFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::ImageGeneration
    }

    fn source(&self) -> BackendSource {
        BackendSource::Cloud
    }

    fn can_handle(&self, path: &Path) -> bool {
        crate::backend::openai_image_bridge::OpenAiImageBridgeBackend::can_handle_path(path)
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (0, 0)
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let backend: Arc<dyn ImageGenBackend> = Arc::new(
            crate::backend::openai_image_bridge::OpenAiImageBridgeBackend::from_path(path)?,
        );
        Ok(Box::new(backend))
    }
}

/// Local Candle Factory (Phase 22)
/// Handles safetensors-based local models (LLama, Phi, etc.)
pub struct LocalCandleFactory;

#[async_trait]
impl BackendFactory for LocalCandleFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::LLM
    }

    fn registered_roles(&self) -> Vec<ModelRole> {
        vec![ModelRole::Llm, ModelRole::Slm, ModelRole::Vlm]
    }

    fn resolved_roles(&self, path: &Path, mmproj_path: Option<&Path>) -> Vec<ModelRole> {
        let has_vision = mmproj_path.is_some()
            || path.join("mmproj.safetensors").exists()
            || path.join("vision_encoder.safetensors").exists();
        if has_vision {
            vec![ModelRole::Llm, ModelRole::Slm, ModelRole::Vlm]
        } else {
            vec![ModelRole::Llm, ModelRole::Slm]
        }
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.is_dir()
            && (path.join("config.json").exists() || path.join("model.safetensors").exists())
    }

    fn estimate_usage(&self, path: &Path) -> (u64, u64) {
        // Estimate based on the size of .safetensors files
        let mut total_size = 0;
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("safetensors") {
                    total_size += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
        }

        // VLM check: if vision components exist, add overhead
        if path.join("mmproj.safetensors").exists()
            || path.join("vision_encoder.safetensors").exists()
        {
            total_size += 512 * 1024 * 1024; // Extra 512MB for vision encoder
        }

        // Add 20% buffer for KV Cache and activation overhead
        let vram = (total_size as f64 * 1.2) as u64;
        (vram, 256 * 1024 * 1024) // 256MB RAM base
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let device = InferenceFactory::detect_best_device();

        if path.join("mmproj.safetensors").exists()
            || path.join("vision_encoder.safetensors").exists()
        {
            let vlm: Arc<dyn ModelBackend> = Arc::new(
                crate::backend::vlm_candle::CandleVlmBackend::load_local(path, device)?,
            );
            Ok(Box::new(vlm))
        } else {
            let backend: Arc<dyn ModelBackend> = Arc::new(
                crate::backend::candle::CandleBackend::load_local(path, device)?,
            );
            Ok(Box::new(backend))
        }
    }
}

/// Local llama.cpp Factory
#[cfg(feature = "llama_cpp")]
pub struct LocalLlamaCppFactory;

#[cfg(feature = "llama_cpp")]
#[async_trait]
impl BackendFactory for LocalLlamaCppFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::LLM
    }

    fn registered_roles(&self) -> Vec<ModelRole> {
        vec![ModelRole::Llm, ModelRole::Slm, ModelRole::Vlm]
    }

    fn resolved_roles(&self, _path: &Path, mmproj_path: Option<&Path>) -> Vec<ModelRole> {
        if mmproj_path.is_some() {
            vec![ModelRole::Llm, ModelRole::Slm, ModelRole::Vlm]
        } else {
            vec![ModelRole::Llm, ModelRole::Slm]
        }
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("gguf")
    }

    fn estimate_usage(&self, path: &Path) -> (u64, u64) {
        let file_size = path.metadata().map(|m| m.len()).unwrap_or(0);
        let vram = (file_size as f64 * 1.2) as u64;
        (vram, 256 * 1024 * 1024)
    }

    async fn create(
        &self,
        path: &Path,
        mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let backend: Arc<dyn ModelBackend> =
            Arc::new(crate::backend::llama_cpp::LlamaCppBackend::new(
                path.to_path_buf(),
                mmproj.map(|p| p.to_path_buf()),
            )?);
        Ok(Box::new(backend))
    }
}

/// Local TTS Factory (Piper)
pub struct LocalTtsFactory;

#[async_trait]
impl BackendFactory for LocalTtsFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::TTS
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.join("model.onnx").exists()
            || (path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("onnx"))
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (0, 128 * 1024 * 1024) // 128MB RAM for Piper
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let backend: Arc<dyn TtsBackend> =
            Arc::new(crate::backend::audio_external::PiperBackend::new(
                path,
                path.to_string_lossy().to_string(),
            )?);
        Ok(Box::new(backend))
    }
}

/// Local Embedding Factory (BERT-style)
pub struct WindowsNativeOnnxEmbeddingFactory;

#[async_trait]
impl BackendFactory for WindowsNativeOnnxEmbeddingFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::Embedding
    }

    fn registered_roles(&self) -> Vec<ModelRole> {
        vec![ModelRole::Embedding]
    }

    fn can_handle(&self, path: &Path) -> bool {
        (path.is_dir() && path.join("model.onnx").exists())
            || (path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("onnx"))
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (256 * 1024 * 1024, 256 * 1024 * 1024)
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let runtime = crate::detect_windows_native_runtime_status();
        if runtime.small_model_runtime_readiness != "windows_native_ready" {
            let diagnosis = crate::diagnose_windows_native_small_model_error(
                Some(path),
                &InferenceError::LoadFailed(runtime.small_model_runtime_reason.clone()),
            );
            return Err(InferenceError::LoadFailed(format!(
                "Cannot activate Windows-native ONNX embedding runtime for {:?}: {} ({}) [windows_native_outcome={} strategy={}] {}",
                path,
                runtime.small_model_runtime_reason,
                runtime.small_model_runtime_readiness,
                diagnosis.outcome,
                diagnosis.strategy,
                diagnosis.note,
            )));
        }
        let model_id = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("windows-native-onnx-embedding")
            .to_string();
        let backend: Arc<dyn EmbeddingBackend> = Arc::new(
            crate::backend::onnx_runtime::WindowsNativeOnnxEmbeddingBackend::load(
                path,
                model_id.clone(),
            )
            .map_err(|err| {
                let diagnosis = crate::diagnose_windows_native_small_model_error(Some(path), &err);
                InferenceError::LoadFailed(format!(
                    "Failed to load Windows-native ONNX embedding runtime for {:?} [windows_native_outcome={} strategy={}] {}",
                    path, diagnosis.outcome, diagnosis.strategy, diagnosis.note
                ))
            })?,
        );
        Ok(Box::new(backend))
    }
}

/// Local Embedding Factory (BERT-style)
pub struct LocalEmbeddingFactory;

#[async_trait]
impl BackendFactory for LocalEmbeddingFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::Embedding
    }

    fn registered_roles(&self) -> Vec<ModelRole> {
        vec![ModelRole::Embedding]
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.is_dir()
            && path.join("config.json").exists()
            && path.join("tokenizer.json").exists()
            && path.join("model.safetensors").exists()
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (400 * 1024 * 1024, 256 * 1024 * 1024)
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let model_id = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("local-embedding")
            .to_string();
        let backend: Arc<dyn EmbeddingBackend> = Arc::new(
            crate::backend::embeddings::BertEmbeddingBackend::load(path, model_id)
                .map_err(|e| InferenceError::LoadFailed(e.to_string()))?,
        );
        Ok(Box::new(backend))
    }
}

/// Local Rerank Factory (cross-encoder)
pub struct WindowsNativeOnnxRerankFactory;

#[async_trait]
impl BackendFactory for WindowsNativeOnnxRerankFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::Rerank
    }

    fn registered_roles(&self) -> Vec<ModelRole> {
        vec![ModelRole::Rerank]
    }

    fn can_handle(&self, path: &Path) -> bool {
        (path.is_dir() && path.join("model.onnx").exists())
            || (path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("onnx"))
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (256 * 1024 * 1024, 256 * 1024 * 1024)
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let runtime = crate::detect_windows_native_runtime_status();
        if runtime.small_model_runtime_readiness != "windows_native_ready" {
            let diagnosis = crate::diagnose_windows_native_small_model_error(
                Some(path),
                &InferenceError::LoadFailed(runtime.small_model_runtime_reason.clone()),
            );
            return Err(InferenceError::LoadFailed(format!(
                "Cannot activate Windows-native ONNX rerank runtime for {:?}: {} ({}) [windows_native_outcome={} strategy={}] {}",
                path,
                runtime.small_model_runtime_reason,
                runtime.small_model_runtime_readiness,
                diagnosis.outcome,
                diagnosis.strategy,
                diagnosis.note,
            )));
        }
        let model_id = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("windows-native-onnx-rerank")
            .to_string();
        let backend: Arc<dyn RerankBackend> = Arc::new(
            crate::backend::onnx_runtime::WindowsNativeOnnxRerankBackend::load(
                path,
                model_id.clone(),
            )
            .map_err(|err| {
                let diagnosis = crate::diagnose_windows_native_small_model_error(Some(path), &err);
                InferenceError::LoadFailed(format!(
                    "Failed to load Windows-native ONNX rerank runtime for {:?} [windows_native_outcome={} strategy={}] {}",
                    path, diagnosis.outcome, diagnosis.strategy, diagnosis.note
                ))
            })?,
        );
        Ok(Box::new(backend))
    }
}

/// Local Rerank Factory (cross-encoder)
pub struct LocalRerankFactory;

#[async_trait]
impl BackendFactory for LocalRerankFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::Rerank
    }

    fn registered_roles(&self) -> Vec<ModelRole> {
        vec![ModelRole::Rerank]
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.is_dir()
            && path.join("config.json").exists()
            && path.join("tokenizer.json").exists()
            && path.join("model.safetensors").exists()
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (500 * 1024 * 1024, 256 * 1024 * 1024)
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let model_id = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("local-reranker")
            .to_string();
        let backend: Arc<dyn RerankBackend> = Arc::new(
            crate::backend::rerank::CandleRerankBackend::load(path, model_id)
                .map_err(|e| InferenceError::LoadFailed(e.to_string()))?,
        );
        Ok(Box::new(backend))
    }
}

/// Local STT Factory (Whisper)
pub struct LocalSttFactory;

#[async_trait]
impl BackendFactory for LocalSttFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::STT
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.join("model.bin").exists() || path.join("model.safetensors").exists()
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (512 * 1024 * 1024, 256 * 1024 * 1024) // Whisper Medium ~512MB VRAM
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let backend = crate::backend::audio_candle::WhisperCandleBackend::new(
            path,
            path.to_string_lossy().to_string(),
        )
        .map_err(|e| InferenceError::LoadFailed(e.to_string()))?;
        let backend_arc: Arc<dyn SttBackend> = Arc::new(backend);
        Ok(Box::new(backend_arc))
    }
}

/// Local OCR Factory (system Tesseract)
pub struct LocalTesseractOcrFactory;

#[async_trait]
impl BackendFactory for LocalTesseractOcrFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::OCR
    }

    fn registered_roles(&self) -> Vec<ModelRole> {
        vec![ModelRole::Ocr]
    }

    fn can_handle(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        path_str.eq_ignore_ascii_case("tesseract")
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (0, 64 * 1024 * 1024)
    }

    async fn create(
        &self,
        _path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let backend: Arc<dyn OcrBackend> =
            Arc::new(crate::backend::ocr_tesseract::TesseractBackend::new("eng"));
        Ok(Box::new(backend))
    }
}

/// Local NLU Factory (Phase 23)
pub struct LocalNluFactory;

#[async_trait]
impl BackendFactory for LocalNluFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::NLU
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.is_dir()
            && path.join("config.json").exists()
            && path.join("model.safetensors").exists()
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        // BERT base is ~400MB
        (400 * 1024 * 1024, 256 * 1024 * 1024)
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        let model_id = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        // Default to non-quantized for optimal, we can add quantized detection later
        let backend = crate::backend::nlu_candle::CandleNluBackend::load(path, model_id, false)
            .map_err(|e| InferenceError::LoadFailed(e.to_string()))?;
        let backend_arc: Arc<dyn benshu_infra::traits::nlu::NluEngine> = Arc::new(backend);
        Ok(Box::new(backend_arc))
    }
}

/// Local Fact Check Factory (NLI)
pub struct LocalFactCheckFactory;

#[async_trait]
impl BackendFactory for LocalFactCheckFactory {
    fn capability(&self) -> BackendCapability {
        BackendCapability::FactCheck
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.join("config.json").exists()
            && (path.join("model.safetensors").exists() || path.join("model.bin").exists())
    }

    fn estimate_usage(&self, _path: &Path) -> (u64, u64) {
        (400 * 1024 * 1024, 200 * 1024 * 1024) // BERT Tiny-Base ~400MB
    }

    async fn create(
        &self,
        path: &Path,
        _mmproj: Option<&Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>> {
        use benshu_infra::traits::validation::FactChecker;
        let model_id = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let backend = crate::backend::validation_candle::CandleFactChecker::load(path, model_id)
            .map_err(|e| InferenceError::LoadFailed(e.to_string()))?;
        let backend_arc: Arc<dyn FactChecker> = Arc::new(backend);
        Ok(Box::new(backend_arc))
    }
}
