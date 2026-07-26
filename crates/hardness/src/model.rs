use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSnapshot {
    pub text: String,
    #[serde(default)]
    pub media: Vec<MediaKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityScore {
    pub score: f32,
    pub reason: String,
    pub predicted_output_tokens: usize,
    pub is_parallelizable: bool,
    pub level: usize,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
