use crate::audio::AudioPlugin;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

/// A plugin that handles streaming TTS synthesis
pub struct StreamingTTS {
    // inner: Arc<dyn AudioPlugin>,
}

impl StreamingTTS {
    pub fn new(_inner: Arc<dyn AudioPlugin>) -> Self {
        Self {}
    }
}

// (Actually, the existing Piper plugin could be made streaming-aware)
