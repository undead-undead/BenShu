//! Backend trait and common types for AI inference.

use crate::engine::KvEngine;
use async_trait::async_trait;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;

pub mod audio_candle;
pub mod audio_external;
pub mod audio_preprocess;
pub mod candle;
pub mod clip;
pub mod cloud;
pub mod direct_storage;
pub mod embeddings;
pub mod factory_impls;
#[cfg(feature = "llama_cpp")]
pub mod llama_cpp;
pub mod nlu;
pub mod nlu_candle;
pub mod ocr;
pub mod ocr_tesseract;
pub mod onnx_runtime;
pub mod openai_image_bridge;
pub mod projector;
pub mod rerank;
#[cfg(feature = "tensorrt")]
pub mod tensorrt;
pub mod validation;
pub mod validation_candle;
pub mod video;
pub mod vlm_candle;

#[derive(Debug, Error, Clone)]
pub enum InferenceError {
    #[error("Model not found: {0}")]
    NotFound(String),
    #[error("Execution error (req={1}): {0}")]
    Execution(String, String),
    #[error("Model loading failed: {0}")]
    LoadFailed(String),
    #[error("KV Cache error: {0}")]
    CacheError(String),
    #[error("Inference timed out (req={1}): {0}")]
    Timeout(String, String),
    #[error("Resource exhausted (OOM/Context): {0}")]
    ResourceExhausted(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Temporary failure (retryable): {0}")]
    Temporary(String),
    #[error("Internal engine error: {0}")]
    Internal(String),
    #[error("Path validation failed for {0}: {1}")]
    PathError(String, String),
    #[error("Backend communication failed: {0}")]
    BackendError(String),
    #[error("Data format/parsing error: {0}")]
    FormatError(String),
}

impl InferenceError {
    pub fn execution<S1: Into<String>, S2: Into<String>>(msg: S1, req_id: S2) -> Self {
        Self::Execution(msg.into(), req_id.into())
    }

    pub fn execution_with_context<S1, S2, S3, S4>(
        msg: S1,
        req_id: S2,
        module: S3,
        model: S4,
    ) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
        S3: Into<String>,
        S4: Into<String>,
    {
        Self::Execution(
            format!(
                "{} [module={} model={}]",
                msg.into(),
                module.into(),
                model.into()
            ),
            req_id.into(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RequestType {
    Text,
    Vision,
    Video,
    Audio,
}

impl From<anyhow::Error> for InferenceError {
    fn from(err: anyhow::Error) -> Self {
        InferenceError::Internal(err.to_string())
    }
}

impl From<candle_core::Error> for InferenceError {
    fn from(err: candle_core::Error) -> Self {
        InferenceError::Execution(err.to_string(), "internal".to_string())
    }
}

pub type Result<T> = std::result::Result<T, InferenceError>;

/// Hardware accelerator types for model backends
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeviceType {
    Cpu,
    Gpu,
    Cuda(u32),
    Metal(u32),
    Vulkan(u32),
    Wasm,
    Cloud,
}

impl DeviceType {
    pub fn is_gpu(&self) -> bool {
        matches!(
            self,
            DeviceType::Gpu | DeviceType::Cuda(_) | DeviceType::Metal(_) | DeviceType::Vulkan(_)
        )
    }
}

impl From<&candle_core::Device> for DeviceType {
    fn from(device: &candle_core::Device) -> Self {
        match device {
            candle_core::Device::Cpu => DeviceType::Cpu,
            candle_core::Device::Cuda(_) => DeviceType::Cuda(0),
            #[cfg(feature = "metal")]
            candle_core::Device::Metal(_) => DeviceType::Metal(0),
            _ => DeviceType::Cpu,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenerationConfig {
    pub max_new_tokens: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub use_cache: bool,
    pub seed: u32,
    pub session_id: Option<String>,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub stop_sequences: Vec<String>,
    /// Session priority: -128 (Critical/Real-time) to 127 (Idle/Background)
    /// Used for VRAM metabolic balancing during resource contention.
    pub priority: i8,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            max_new_tokens: 512,
            temperature: 0.7,
            top_p: 0.9,
            use_cache: true,
            seed: 42,
            session_id: None,
            timeout_secs: Some(60),
            stop_sequences: Vec::new(),
            priority: 0,
        }
    }
}

#[async_trait]
pub trait ModelBackend: Send + Sync {
    fn is_quantized(&self) -> bool {
        false
    }
    /// Generate a full completion
    async fn generate(
        &self,
        request_id: &str,
        prompt: &str,
        images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        kv_engine: Arc<RwLock<KvEngine>>,
    ) -> Result<String>;

    /// Stream generation results
    async fn stream_generate(
        &self,
        request_id: &str,
        prompt: &str,
        images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        kv_engine: Arc<RwLock<KvEngine>>,
        tx: mpsc::Sender<Result<String>>,
    ) -> Result<()>;

    /// Get backend metadata/info
    fn model_info(&self) -> String;

    /// Hardware accelerator type
    fn device_info(&self) -> DeviceType;

    /// Estimate memory usage in bytes
    fn estimated_memory_usage(&self) -> u64;

    /// Downcast support (Phase 21.10)
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Specialized tasks for vision-language models
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VisionTask {
    Describe,
    OCR,
    Grounding,
}

#[async_trait]
pub trait VisionModelBackend: ModelBackend {
    async fn vision_analyze(
        &self,
        image: &image::DynamicImage,
        task: VisionTask,
        prompt: Option<&str>,
        config: Option<GenerationConfig>,
    ) -> Result<String>;

    async fn vision_analyze_video(
        &self,
        frames: &[image::DynamicImage],
        prompt: Option<&str>,
        config: Option<GenerationConfig>,
    ) -> Result<String>;
}

/// Generic Audio completion backends
#[async_trait]
pub trait AudioModelBackend: Send + Sync {
    fn model_info(&self) -> String;
    fn estimated_memory_usage(&self) -> u64;
    fn is_quantized(&self) -> bool {
        false
    }
    fn device_info(&self) -> DeviceType {
        DeviceType::Cpu
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffusionConfig {
    pub steps: usize,
    pub guidance_scale: f32,
    pub seed: u32,
    pub strength: f32, // Used for Img2Img (0.0 - 1.0)
    pub negative_prompt: Option<String>,
}

impl Default for DiffusionConfig {
    fn default() -> Self {
        Self {
            steps: 25,
            guidance_scale: 7.5,
            seed: 42,
            strength: 0.7,
            negative_prompt: None,
        }
    }
}

#[async_trait]
pub trait ImageGenBackend: Send + Sync {
    /// Get backend identity
    fn model_info(&self) -> String;

    /// Generate an image from a prompt (Text-to-Image)
    async fn generate_image(
        &self,
        prompt: &str,
        size: (u32, u32),
        config: DiffusionConfig,
    ) -> Result<image::DynamicImage>;

    /// Modify an existing image (Image-to-Image)
    async fn generate_image_img2img(
        &self,
        prompt: &str,
        initial_image: &image::DynamicImage,
        config: DiffusionConfig,
    ) -> Result<image::DynamicImage>;

    /// Repair or modify a specific region (Inpainting)
    async fn generate_image_inpainting(
        &self,
        prompt: &str,
        initial_image: &image::DynamicImage,
        mask: &image::DynamicImage,
        config: DiffusionConfig,
    ) -> Result<image::DynamicImage>;

    /// Estimate memory usage
    fn estimated_memory_usage(&self) -> u64;

    /// Return the device type
    fn device_info(&self) -> DeviceType;
}

#[async_trait]
pub trait SttBackend: AudioModelBackend {
    /// Transcribe raw PCM audio (16kHz f32)
    async fn transcribe(&self, pcm_data: &[f32]) -> Result<String>;
}

#[async_trait]
pub trait TtsBackend: AudioModelBackend {
    /// Synthesize text to raw audio bytes
    async fn synthesize(&self, text: &str) -> Result<Vec<u8>>;
}

/// Specialized tasks for Optical Character Recognition (Phase 21 Implementation)
#[async_trait]
pub trait OcrBackend: Send + Sync {
    /// Get backend identity
    fn model_info(&self) -> String;

    /// Recognize text from a single image
    async fn recognize(&self, image: &image::DynamicImage) -> Result<String>;

    fn estimated_memory_usage(&self) -> u64;
    fn is_quantized(&self) -> bool {
        false
    }
    fn device_info(&self) -> DeviceType {
        DeviceType::Cpu
    }

    /// Perform localized OCR if supported (returns text regions/bboxes)
    async fn recognize_with_layout(
        &self,
        _image: &image::DynamicImage,
    ) -> Result<serde_json::Value> {
        Err(InferenceError::Internal(
            "Layout analysis not implemented for this backend".into(),
        ))
    }
}

/// Specialized tasks for Text Embeddings (Unified AI Abstraction)
#[async_trait]
pub trait EmbeddingBackend: Send + Sync {
    /// Get backend identity (e.g., "openai/text-embedding-3-small", "bert-base-uncased")
    fn model_info(&self) -> String;

    /// Return the embedding vector dimension
    fn dimension(&self) -> usize;

    /// Return the device type this backend is running on
    fn device_info(&self) -> DeviceType;

    /// Estimate memory usage in bytes
    fn estimated_memory_usage(&self) -> u64;

    /// Generate embedding vector for text
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Batch version of embed - backends should override this for performance
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for t in texts {
            results.push(self.embed(t).await?);
        }
        Ok(results)
    }
}

/// Specialized tasks for Reranking (Cross-Encoders)
#[async_trait]
pub trait RerankBackend: Send + Sync {
    /// Get backend identity
    fn model_info(&self) -> String;

    /// Return the device type
    fn device_info(&self) -> DeviceType;

    /// Estimate memory usage
    fn estimated_memory_usage(&self) -> u64;

    /// Compute similarity scores for a list of documents against a query
    async fn rerank(&self, query: &str, documents: &[String]) -> Result<Vec<f32>>;
}

/// Resource Controller (Phase 22 Implementation)
/// Prevents OOM by managing VRAM/RAM reservations before loading models.
pub struct ResourceController {
    max_vram: u64,
    max_ram: u64,
    separate_vram_pool: bool,
    reserved_vram: Arc<AtomicU64>,
    reserved_ram: Arc<AtomicU64>,
}

impl ResourceController {
    pub fn new() -> Self {
        let budgets = crate::hardware::HardwareStatus::detect().budgets();
        Self {
            max_vram: budgets.max_vram_bytes,
            max_ram: budgets.max_ram_bytes,
            separate_vram_pool: budgets.separate_vram_pool,
            reserved_vram: Arc::new(AtomicU64::new(0)),
            reserved_ram: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn check_and_reserve(&self, vram: u64, ram: u64) -> Result<ResourceGuard> {
        let current_vram = self.reserved_vram.load(std::sync::atomic::Ordering::SeqCst);
        let current_ram = self.reserved_ram.load(std::sync::atomic::Ordering::SeqCst);
        let effective_vram = if self.separate_vram_pool { vram } else { 0 };
        let effective_ram = if self.separate_vram_pool {
            ram
        } else {
            ram.saturating_add(vram)
        };

        if self.separate_vram_pool && current_vram + effective_vram > self.max_vram {
            return Err(InferenceError::ResourceExhausted(format!(
                "Insufficient VRAM: Requested {} bytes, but only {} remaining of {}",
                effective_vram,
                self.max_vram - current_vram,
                self.max_vram
            )));
        }

        if current_ram + effective_ram > self.max_ram {
            return Err(InferenceError::ResourceExhausted(format!(
                "Insufficient RAM: Requested {} bytes, but only {} remaining of {}",
                effective_ram,
                self.max_ram - current_ram,
                self.max_ram
            )));
        }

        if self.separate_vram_pool {
            self.reserved_vram
                .fetch_add(effective_vram, std::sync::atomic::Ordering::SeqCst);
        }
        self.reserved_ram
            .fetch_add(effective_ram, std::sync::atomic::Ordering::SeqCst);

        Ok(ResourceGuard {
            vram: effective_vram,
            ram: effective_ram,
            reserved_vram: self.reserved_vram.clone(),
            reserved_ram: self.reserved_ram.clone(),
        })
    }
}

pub struct ResourceGuard {
    vram: u64,
    ram: u64,
    reserved_vram: Arc<AtomicU64>,
    reserved_ram: Arc<AtomicU64>,
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        self.reserved_vram
            .fetch_sub(self.vram, std::sync::atomic::Ordering::SeqCst);
        self.reserved_ram
            .fetch_sub(self.ram, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_controller_folds_vram_into_ram_without_separate_pool() {
        let controller = ResourceController {
            max_vram: 0,
            max_ram: 1024,
            separate_vram_pool: false,
            reserved_vram: Arc::new(AtomicU64::new(0)),
            reserved_ram: Arc::new(AtomicU64::new(0)),
        };

        let _guard = controller.check_and_reserve(256, 256).expect("reserve");
        assert_eq!(
            controller
                .reserved_vram
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert_eq!(
            controller
                .reserved_ram
                .load(std::sync::atomic::Ordering::SeqCst),
            512
        );
    }

    #[test]
    fn resource_controller_uses_vram_pool_when_present() {
        let controller = ResourceController {
            max_vram: 1024,
            max_ram: 1024,
            separate_vram_pool: true,
            reserved_vram: Arc::new(AtomicU64::new(0)),
            reserved_ram: Arc::new(AtomicU64::new(0)),
        };

        let _guard = controller.check_and_reserve(256, 256).expect("reserve");
        assert_eq!(
            controller
                .reserved_vram
                .load(std::sync::atomic::Ordering::SeqCst),
            256
        );
        assert_eq!(
            controller
                .reserved_ram
                .load(std::sync::atomic::Ordering::SeqCst),
            256
        );
    }
}

/// Capability categories for Backend Factories
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BackendCapability {
    LLM,
    Vision,
    OCR,
    Embedding,
    Rerank,
    STT,
    TTS,
    NLU,
    FactCheck,
    ImageGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendSource {
    Local,
    Cloud,
}

impl BackendSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloud => "cloud",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Llm,
    Slm,
    Vlm,
    Embedding,
    Rerank,
    Nlu,
    FactCheck,
    Stt,
    Tts,
    Ocr,
}

impl ModelRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Llm => "llm",
            Self::Slm => "slm",
            Self::Vlm => "vlm",
            Self::Embedding => "embedding",
            Self::Rerank => "rerank",
            Self::Nlu => "nlu",
            Self::FactCheck => "fact_check",
            Self::Stt => "stt",
            Self::Tts => "tts",
            Self::Ocr => "ocr",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackendFactoryDescriptor {
    pub factory_id: String,
    pub source: BackendSource,
    pub capability: BackendCapability,
    pub declared_roles: Vec<ModelRole>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackendBindingDescriptor {
    pub factory_id: String,
    pub source: BackendSource,
    pub capability: BackendCapability,
    pub declared_roles: Vec<ModelRole>,
    pub model_path: String,
    pub mmproj_path: Option<String>,
    pub is_local: bool,
}

fn default_registered_roles(capability: BackendCapability) -> Vec<ModelRole> {
    match capability {
        BackendCapability::LLM => vec![ModelRole::Llm, ModelRole::Slm],
        BackendCapability::Vision => vec![ModelRole::Vlm],
        BackendCapability::OCR => vec![ModelRole::Ocr],
        BackendCapability::Embedding => vec![ModelRole::Embedding],
        BackendCapability::Rerank => vec![ModelRole::Rerank],
        BackendCapability::NLU => vec![ModelRole::Nlu],
        BackendCapability::FactCheck => vec![ModelRole::FactCheck],
        BackendCapability::STT => vec![ModelRole::Stt],
        BackendCapability::TTS => vec![ModelRole::Tts],
        BackendCapability::ImageGeneration => Vec::new(),
    }
}

/// Generic Factory Trait for AI Backends (Refined Phase 22)
#[async_trait]
pub trait BackendFactory: Send + Sync {
    /// Return the capability of this factory
    fn capability(&self) -> BackendCapability;

    /// Where this backend is provisioned from.
    fn source(&self) -> BackendSource {
        BackendSource::Local
    }

    /// Role declarations for registry / audit views.
    fn registered_roles(&self) -> Vec<ModelRole> {
        default_registered_roles(self.capability())
    }

    /// Resolve path-aware roles for the requested model binding.
    fn resolved_roles(
        &self,
        path: &std::path::Path,
        mmproj_path: Option<&std::path::Path>,
    ) -> Vec<ModelRole> {
        let _ = path;
        let _ = mmproj_path;
        self.registered_roles()
    }

    /// Returns true if this factory can handle the given path/ID
    fn can_handle(&self, path: &std::path::Path) -> bool;

    /// Estimate memory usage BEFORE loading the model
    fn estimate_usage(&self, path: &std::path::Path) -> (u64, u64); // (VRAM, RAM)

    /// Create the actual backend (Returns a trait object that must be downcasted)
    async fn create(
        &self,
        path: &std::path::Path,
        mmproj_path: Option<&std::path::Path>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>>;
}

lazy_static::lazy_static! {
    pub static ref RESOURCES: ResourceController = ResourceController::new();
    static ref MODEL_BACKEND_CACHE: DashMap<String, Arc<dyn ModelBackend>> = DashMap::new();
}

/// Dynamic Registry for pluggable AI capabilities (Phase 21.4 Evolution)
pub struct BackendRegistry {
    factories: DashMap<String, Arc<dyn BackendFactory>>,
}

lazy_static::lazy_static! {
    pub static ref REGISTRY: BackendRegistry = BackendRegistry {
        factories: DashMap::new(),
    };
}

impl BackendRegistry {
    pub fn register(&self, id: &str, factory: Arc<dyn BackendFactory>) {
        self.factories.insert(id.to_lowercase(), factory);
    }

    pub fn find_factory(
        &self,
        path: &std::path::Path,
        cap: BackendCapability,
    ) -> Option<Arc<dyn BackendFactory>> {
        self.find_factory_entry(path, cap)
            .map(|(_, factory)| factory)
    }

    pub fn find_factory_entry(
        &self,
        path: &std::path::Path,
        cap: BackendCapability,
    ) -> Option<(String, Arc<dyn BackendFactory>)> {
        // 1. Try exact matches based on path extension or prefix (high priority)
        for entry in self.factories.iter() {
            if entry.capability() == cap && entry.can_handle(path) {
                return Some((entry.key().clone(), entry.value().clone()));
            }
        }
        None
    }

    pub fn registered_factories(&self) -> Vec<BackendFactoryDescriptor> {
        let mut descriptors: Vec<_> = self
            .factories
            .iter()
            .map(|entry| BackendFactoryDescriptor {
                factory_id: entry.key().clone(),
                source: entry.value().source(),
                capability: entry.value().capability(),
                declared_roles: entry.value().registered_roles(),
            })
            .collect();
        descriptors.sort_by(|a, b| a.factory_id.cmp(&b.factory_id));
        descriptors
    }

    /// Primary entrypoint to initialize all standard factories
    pub fn init_standard(&self) {
        use crate::backend::factory_impls::*;
        // Cloud handlers for all traits
        self.register(
            "cloud_llm",
            Arc::new(CloudBackendFactory::new(BackendCapability::LLM)),
        );
        self.register(
            "cloud_vlm",
            Arc::new(CloudBackendFactory::new(BackendCapability::Vision)),
        );
        self.register(
            "cloud_img",
            Arc::new(CloudBackendFactory::new(BackendCapability::ImageGeneration)),
        );
        self.register("openai_image_bridge", Arc::new(OpenAiImageBridgeFactory));
        self.register(
            "cloud_stt",
            Arc::new(CloudBackendFactory::new(BackendCapability::STT)),
        );
        self.register(
            "cloud_tts",
            Arc::new(CloudBackendFactory::new(BackendCapability::TTS)),
        );
        self.register(
            "cloud_emb",
            Arc::new(CloudBackendFactory::new(BackendCapability::Embedding)),
        );
        self.register(
            "cloud_rerank",
            Arc::new(CloudBackendFactory::new(BackendCapability::Rerank)),
        );
        self.register(
            "cloud_ocr",
            Arc::new(CloudBackendFactory::new(BackendCapability::OCR)),
        );

        self.register("candle", Arc::new(LocalCandleFactory));
        #[cfg(feature = "llama_cpp")]
        self.register("llama_cpp", Arc::new(LocalLlamaCppFactory));
        self.register(
            "onnx_embedding_winml",
            Arc::new(WindowsNativeOnnxEmbeddingFactory),
        );
        self.register("bert_embedding", Arc::new(LocalEmbeddingFactory));
        self.register(
            "onnx_rerank_winml",
            Arc::new(WindowsNativeOnnxRerankFactory),
        );
        self.register("cross_encoder_rerank", Arc::new(LocalRerankFactory));
        self.register("piper", Arc::new(LocalTtsFactory));
        self.register("whisper", Arc::new(LocalSttFactory));
        self.register("tesseract_ocr", Arc::new(LocalTesseractOcrFactory));
        self.register("nlu", Arc::new(LocalNluFactory));
        self.register("fact_check", Arc::new(LocalFactCheckFactory));
    }
}

/// Unified Gateway for all AI capabilities (Phase 21.3)
pub struct InferenceFactory;

impl InferenceFactory {
    fn backend_cache_key(path: &std::path::Path, mmproj_path: Option<&std::path::Path>) -> String {
        fn normalize(p: &std::path::Path) -> String {
            std::fs::canonicalize(p)
                .unwrap_or_else(|_| p.to_path_buf())
                .to_string_lossy()
                .into_owned()
        }

        let model = normalize(path);
        let mmproj = mmproj_path
            .map(normalize)
            .unwrap_or_else(|| "-".to_string());
        format!("model={model}|mmproj={mmproj}")
    }

    pub fn describe_registered_backends() -> Vec<BackendFactoryDescriptor> {
        REGISTRY.registered_factories()
    }

    pub fn describe_binding(
        path: &std::path::Path,
        mmproj_path: Option<&std::path::Path>,
        capability: BackendCapability,
    ) -> Result<BackendBindingDescriptor> {
        let (factory_id, factory) =
            REGISTRY
                .find_factory_entry(path, capability)
                .ok_or_else(|| {
                    InferenceError::NotFound(format!(
                        "No backend factory capable of describing {:?} for {:?}",
                        capability, path
                    ))
                })?;

        let is_cloud = matches!(factory.source(), BackendSource::Cloud);

        Ok(BackendBindingDescriptor {
            factory_id,
            source: factory.source(),
            capability,
            declared_roles: factory.resolved_roles(path, mmproj_path),
            model_path: path.to_string_lossy().to_string(),
            mmproj_path: mmproj_path.map(|p| p.to_string_lossy().to_string()),
            is_local: !is_cloud,
        })
    }

    /// Generic Backend Loader (Defaults to Text/VLM)
    pub async fn create_backend(
        path: &std::path::Path,
        mmproj_path: Option<&std::path::Path>,
    ) -> Result<Arc<dyn ModelBackend>> {
        let cache_key = Self::backend_cache_key(path, mmproj_path);
        if let Some(cached) = MODEL_BACKEND_CACHE.get(&cache_key) {
            tracing::info!("♻️ Reusing shared inference backend for {}", path.display());
            return Ok(cached.value().clone());
        }

        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::LLM) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, mmproj_path).await?;
            if let Some(backend) = backend_any.downcast_ref::<Arc<dyn ModelBackend>>() {
                let backend = backend.clone();
                MODEL_BACKEND_CACHE.insert(cache_key, backend.clone());
                return Ok(backend);
            }
            if let Ok(backend) = backend_any.downcast::<Arc<dyn ModelBackend>>() {
                let backend = *backend;
                MODEL_BACKEND_CACHE.insert(cache_key, backend.clone());
                return Ok(backend);
            }
        }

        // 🧪 Robust Fallback Logic for Legacy/Direct paths
        let path_str = path.to_string_lossy();
        if path_str.starts_with("api:") || path_str.starts_with("http") {
            return Ok(Arc::new(crate::backend::cloud::CloudBackend::from_path(
                &path_str,
            )?));
        }

        Err(InferenceError::NotFound(format!(
            "No backend factory capable of loading: {:?}",
            path
        )))
    }

    /// Vision-Specific Loader
    pub async fn create_vision_backend(
        path: &std::path::Path,
    ) -> Result<Arc<dyn VisionModelBackend>> {
        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::Vision) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, None).await?;
            if let Some(backend) = backend_any.downcast_ref::<Arc<dyn VisionModelBackend>>() {
                return Ok(backend.clone());
            }
        }

        // Cloud-based vision models already handle generic ModelBackend.
        let _ = Self::create_backend(path, None).await?;
        Ok(Arc::new(crate::backend::cloud::CloudBackend::from_path(
            &path.to_string_lossy(),
        )?))
    }

    /// OCR Loader (Universal Base)
    pub async fn create_ocr_backend(path: &std::path::Path) -> Result<Arc<dyn OcrBackend>> {
        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::OCR) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, None).await?;
            if let Some(backend) = backend_any.downcast_ref::<Arc<dyn OcrBackend>>() {
                return Ok(backend.clone());
            }
        }

        let path_str = path.to_string_lossy();
        if path_str == "tesseract" {
            return Ok(Arc::new(
                crate::backend::ocr_tesseract::TesseractBackend::new("eng"),
            ));
        }
        Ok(Arc::new(NullOcrBackend))
    }

    /// Embedding Loader
    pub async fn create_embedding_backend(
        path: &std::path::Path,
    ) -> Result<Arc<dyn EmbeddingBackend>> {
        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::Embedding) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, None).await?;
            if let Some(backend) = backend_any.downcast_ref::<Arc<dyn EmbeddingBackend>>() {
                return Ok(backend.clone());
            }
        }
        Ok(Arc::new(crate::backend::embeddings::NullEmbeddingBackend))
    }

    /// Rerank Loader
    pub async fn create_rerank_backend(path: &std::path::Path) -> Result<Arc<dyn RerankBackend>> {
        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::Rerank) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, None).await?;
            if let Some(backend) = backend_any.downcast_ref::<Arc<dyn RerankBackend>>() {
                return Ok(backend.clone());
            }
        }
        Ok(Arc::new(crate::backend::rerank::NullRerankBackend))
    }

    /// STT (Audio to Text) Loader
    pub async fn create_stt_backend(path: &std::path::Path) -> Result<Arc<dyn SttBackend>> {
        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::STT) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, None).await?;
            if let Some(backend) = backend_any.downcast_ref::<Arc<dyn SttBackend>>() {
                return Ok(backend.clone());
            }
        }
        Ok(Arc::new(NullSttBackend))
    }

    /// TTS (Text to Audio) Loader
    pub async fn create_tts_backend(path: &std::path::Path) -> Result<Arc<dyn TtsBackend>> {
        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::TTS) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, None).await?;
            if let Some(backend) = backend_any.downcast_ref::<Arc<dyn TtsBackend>>() {
                return Ok(backend.clone());
            }
        }
        Ok(Arc::new(NullTtsBackend))
    }

    /// NLU Loader
    pub async fn create_nlu_backend(
        path: &std::path::Path,
    ) -> Result<Arc<dyn benshu_infra::traits::nlu::NluEngine>> {
        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::NLU) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, None).await?;
            if let Some(backend) =
                backend_any.downcast_ref::<Arc<dyn benshu_infra::traits::nlu::NluEngine>>()
            {
                return Ok(backend.clone());
            }
        }
        Ok(Arc::new(crate::backend::nlu::NullNluEngine))
    }

    /// FactCheck Loader
    pub async fn create_fact_checker_backend(
        path: &std::path::Path,
    ) -> Result<Arc<dyn benshu_infra::traits::validation::FactChecker>> {
        use benshu_infra::traits::validation::FactChecker;
        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::FactCheck) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, None).await?;
            if let Some(backend) = backend_any.downcast_ref::<Arc<dyn FactChecker>>() {
                return Ok(backend.clone());
            }
        }
        Ok(Arc::new(crate::backend::validation_candle::NullFactChecker))
    }

    /// Image Generation Loader (Phase 24)
    pub async fn create_image_gen_backend(
        path: &std::path::Path,
    ) -> Result<Arc<dyn ImageGenBackend>> {
        if let Some(factory) = REGISTRY.find_factory(path, BackendCapability::ImageGeneration) {
            let (vram, ram) = factory.estimate_usage(path);
            let _guard = RESOURCES.check_and_reserve(vram, ram)?;

            let backend_any = factory.create(path, None).await?;
            if let Some(backend) = backend_any.downcast_ref::<Arc<dyn ImageGenBackend>>() {
                return Ok(backend.clone());
            }
        }
        Ok(Arc::new(NullImageGenBackend))
    }

    /// Private helper for device detection (Phase 21.10: Explicit Diagnostics)
    pub fn detect_best_device() -> candle_core::Device {
        let hw = crate::hardware::HardwareStatus::detect();
        match hw.acceleration_profile() {
            crate::hardware::AccelerationProfile::CudaPreferred => {
                tracing::info!(
                    "🚀 [Hardware] NVIDIA capability profile detected ({:?}, {:?}). Initializing CUDA.",
                    hw.gpu_vendor,
                    hw.gpu_probe_confidence
                );
                candle_core::Device::new_cuda(0).unwrap_or_else(|e| {
                    tracing::warn!(
                        "⚠️ [Hardware] CUDA initialization failed (Driver/SM Mismatch?): {}. Falling back to CPU.",
                        e
                    );
                    candle_core::Device::Cpu
                })
            }
            crate::hardware::AccelerationProfile::MetalPreferred => {
                tracing::info!(
                    "🍎 [Hardware] Apple Metal profile detected. Initializing accelerator."
                );
                candle_core::Device::new_metal(0).unwrap_or_else(|e| {
                    tracing::warn!(
                        "⚠️ [Hardware] Metal initialization failed: {}. Falling back to CPU.",
                        e
                    );
                    candle_core::Device::Cpu
                })
            }
            crate::hardware::AccelerationProfile::VulkanPreferred => {
                tracing::info!(
                    "🎮 [Hardware] {:?} GPU detected with Vulkan capability. Candle has no Vulkan runtime, so CPU remains the candle execution device.",
                    hw.gpu_vendor
                );
                candle_core::Device::Cpu
            }
            crate::hardware::AccelerationProfile::CpuOnly => {
                tracing::debug!("💻 [Hardware] No specialized accelerators found. Using CPU.");
                candle_core::Device::Cpu
            }
        }
    }
}

/// 🚫 Fallback STT
pub struct NullSttBackend;
#[async_trait]
impl AudioModelBackend for NullSttBackend {
    fn model_info(&self) -> String {
        "Null STT".into()
    }
    fn estimated_memory_usage(&self) -> u64 {
        0
    }
}
#[async_trait]
impl SttBackend for NullSttBackend {
    async fn transcribe(&self, _pcm: &[f32]) -> Result<String> {
        tracing::warn!("⚠️ [Fallback] NullSttBackend invoked. Returning empty transcript.");
        Ok("".into())
    }
}

/// 🚫 Fallback TTS
pub struct NullTtsBackend;
#[async_trait]
impl AudioModelBackend for NullTtsBackend {
    fn model_info(&self) -> String {
        "Null TTS".into()
    }
    fn estimated_memory_usage(&self) -> u64 {
        0
    }
}
#[async_trait]
impl TtsBackend for NullTtsBackend {
    async fn synthesize(&self, _text: &str) -> Result<Vec<u8>> {
        tracing::warn!("⚠️ [Fallback] NullTtsBackend invoked. Returning empty audio.");
        Ok(Vec::new())
    }
}

/// 🚫 Fallback Image Gen
pub struct NullImageGenBackend;
#[async_trait]
impl ImageGenBackend for NullImageGenBackend {
    fn model_info(&self) -> String {
        "Null Image Generator".into()
    }
    async fn generate_image(
        &self,
        _prompt: &str,
        _size: (u32, u32),
        _config: DiffusionConfig,
    ) -> Result<image::DynamicImage> {
        Err(InferenceError::Internal(
            "No image generation backend available".into(),
        ))
    }

    async fn generate_image_img2img(
        &self,
        _prompt: &str,
        _initial_image: &image::DynamicImage,
        _config: DiffusionConfig,
    ) -> Result<image::DynamicImage> {
        Err(InferenceError::Internal(
            "No image-to-image backend available".into(),
        ))
    }

    async fn generate_image_inpainting(
        &self,
        _prompt: &str,
        _initial_image: &image::DynamicImage,
        _mask: &image::DynamicImage,
        _config: DiffusionConfig,
    ) -> Result<image::DynamicImage> {
        Err(InferenceError::Internal(
            "No inpainting backend available".into(),
        ))
    }

    fn estimated_memory_usage(&self) -> u64 {
        0
    }
    fn device_info(&self) -> DeviceType {
        DeviceType::Cpu
    }
}

/// 🚫 Fallback OCR
pub struct NullOcrBackend;
#[async_trait]
impl OcrBackend for NullOcrBackend {
    fn model_info(&self) -> String {
        "Null OCR".into()
    }
    async fn recognize(&self, _img: &image::DynamicImage) -> Result<String> {
        tracing::warn!("⚠️ [Fallback] NullOcrBackend invoked. Returning empty text.");
        Ok("".into())
    }

    fn estimated_memory_usage(&self) -> u64 {
        0
    }
    fn device_info(&self) -> DeviceType {
        DeviceType::Cpu
    }
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_fixture_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "benshu-inference-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn registered_backends_expose_declared_roles_and_sources() {
        REGISTRY.init_standard();
        let descriptors = InferenceFactory::describe_registered_backends();

        let cloud_llm = descriptors
            .iter()
            .find(|item| item.factory_id == "cloud_llm")
            .expect("cloud llm descriptor");
        assert_eq!(cloud_llm.source, BackendSource::Cloud);
        assert!(cloud_llm.declared_roles.contains(&ModelRole::Llm));
        assert!(cloud_llm.declared_roles.contains(&ModelRole::Slm));

        let candle = descriptors
            .iter()
            .find(|item| item.factory_id == "candle")
            .expect("candle descriptor");
        assert_eq!(candle.source, BackendSource::Local);
        assert!(candle.declared_roles.contains(&ModelRole::Llm));
        assert!(candle.declared_roles.contains(&ModelRole::Slm));
        assert!(candle.declared_roles.contains(&ModelRole::Vlm));

        let embedding = descriptors
            .iter()
            .find(|item| item.factory_id == "bert_embedding")
            .expect("embedding descriptor");
        assert_eq!(embedding.declared_roles, vec![ModelRole::Embedding]);

        let rerank = descriptors
            .iter()
            .find(|item| item.factory_id == "cross_encoder_rerank")
            .expect("rerank descriptor");
        assert_eq!(rerank.declared_roles, vec![ModelRole::Rerank]);

        let ocr = descriptors
            .iter()
            .find(|item| item.factory_id == "tesseract_ocr")
            .expect("ocr descriptor");
        assert_eq!(ocr.declared_roles, vec![ModelRole::Ocr]);
    }

    #[test]
    fn describe_binding_reports_local_text_and_vlm_roles() {
        REGISTRY.init_standard();

        let path = temp_fixture_path("candle-text");
        fs::create_dir_all(&path).expect("create temp dir");
        fs::write(path.join("config.json"), "{}").expect("write config");

        let text_binding = InferenceFactory::describe_binding(&path, None, BackendCapability::LLM)
            .expect("describe text binding");
        assert_eq!(text_binding.factory_id, "candle");
        assert!(text_binding.is_local);
        assert_eq!(text_binding.source, BackendSource::Local);
        assert_eq!(
            text_binding.declared_roles,
            vec![ModelRole::Llm, ModelRole::Slm]
        );

        fs::write(path.join("vision_encoder.safetensors"), "").expect("write vision marker");
        let vlm_binding = InferenceFactory::describe_binding(&path, None, BackendCapability::LLM)
            .expect("describe vlm binding");
        assert!(vlm_binding.declared_roles.contains(&ModelRole::Vlm));

        fs::remove_dir_all(&path).expect("cleanup temp dir");
    }

    #[test]
    fn describe_binding_reports_cloud_llm_and_slm_roles() {
        REGISTRY.init_standard();

        let path = PathBuf::from("api:openai/test-small-model");
        let binding = InferenceFactory::describe_binding(&path, None, BackendCapability::LLM)
            .expect("describe cloud binding");

        assert_eq!(binding.factory_id, "cloud_llm");
        assert_eq!(binding.source, BackendSource::Cloud);
        assert!(!binding.is_local);
        assert!(binding.declared_roles.contains(&ModelRole::Llm));
        assert!(binding.declared_roles.contains(&ModelRole::Slm));
    }

    #[test]
    fn describe_binding_reports_embedding_rerank_and_ocr_roles() {
        REGISTRY.init_standard();

        let path = temp_fixture_path("encoder");
        fs::create_dir_all(&path).expect("create temp dir");
        fs::write(path.join("config.json"), "{}").expect("write config");
        fs::write(path.join("tokenizer.json"), "{}").expect("write tokenizer");
        fs::write(path.join("model.safetensors"), "").expect("write model marker");

        let embedding =
            InferenceFactory::describe_binding(&path, None, BackendCapability::Embedding)
                .expect("describe embedding");
        assert_eq!(embedding.factory_id, "bert_embedding");
        assert_eq!(embedding.declared_roles, vec![ModelRole::Embedding]);

        let rerank = InferenceFactory::describe_binding(&path, None, BackendCapability::Rerank)
            .expect("describe rerank");
        assert_eq!(rerank.factory_id, "cross_encoder_rerank");
        assert_eq!(rerank.declared_roles, vec![ModelRole::Rerank]);

        let ocr = InferenceFactory::describe_binding(
            &PathBuf::from("tesseract"),
            None,
            BackendCapability::OCR,
        )
        .expect("describe ocr");
        assert_eq!(ocr.factory_id, "tesseract_ocr");
        assert_eq!(ocr.declared_roles, vec![ModelRole::Ocr]);

        fs::remove_dir_all(&path).expect("cleanup temp dir");
    }

    #[test]
    fn describe_binding_reports_onnx_embedding_and_rerank_factories() {
        REGISTRY.init_standard();

        let path = temp_fixture_path("onnx-encoder");
        fs::create_dir_all(&path).expect("create temp dir");
        fs::write(path.join("model.onnx"), "").expect("write onnx model");

        let embedding =
            InferenceFactory::describe_binding(&path, None, BackendCapability::Embedding)
                .expect("describe onnx embedding");
        assert_eq!(embedding.factory_id, "onnx_embedding_winml");
        assert_eq!(embedding.declared_roles, vec![ModelRole::Embedding]);

        let rerank = InferenceFactory::describe_binding(&path, None, BackendCapability::Rerank)
            .expect("describe onnx rerank");
        assert_eq!(rerank.factory_id, "onnx_rerank_winml");
        assert_eq!(rerank.declared_roles, vec![ModelRole::Rerank]);

        fs::remove_dir_all(&path).expect("cleanup temp dir");
    }

    #[test]
    fn describe_binding_reports_nlu_and_fact_check_roles() {
        REGISTRY.init_standard();

        let nlu_path = temp_fixture_path("nlu-optimal");
        fs::create_dir_all(&nlu_path).expect("create temp dir");
        fs::write(nlu_path.join("config.json"), "{}").expect("write config");
        fs::write(nlu_path.join("model.safetensors"), "").expect("write model marker");

        let nlu = InferenceFactory::describe_binding(&nlu_path, None, BackendCapability::NLU)
            .expect("describe nlu");
        assert_eq!(nlu.factory_id, "nlu");
        assert_eq!(nlu.declared_roles, vec![ModelRole::Nlu]);

        let fact_path = temp_fixture_path("fact-check-local");
        fs::create_dir_all(&fact_path).expect("create temp dir");
        fs::write(fact_path.join("config.json"), "{}").expect("write config");
        fs::write(fact_path.join("model.safetensors"), "").expect("write model marker");

        let fact =
            InferenceFactory::describe_binding(&fact_path, None, BackendCapability::FactCheck)
                .expect("describe fact check");
        assert_eq!(fact.factory_id, "fact_check");
        assert_eq!(fact.declared_roles, vec![ModelRole::FactCheck]);

        fs::remove_dir_all(&nlu_path).expect("cleanup nlu");
        fs::remove_dir_all(&fact_path).expect("cleanup fact");
    }

    #[tokio::test]
    async fn creating_onnx_embedding_requires_ready_windows_native_runtime() {
        REGISTRY.init_standard();

        let path = temp_fixture_path("onnx-embedding-runtime-gate");
        fs::create_dir_all(&path).expect("create temp dir");
        fs::write(path.join("model.onnx"), "").expect("write onnx model");

        let err = match InferenceFactory::create_embedding_backend(&path).await {
            Ok(_) => panic!("onnx embedding should not activate on a non-ready host"),
            Err(err) => err,
        };
        let text = err.to_string();
        assert!(
            text.contains("Cannot activate Windows-native ONNX embedding runtime"),
            "unexpected error: {text}"
        );

        fs::remove_dir_all(&path).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn creating_onnx_rerank_requires_ready_windows_native_runtime() {
        REGISTRY.init_standard();

        let path = temp_fixture_path("onnx-rerank-runtime-gate");
        fs::create_dir_all(&path).expect("create temp dir");
        fs::write(path.join("model.onnx"), "").expect("write onnx model");

        let err = match InferenceFactory::create_rerank_backend(&path).await {
            Ok(_) => panic!("onnx rerank should not activate on a non-ready host"),
            Err(err) => err,
        };
        let text = err.to_string();
        assert!(
            text.contains("Cannot activate Windows-native ONNX rerank runtime"),
            "unexpected error: {text}"
        );

        fs::remove_dir_all(&path).expect("cleanup temp dir");
    }
}
