//! Voice interaction tools (STT and TTS)
//!
//! Provides tools for Speech-to-Text (Transcribe) and Text-to-Speech (Speak).
//! Currently supports OpenAI API compatible endpoints.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt; // For write_all

use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};
use benshu_state::{ArtifactLifecycle, ArtifactManager};

use super::{register_tool_output_artifact, ToolArtifactRegistration, ToolCleanup};

const OPENAI_API_BASE: &str = "https://api.openai.com/v1";
const MAX_AUDIO_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Tool for transcribing audio to text (STT)
pub struct TranscribeTool {
    _api_key: String,
    _base_url: String,
    sensory: std::sync::Arc<benshu_sensory::SensoryHub>,
}

impl TranscribeTool {
    /// Create a new TranscribeTool
    pub fn new(
        api_key: impl Into<String>,
        base_url: Option<String>,
        sensory: std::sync::Arc<benshu_sensory::SensoryHub>,
    ) -> Self {
        Self {
            _api_key: api_key.into(),
            _base_url: base_url.unwrap_or_else(|| OPENAI_API_BASE.to_string()),
            sensory,
        }
    }
}

#[derive(Deserialize)]
struct TranscribeArgs {
    #[serde(alias = "path", alias = "file", alias = "audio_path")]
    file_path: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
}

#[async_trait]
impl Tool for TranscribeTool {
    fn name(&self) -> String {
        "transcribe_audio".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Transcribe audio file to text using Whisper model.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Absolute path to the audio file to transcribe"
                    },
                    "language": {
                        "type": "string",
                        "description": "Optional ISO-639-1 language code (e.g. 'en', 'zh')"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Optional prompt to guide the model's style or terminology"
                    }
                },
                "required": ["file_path"]
            }),
            parameters_ts: Some("interface TranscribeArgs { \n  file_path: string; // Absolute path to audio file\n  language?: string; // e.g. 'en', 'zh'\n  prompt?: string; // Context or spelling guide\n}".to_string()),
            is_binary: false,
            is_verified: false,
            usage_guidelines: Some("Use this to convert audio files (mp3, wav, etc.) to text.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: TranscribeArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: format!("Invalid arguments: {}", e),
            })?;

        let path = Path::new(&args.file_path);
        if !path.exists() {
            return Err(anyhow::anyhow!("Audio file not found: {}", args.file_path));
        }
        let metadata = tokio::fs::metadata(path).await?;
        if metadata.len() > MAX_AUDIO_FILE_BYTES {
            anyhow::bail!(
                "audio file is larger than the 100MB single-file safety limit: {} bytes",
                metadata.len()
            );
        }

        let file_content = tokio::fs::read(path).await?;

        // Use the injected Sensory Hub for unified transcription
        use benshu_sensory::{SensoryInput, SensoryOutput, SensoryRequest};

        let req = SensoryRequest::Audio {
            input: SensoryInput::Audio(file_content),
            plugin_hint: None,
        };

        match self.sensory.dispatch(req).await? {
            SensoryOutput::Text(t) => Ok(t),
            _ => Err(anyhow::anyhow!("Unexpected sensory response")),
        }
    }
}

/// Tool for Converting text to speech (TTS)
pub struct SpeakTool {
    _api_key: String,
    _base_url: String,
    output_dir: PathBuf,
    sensory: Arc<benshu_sensory::SensoryHub>,
    artifact_manager: Option<Arc<ArtifactManager>>,
    agent_id: String,
}

impl SpeakTool {
    /// Create a new SpeakTool
    pub fn new(
        api_key: impl Into<String>,
        base_url: Option<String>,
        output_dir: PathBuf,
        sensory: Arc<benshu_sensory::SensoryHub>,
    ) -> Self {
        Self {
            _api_key: api_key.into(),
            _base_url: base_url.unwrap_or_else(|| OPENAI_API_BASE.to_string()),
            output_dir,
            sensory,
            artifact_manager: None,
            agent_id: "voice".to_string(),
        }
    }

    /// Set output directory
    pub fn with_output_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.output_dir = path.into();
        self
    }

    pub fn with_artifact_manager(
        mut self,
        manager: Arc<ArtifactManager>,
        agent_id: impl Into<String>,
    ) -> Self {
        self.artifact_manager = Some(manager);
        self.agent_id = agent_id.into();
        self
    }
}

fn default_voice() -> String {
    "alloy".to_string()
}

fn default_model() -> String {
    "tts-1".to_string()
}

#[derive(Deserialize)]
struct SpeakArgs {
    text: String,
    #[serde(default = "default_voice")]
    voice: String,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default)]
    speed: Option<f32>,
    #[serde(default)]
    output_filename: Option<String>,
}

fn speak_cleanup() -> ToolCleanup {
    ToolCleanup::inactive()
}

async fn register_speech_output_artifact(
    artifact_manager: Option<&ArtifactManager>,
    agent_id: &str,
    output_path: &Path,
    voice: &str,
    model: &str,
    user_supplied_output: bool,
) -> anyhow::Result<Option<serde_json::Value>> {
    let Some(manager) = artifact_manager else {
        return Ok(None);
    };

    let mut metadata = HashMap::new();
    metadata.insert("voice".to_string(), voice.to_string());
    metadata.insert("model".to_string(), model.to_string());
    metadata.insert(
        "output_origin".to_string(),
        if user_supplied_output {
            "user_supplied".to_string()
        } else {
            "tool_default_output".to_string()
        },
    );
    let record = register_tool_output_artifact(
        manager,
        agent_id,
        "text_to_speech",
        &output_path.to_string_lossy(),
        ArtifactLifecycle::Session,
        "speech_output",
        metadata,
    )
    .await?;
    Ok(Some(
        ToolArtifactRegistration::from_record(&record).as_json(),
    ))
}

#[async_trait]
impl Tool for SpeakTool {
    fn name(&self) -> String {
        "text_to_speech".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Convert text to speech audio file.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to convert to speech"
                    },
                    "voice": {
                        "type": "string",
                        "description": "Voice to use (alloy, echo, fable, onyx, nova, shimmer)",
                        "default": "alloy"
                    },
                    "model": {
                        "type": "string",
                        "description": "TTS model to use (tts-1, tts-1-hd)",
                        "default": "tts-1"
                    },
                    "speed": {
                        "type": "number",
                        "description": "Speed of the speech (0.25 to 4.0)",
                        "default": 1.0
                    },
                    "output_filename": {
                        "type": "string",
                        "description": "Optional custom filename for the output mp3"
                    }
                },
                "required": ["text"]
            }),
            parameters_ts: Some("interface SpeakArgs { \n  text: string; // Text to convert\n  voice?: 'alloy' | 'echo' | 'fable' | 'onyx' | 'nova' | 'shimmer';\n  model?: 'tts-1' | 'tts-1-hd';\n  speed?: number; // 0.25 to 4.0\n  output_filename?: string;\n}".to_string()),
            is_binary: false,
            is_verified: false,
            usage_guidelines: Some("Use this to generate audio files from text descriptions or agent responses.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: SpeakArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: format!("Invalid arguments: {}", e),
            })?;

        let user_supplied_output = args.output_filename.is_some();
        let filename = args.output_filename.clone().unwrap_or_else(|| {
            format!(
                "speech_{}.mp3",
                uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
            )
        });
        let output_path = self.output_dir.join(filename);

        let voice = if args.voice == default_voice() {
            std::env::var("VOICE_TTS_VOICE").unwrap_or(args.voice)
        } else {
            args.voice
        };

        let model = if args.model == default_model() {
            std::env::var("VOICE_TTS_MODEL").unwrap_or(args.model)
        } else {
            args.model
        };

        // Use Sensory Hub for unified synthesis (Phase 11)
        use benshu_sensory::{SensoryOutput, SensoryRequest};

        // Map local enabled to plugin hint if needed, or follow hub policy
        let plugin_hint = if std::env::var("VOICE_LOCAL_TTS_ENABLED")
            .map(|v| v == "true")
            .unwrap_or(false)
        {
            Some("piper-local".to_string())
        } else {
            Some("cloud-tts".to_string())
        };

        let req = SensoryRequest::Speak {
            text: args.text.clone(),
            plugin_hint,
        };

        match self.sensory.dispatch(req).await? {
            SensoryOutput::Audio(bytes) => {
                if let Some(parent) = output_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let mut file = File::create(&output_path).await?;
                file.write_all(&bytes).await?;
                let artifact_registration = register_speech_output_artifact(
                    self.artifact_manager.as_deref(),
                    &self.agent_id,
                    &output_path,
                    &voice,
                    &model,
                    user_supplied_output,
                )
                .await?;
                Ok(serde_json::to_string_pretty(&json!({
                    "success": true,
                    "output_path": output_path.to_string_lossy(),
                    "voice": voice,
                    "model": model,
                    "cleanup": speak_cleanup().as_json(),
                    "artifact_registration": artifact_registration,
                }))?)
            }
            _ => Err(anyhow::anyhow!("Unexpected sensory response")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use benshu_state::ArtifactQuery;
    use tempfile::tempdir;

    #[test]
    fn speak_cleanup_reports_durable_output() {
        let cleanup = speak_cleanup();
        assert_eq!(
            cleanup.schema_version,
            crate::tool::TOOL_CLEANUP_SCHEMA_VERSION
        );
        assert!(!cleanup.active);
    }

    #[tokio::test]
    async fn register_speech_output_artifact_persists_when_registry_is_present() {
        let db_path = std::env::temp_dir().join(format!(
            "benshu_voice_artifact_test_{}.redb",
            uuid::Uuid::new_v4()
        ));
        let db = redb::Database::create(&db_path).expect("db");
        let manager = ArtifactManager::new(Arc::new(db));
        let temp = tempdir().expect("tempdir");
        let output_path = temp.path().join("speech.mp3");
        tokio::fs::write(&output_path, b"fake audio")
            .await
            .expect("audio");

        let result = register_speech_output_artifact(
            Some(&manager),
            "voice-agent",
            &output_path,
            "alloy",
            "tts-1",
            false,
        )
        .await
        .expect("registration")
        .expect("artifact registration");
        assert_eq!(result["registered"].as_bool(), Some(true));

        let artifacts = manager
            .query(&ArtifactQuery {
                source_kind: Some("builtin_tool_output".to_string()),
                limit: Some(10),
                ..ArtifactQuery::default()
            })
            .await
            .expect("query");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].tool_name.as_deref(), Some("text_to_speech"));
        assert_eq!(artifacts[0].uri, output_path.to_string_lossy());

        let _ = tokio::fs::remove_file(&db_path).await;
    }
}
