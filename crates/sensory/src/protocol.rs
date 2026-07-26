use image::DynamicImage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb8,
    Nv12,
    Bgr8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameMetadata {
    pub source: String, // e.g., "camera", "desktop", "remote"
    pub sequence_id: u64,
    pub timestamp: u64, // Unix timestamp in ms
    pub extras: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub buffer: Arc<Vec<u8>>, // Use Arc for zero-copy sharing
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub metadata: FrameMetadata,
}

/// Standard inputs for any sensory task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SensoryInput {
    #[serde(skip)]
    Image(DynamicImage),
    #[serde(skip)]
    SharedImage(Arc<DynamicImage>),
    Audio(Vec<u8>),
    #[serde(skip)]
    VideoFrame(VideoFrame),
}

/// Fallback policy for sensory failures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FallbackPolicy {
    Error,
    DegradeToText, // Close vision/audio and continue with text
    SwitchToCloud, // Fallback to configured provider API
}

/// Unified Output for the Sensory System
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SensoryOutput {
    Text(String),
    Tags(Vec<DetectedElement>),
    Features(Vec<f32>),
    Audio(Vec<u8>), // For TTS results
    Coordinates {
        x: f32,
        y: f32,
        label: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedElement {
    pub label: String,
    pub box_2d: [u32; 4], // [x, y, w, h]
    pub confidence: f32,
    pub metadata: Option<serde_json::Value>,
}

/// A high-level request to the Sensory Hub
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SensoryRequest {
    Vision {
        input: SensoryInput,
        plugin_hint: Option<String>,
        prompt: Option<String>,
    },
    Audio {
        input: SensoryInput,
        plugin_hint: Option<String>,
    },
    Speak {
        text: String,
        plugin_hint: Option<String>,
    },
}
