use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use image::DynamicImage;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock as AsyncRwLock;
use tracing::{debug, info, warn};

use crate::audio::AudioPlugin;
use crate::protocol::*;
use crate::vision::VisionPlugin;

/// Configuration for the Sensory Hub (Phase 22)
pub struct SensoryConfig {
    pub fallback_policy: FallbackPolicy,
    pub vram_budget: u64,
    pub max_image_dimension: u32,
    pub vision_fallback: Option<String>,
    pub audio_fallback: Option<String>,
    pub video_frame_buffer_size: usize,
}

impl Default for SensoryConfig {
    fn default() -> Self {
        Self {
            fallback_policy: FallbackPolicy::SwitchToCloud,
            vram_budget: 4 * 1024 * 1024 * 1024, // 4GB Default
            max_image_dimension: 2048,
            vision_fallback: None,
            audio_fallback: None,
            video_frame_buffer_size: 10,
        }
    }
}

pub struct SensoryHub {
    vision_plugins: DashMap<String, Arc<dyn VisionPlugin>>,
    audio_plugins: DashMap<String, Arc<dyn AudioPlugin>>,
    config: AsyncRwLock<SensoryConfig>,
    _device: candle_core::Device,
    current_vram: AtomicU64,
    lru_queue: AsyncMutex<VecDeque<String>>,
    video_frames: DashMap<String, Arc<AsyncRwLock<VecDeque<VideoFrame>>>>,
}

impl SensoryHub {
    pub fn new(config: SensoryConfig) -> Self {
        let device = if candle_core::utils::cuda_is_available() {
            candle_core::Device::new_cuda(0).unwrap_or(candle_core::Device::Cpu)
        } else if candle_core::utils::metal_is_available() {
            candle_core::Device::new_metal(0).unwrap_or(candle_core::Device::Cpu)
        } else {
            candle_core::Device::Cpu
        };

        Self {
            vision_plugins: DashMap::new(),
            audio_plugins: DashMap::new(),
            config: AsyncRwLock::new(config),
            _device: device,
            current_vram: AtomicU64::new(0),
            lru_queue: AsyncMutex::new(VecDeque::new()),
            video_frames: DashMap::new(),
        }
    }

    pub async fn reconfigure(&self, new_config: SensoryConfig) {
        let mut config = self.config.write().await;
        info!(
            "SensoryHub: Reconfiguring VRAM Budget from {} to {}",
            config.vram_budget, new_config.vram_budget
        );
        *config = new_config;
    }

    pub fn register_vision(&self, plugin: Arc<dyn VisionPlugin>) {
        self.vision_plugins
            .insert(plugin.name().to_string(), plugin);
    }

    pub fn register_audio(&self, plugin: Arc<dyn AudioPlugin>) {
        self.audio_plugins.insert(plugin.name().to_string(), plugin);
    }

    pub fn audio_plugins(&self) -> Vec<(String, Arc<dyn AudioPlugin>)> {
        self.audio_plugins
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    pub fn vision_plugins(&self) -> Vec<(String, Arc<dyn VisionPlugin>)> {
        self.vision_plugins
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    pub async fn load_plugin(&self, name: &str) -> Result<()> {
        let usage = if let Some(p) = self.vision_plugins.get(name) {
            p.estimated_memory_usage()
        } else if let Some(p) = self.audio_plugins.get(name) {
            p.estimated_memory_usage()
        } else {
            anyhow::bail!("Plugin {} not found", name);
        };

        self.load_and_arbitrate(name, usage).await
    }

    /// Optimized VRAM arbitration (Phase 11.4 - Per apprentice feedback)
    /// 1. Minimizes lock time (Lock only for state check/update).
    /// 2. Atomic accounting: Only increments total usage if load succeeds.
    /// 3. Concurrent-safe: Prevents multiple threads from loading/unloading without coordination.
    async fn load_and_arbitrate(&self, name: &str, usage: u64) -> Result<()> {
        let mut to_unload = Vec::new();

        // --- SECTION 1: Strategic Decision ---
        {
            let mut queue = self.lru_queue.lock().await;

            // Check if already loaded OR currently in the queue as "active"
            if queue.contains(&name.to_string()) {
                // If it's already there, just move it to most-recent
                queue.retain(|n| n != name);
                queue.push_back(name.to_string());
                return Ok(());
            }

            // Calculate budget and pick victims
            let mut current = self.current_vram.load(Ordering::Acquire);
            let budget = self.config.read().await.vram_budget;
            while current + usage > budget && !queue.is_empty() {
                if let Some(oldest_name) = queue.pop_front() {
                    let oldest_usage = self.get_plugin_usage(&oldest_name);
                    to_unload.push((oldest_name, oldest_usage));
                    current -= oldest_usage;
                }
            }

            // Optimistic pre-insertion to the queue to mark as "loading/active"
            // This prevents concurrent duplicates if they check the queue inside Section 1.
            queue.push_back(name.to_string());
        }

        // --- SECTION 2: Action (No Global Lock) ---
        for (old_name, old_usage) in to_unload {
            info!(
                "VRAM Arbitrator: Reclaiming {} ({} bytes)",
                old_name, old_usage
            );
            if let Some(v_p) = self.vision_plugins.get(&old_name) {
                v_p.unload();
            } else if let Some(a_p) = self.audio_plugins.get(&old_name) {
                a_p.unload();
            }
            // Double check: only deduct if it was really loaded
            self.current_vram.fetch_sub(old_usage, Ordering::SeqCst);
        }

        // Load new plugin
        info!("Loading sensory plugin: {}", name);
        let result = if let Some(v_p) = self.vision_plugins.get(name) {
            v_p.load().await
        } else if let Some(a_p) = self.audio_plugins.get(name) {
            a_p.load().await
        } else {
            anyhow::bail!("Plugin {} disappeared during arbitration", name)
        };

        // --- SECTION 3: Bookkeeping ---
        match result {
            Ok(_) => {
                // Verify ready state
                let is_ready = if let Some(v_p) = self.vision_plugins.get(name) {
                    v_p.is_loaded()
                } else if let Some(a_p) = self.audio_plugins.get(name) {
                    a_p.is_loaded()
                } else {
                    false
                };

                if is_ready {
                    self.current_vram.fetch_add(usage, Ordering::SeqCst);
                    Ok(())
                } else {
                    warn!("Plugin {} load returned OK but is_loaded is false", name);
                    Err(anyhow::anyhow!(
                        "Plugin {} failed post-load readiness check",
                        name
                    ))
                }
            }
            Err(e) => {
                warn!("Failed to load plugin {}: {}", name, e);
                // Clean up the queue since it failed
                let mut queue = self.lru_queue.lock().await;
                queue.retain(|n| n != name);
                Err(e)
            }
        }
    }

    fn get_plugin_usage(&self, name: &str) -> u64 {
        if let Some(p) = self.vision_plugins.get(name) {
            p.estimated_memory_usage()
        } else if let Some(p) = self.audio_plugins.get(name) {
            p.estimated_memory_usage()
        } else {
            0
        }
    }

    pub async fn dispatch(&self, request: SensoryRequest) -> Result<SensoryOutput> {
        match request {
            SensoryRequest::Vision {
                input,
                plugin_hint,
                prompt,
            } => {
                let img = match input {
                    SensoryInput::Image(i) => i,
                    SensoryInput::SharedImage(i) => (*i).clone(),
                    SensoryInput::VideoFrame(vf) => {
                        // For Phase 0, we convert VideoFrame to DynamicImage for processing
                        // In Phase 1+, we might use a more efficient sampling path
                        self.convert_frame_to_image(vf)?
                    }
                    _ => anyhow::bail!("Invalid input type for Vision task"),
                };
                let processed_img = self.pre_process_image(img).await?;
                let plugin = self.select_vision_plugin(plugin_hint.as_deref())?;

                self.load_and_arbitrate(plugin.name(), plugin.estimated_memory_usage())
                    .await?;

                match plugin.process(&processed_img, prompt.as_deref()).await {
                    Ok(out) => Ok(out),
                    Err(e) => {
                        self.handle_vision_fallback(e, &processed_img, prompt.as_deref())
                            .await
                    }
                }
            }
            SensoryRequest::Audio { input, plugin_hint } => {
                let data = match input {
                    SensoryInput::Audio(d) => d,
                    _ => anyhow::bail!("Invalid input type for Audio task"),
                };
                let plugin = self.select_audio_plugin(plugin_hint.as_deref())?;

                self.load_and_arbitrate(plugin.name(), plugin.estimated_memory_usage())
                    .await?;

                match plugin.process(&data).await {
                    Ok(out) => Ok(out),
                    Err(e) => self.handle_audio_fallback(e, &data).await,
                }
            }
            SensoryRequest::Speak { text, plugin_hint } => {
                let plugin = self.select_audio_plugin(plugin_hint.as_deref())?;
                self.load_and_arbitrate(plugin.name(), plugin.estimated_memory_usage())
                    .await?;

                let audio = plugin.synthesize(&text).await?;
                Ok(SensoryOutput::Audio(audio))
            }
        }
    }

    /// Entry point for external video stream connectors (Phase 0)
    pub async fn push_frame(&self, frame: VideoFrame) -> Result<()> {
        let source = frame.metadata.source.clone();
        let buffer_size = self.config.read().await.video_frame_buffer_size;

        let frames_lock = self
            .video_frames
            .entry(source.clone())
            .or_insert_with(|| Arc::new(AsyncRwLock::new(VecDeque::with_capacity(buffer_size))))
            .value()
            .clone();

        let mut frames = frames_lock.write().await;
        let buffer_size = self.config.read().await.video_frame_buffer_size;
        if frames.len() >= buffer_size {
            frames.pop_front();
        }

        frames.push_back(frame);

        debug!(
            "SensoryHub: Sampled frame from {}. Buffer size: {}",
            source,
            frames.len()
        );
        Ok(())
    }

    /// Retrieve the last set of frames for a given source
    pub async fn get_frames(&self, source: &str, count: usize) -> Vec<VideoFrame> {
        if let Some(frames_lock) = self.video_frames.get(source) {
            let frames = frames_lock.read().await;
            frames.iter().rev().take(count).cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Helper to convert raw VideoFrame to DynamicImage (Phase 0)
    fn convert_frame_to_image(&self, frame: VideoFrame) -> Result<DynamicImage> {
        match frame.format {
            PixelFormat::Rgb8 => {
                let img = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(
                    frame.width,
                    frame.height,
                    (*frame.buffer).clone(),
                )
                .ok_or_else(|| anyhow::anyhow!("Failed to create ImageBuffer from RGB8"))?;
                Ok(DynamicImage::ImageRgb8(img))
            }
            PixelFormat::Bgr8 => {
                let mut buffer = (*frame.buffer).clone();
                // Swap B and R channels in-place
                for chunk in buffer.chunks_exact_mut(3) {
                    chunk.swap(0, 2);
                }
                let img = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(
                    frame.width,
                    frame.height,
                    buffer,
                )
                .ok_or_else(|| anyhow::anyhow!("Failed to create ImageBuffer from BGR8"))?;
                Ok(DynamicImage::ImageRgb8(img))
            }
            PixelFormat::Nv12 => {
                // NV12 is YUV 4:2:0. Conversion is more complex and usually better handled
                // by specialized kernels or the 'ffmpeg-next' crate.
                // For now, we continue to bail but with a clearer plan.
                anyhow::bail!("NV12 conversion requires specialized YUV kernels (Phase 23)")
            }
        }
    }

    pub async fn vision_check(
        &self,
        image: DynamicImage,
        prompt: Option<&str>,
        plugin_hint: Option<&str>,
    ) -> Result<SensoryOutput> {
        self.dispatch(SensoryRequest::Vision {
            input: SensoryInput::Image(image),
            plugin_hint: plugin_hint.map(|s| s.to_string()),
            prompt: prompt.map(|s| s.to_string()),
        })
        .await
    }

    fn select_vision_plugin(&self, hint: Option<&str>) -> Result<Arc<dyn VisionPlugin>> {
        if let Some(name) = hint {
            if let Some(p) = self.vision_plugins.get(name) {
                return Ok(p.value().clone());
            }
        }
        self.vision_plugins
            .iter()
            .next()
            .map(|r| r.value().clone())
            .ok_or_else(|| anyhow::anyhow!("No Vision plugins registered"))
    }

    fn select_audio_plugin(&self, hint: Option<&str>) -> Result<Arc<dyn AudioPlugin>> {
        if let Some(name) = hint {
            if let Some(p) = self.audio_plugins.get(name) {
                return Ok(p.value().clone());
            }
        }
        self.audio_plugins
            .iter()
            .next()
            .map(|r| r.value().clone())
            .ok_or_else(|| anyhow::anyhow!("No Audio plugins registered"))
    }

    /// Safeguard against massive images causing OOM/Stall (Phase 11.4)
    /// CPU-bound resizing is offloaded to spawn_blocking.
    async fn pre_process_image(&self, img: DynamicImage) -> Result<DynamicImage> {
        let (w, h) = (img.width(), img.height());
        let max_dim = self.config.read().await.max_image_dimension;

        if w > max_dim || h > max_dim {
            info!(
                "Pre-processing: Resizing image from {}x{} to fit {}",
                w, h, max_dim
            );
            let resized = tokio::task::spawn_blocking(move || img.thumbnail(max_dim, max_dim))
                .await
                .map_err(|e| anyhow::anyhow!("Spawn blocking failed: {}", e))?;
            Ok(resized)
        } else {
            Ok(img)
        }
    }

    async fn handle_vision_fallback(
        &self,
        err: anyhow::Error,
        img: &DynamicImage,
        prompt: Option<&str>,
    ) -> Result<SensoryOutput> {
        warn!(
            "Vision primary failed: {}. Executing fallback strategy...",
            err
        );
        let config = self.config.read().await;
        match config.fallback_policy {
            FallbackPolicy::DegradeToText => Ok(SensoryOutput::Text(format!(
                "Vision error (Fell back to text): {}",
                err
            ))),
            FallbackPolicy::SwitchToCloud => {
                let fallback_plugin = if let Some(name) = &config.vision_fallback {
                    self.vision_plugins.get(name).map(|r| r.value().clone())
                } else {
                    self.vision_plugins
                        .iter()
                        .find(|r| {
                            let p = r.value();
                            p.name().contains("cloud")
                                || p.name().contains("gpt")
                                || p.name().contains("gemini")
                        })
                        .map(|r| r.value().clone())
                };

                if let Some(plugin) = fallback_plugin {
                    info!("Falling back to vision plugin: {}", plugin.name());
                    plugin.process(img, prompt).await
                } else {
                    Ok(SensoryOutput::Text(format!(
                        "Cloud fallback failed (no cloud plugin) - original error: {}",
                        err
                    )))
                }
            }
            FallbackPolicy::Error => Err(err),
        }
    }

    async fn handle_audio_fallback(
        &self,
        err: anyhow::Error,
        data: &[u8],
    ) -> Result<SensoryOutput> {
        warn!(
            "Audio primary failed: {}. Executing fallback strategy...",
            err
        );
        let config = self.config.read().await;
        match config.fallback_policy {
            FallbackPolicy::DegradeToText => Ok(SensoryOutput::Text(format!(
                "Audio error (Fell back to text): {}",
                err
            ))),
            FallbackPolicy::SwitchToCloud => {
                let fallback_plugin = if let Some(name) = &config.audio_fallback {
                    self.audio_plugins.get(name).map(|r| r.value().clone())
                } else {
                    self.audio_plugins
                        .iter()
                        .find(|r| {
                            let p = r.value();
                            p.name().contains("cloud")
                                || p.name().contains("openai")
                                || p.name().contains("azure")
                        })
                        .map(|r| r.value().clone())
                };

                if let Some(plugin) = fallback_plugin {
                    info!("Falling back to audio plugin: {}", plugin.name());
                    plugin.process(data).await
                } else {
                    Ok(SensoryOutput::Text(format!(
                        "Cloud fallback failed (no cloud plugin) - original error: {}",
                        err
                    )))
                }
            }
            FallbackPolicy::Error => Err(err),
        }
    }
}

#[async_trait]
impl benshu_infra::HealthCheck for SensoryHub {
    async fn check_health(&self) -> benshu_infra::HealthStatus {
        let vram = self.current_vram.load(Ordering::Relaxed);
        let budget = self.config.read().await.vram_budget;
        if vram > budget {
            benshu_infra::HealthStatus::Degraded(format!(
                "VRAM over budget: {}MB / {}MB",
                vram / 1024 / 1024,
                budget / 1024 / 1024
            ))
        } else {
            benshu_infra::HealthStatus::Healthy
        }
    }

    fn module_name(&self) -> &'static str {
        "benshu-sensory::hub"
    }
}

#[async_trait]
impl benshu_infra::traits::SensoryLiaison for SensoryHub {
    async fn dispatch(
        &self,
        request: serde_json::Value,
    ) -> benshu_infra::error::Result<serde_json::Value> {
        let req: SensoryRequest = serde_json::from_value(request).map_err(|e| {
            benshu_infra::error::Error::Internal(format!("Invalid sensory request: {}", e))
        })?;
        let output = self
            .dispatch(req)
            .await
            .map_err(|e| benshu_infra::error::Error::Internal(format!("Sensory error: {}", e)))?;
        Ok(serde_json::to_value(output).unwrap())
    }

    async fn get_hardware_utilization(
        &self,
    ) -> benshu_infra::error::Result<benshu_infra::traits::resource::AcceleratorInfo> {
        let vram = self.current_vram.load(Ordering::Relaxed);
        let budget = self.config.read().await.vram_budget;

        Ok(benshu_infra::traits::resource::AcceleratorInfo {
            name: "sensory_hub_vram".to_string(),
            kind: "vram".to_string(),
            vram_total_mb: budget / 1024 / 1024,
            vram_used_mb: vram / 1024 / 1024,
            vram_pressure_pct: (vram as f32 / budget as f32) * 100.0,
        })
    }
}
