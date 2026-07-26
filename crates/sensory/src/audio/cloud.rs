use crate::audio::AudioPlugin;
use crate::protocol::SensoryOutput;
use anyhow::Result;
use async_trait::async_trait;
use reqwest::{multipart, Client};
use serde::Deserialize;

/// Cloud-based STT implementation (OpenAI compatible)
pub struct CloudSTT {
    api_key: String,
    base_url: String,
    client: Client,
    name: String,
}

impl CloudSTT {
    pub fn new(name: String, api_key: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            client: Client::new(),
            name,
        }
    }
}

#[async_trait]
impl AudioPlugin for CloudSTT {
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self, audio_data: &[u8]) -> Result<SensoryOutput> {
        let file_part = multipart::Part::bytes(audio_data.to_vec())
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let form = multipart::Form::new()
            .part("file", file_part)
            .text("model", "whisper-1");

        let response = self
            .client
            .post(format!("{}/audio/transcriptions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let err = response.text().await?;
            anyhow::bail!("Cloud STT failed: {}", err);
        }

        #[derive(Deserialize)]
        struct Resp {
            text: String,
        }
        let res: Resp = response.json().await?;

        Ok(SensoryOutput::Text(res.text))
    }

    fn estimated_memory_usage(&self) -> u64 {
        0 // Cloud models don't use local VRAM
    }
}

/// Cloud-based TTS implementation (OpenAI compatible)
pub struct CloudTTS {
    api_key: String,
    base_url: String,
    client: Client,
    name: String,
}

impl CloudTTS {
    pub fn new(name: String, api_key: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            client: Client::new(),
            name,
        }
    }
}

#[async_trait]
impl AudioPlugin for CloudTTS {
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self, _audio_data: &[u8]) -> Result<SensoryOutput> {
        anyhow::bail!("CloudTTS does not support processing audio data, only synthesis.")
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        let json_body = serde_json::json!({
            "model": "tts-1",
            "input": text,
            "voice": "alloy"
        });

        let response = self
            .client
            .post(format!("{}/audio/speech", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let err = response.text().await?;
            anyhow::bail!("Cloud TTS failed: {}", err);
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    fn estimated_memory_usage(&self) -> u64 {
        0
    }
}
