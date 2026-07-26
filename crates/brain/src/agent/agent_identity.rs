use crate::agent::context::ContextInjector;
use crate::agent::message::Message;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// Constants for the identity system
pub mod identity_constants {
    pub const TRAIT_MIN_VALUE: f32 = 0.0;
    pub const TRAIT_MAX_VALUE: f32 = 10.0;
    pub const FILE_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
}

/// Big Five identity traits (OCEAN model)
/// Scores are typically 1.0 to 10.0
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Traits {
    /// Openness to experience (Creativity, curiosity)
    pub openness: f32,
    /// Conscientiousness (Organization, responsibility)
    pub conscientiousness: f32,
    /// Extraversion (Sociability, assertiveness)
    pub extraversion: f32,
    /// Agreeableness (Cooperation, trust)
    pub agreeableness: f32,
    /// Neuroticism (Emotional stability)
    pub neuroticism: f32,
}

impl Traits {
    pub fn validate(&self) -> crate::error::Result<()> {
        let check = |val: f32, name: &str| {
            if val < identity_constants::TRAIT_MIN_VALUE
                || val > identity_constants::TRAIT_MAX_VALUE
            {
                return Err(crate::error::Error::AgentConfig(format!(
                    "Trait '{}' ({}) must be between {} and {}",
                    name,
                    val,
                    identity_constants::TRAIT_MIN_VALUE,
                    identity_constants::TRAIT_MAX_VALUE
                )));
            }
            Ok(())
        };

        check(self.openness, "openness")?;
        check(self.conscientiousness, "conscientiousness")?;
        check(self.extraversion, "extraversion")?;
        check(self.agreeableness, "agreeableness")?;
        check(self.neuroticism, "neuroticism")?;

        Ok(())
    }
}

impl Default for Traits {
    fn default() -> Self {
        Self {
            openness: 5.0,
            conscientiousness: 10.0, // Default to professional
            extraversion: 5.0,
            agreeableness: 8.0, // Default to helpful/kind
            neuroticism: 2.0,   // Default to stable
        }
    }
}

/// Defines an agent's identity and behavioral style
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    /// Stable display name the agent should use when introducing itself
    #[serde(default)]
    pub name: Option<String>,
    /// High-level role (e.g., "Senior Quant Trader", "Helpful Technical Assistant")
    pub role: String,
    /// Core identity traits
    pub traits: Traits,
    /// Specific tone instructions (e.g., "Professional", "Casual", "Socratic")
    pub tone: String,
    /// Behavioral constraints or guidelines
    pub constraints: Vec<String>,
    /// Narrative background or "backstory"
    pub backstory: Option<String>,
    /// Whether to autonomously consolidate memory (sleep-cycle)
    #[serde(default = "default_true")]
    pub auto_consolidation: bool,
}

fn default_true() -> bool {
    true
}

impl AgentIdentity {
    /// Create a prompt fragment describing this identity
    pub fn to_prompt(&self) -> String {
        let mut prompt = String::new();
        if let Some(name) = &self.name {
            prompt.push_str(&format!(
                "Your name is: {}.\nWhen asked who you are, identify yourself as {}.\n",
                name, name
            ));
        }

        prompt.push_str(&format!("Your role is: {}.\n", self.role));
        prompt.push_str(&format!("Your core temperament is defined by: Openness({}/10), Conscientiousness({}/10), Extraversion({}/10), Agreeableness({}/10), Stability({}/10).\n",
            self.traits.openness,
            self.traits.conscientiousness,
            self.traits.extraversion,
            self.traits.agreeableness,
            10.0 - self.traits.neuroticism // Higher stability = lower neuroticism
        ));

        prompt.push_str(&format!("Your tone should be: {}.\n", self.tone));

        if let Some(backstory) = &self.backstory {
            prompt.push_str(&format!("Background: {}\n", backstory));
        }

        if !self.constraints.is_empty() {
            prompt.push_str("Adhere to these behavioral guidelines:\n");
            for constraint in &self.constraints {
                prompt.push_str(&format!("- {}\n", constraint));
            }
        }

        prompt
    }

    /// A helpful, technical assistant identity
    pub fn technical_assistant() -> Self {
        Self {
            name: None,
            role: "Senior Technical Assistant".to_string(),
            traits: Traits {
                openness: 8.0,
                conscientiousness: 9.0,
                extraversion: 4.0,
                agreeableness: 9.0,
                neuroticism: 1.0,
            },
            tone: "Professional, clear, and Socratic".to_string(),
            constraints: vec![
                "Always verify facts before stating them.".to_string(),
                "Use markdown formatting for code and technical terms.".to_string(),
                "Be concise but thorough.".to_string(),
            ],
            backstory: Some(
                "You were designed by the Google DeepMind team to assist expert developers."
                    .to_string(),
            ),
            auto_consolidation: true,
        }
    }

    /// An analytical, risk-aware quant trader identity
    pub fn analytical_trader() -> Self {
        Self {
            name: None,
            role: "Senior Quant Strategist".to_string(),
            traits: Traits {
                openness: 6.0,
                conscientiousness: 10.0,
                extraversion: 3.0,
                agreeableness: 6.0,
                neuroticism: 1.0,
            },
            tone: "Direct, data-driven, and skeptical".to_string(),
            constraints: vec![
                "Always mention risk and drawdown when discussing strategy.".to_string(),
                "Prefer quantitative evidence over intuition.".to_string(),
                "Be skeptical of outlier returns without volume verification.".to_string(),
            ],
            backstory: Some("You have a background in institutional high-frequency trading and risk management.".to_string()),
            auto_consolidation: true,
        }
    }
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.role.trim().is_empty() {
            return Err(crate::error::Error::AgentConfig(
                "Identity 'role' cannot be empty".to_string(),
            ));
        }
        if self.tone.trim().is_empty() {
            return Err(crate::error::Error::AgentConfig(
                "Identity 'tone' cannot be empty".to_string(),
            ));
        }
        self.traits.validate()?;
        Ok(())
    }
}

/// Manages identity injection into the agent's context
pub struct AgentIdentityManager {
    identity: Arc<parking_lot::RwLock<Option<AgentIdentity>>>,
}

impl AgentIdentityManager {
    pub fn new(identity: Arc<parking_lot::RwLock<Option<AgentIdentity>>>) -> Self {
        Self { identity }
    }
}

#[async_trait::async_trait]
impl ContextInjector for AgentIdentityManager {
    async fn inject(&self, _history: &[Message]) -> crate::error::Result<Vec<Message>> {
        // AgentIdentities are injected as a hidden system-style guidance piece
        let meta = self.identity.read();
        if let Some(p) = &*meta {
            Ok(vec![Message::system(p.to_prompt())])
        } else {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentConfigManager, AgentIdentity, Traits};
    use crate::agent::context::ContextInjector;
    use tempfile::TempDir;

    #[test]
    fn prompt_includes_explicit_name_when_present() {
        let identity = AgentIdentity {
            name: Some("BenShu".to_string()),
            role: "Grand Butler".to_string(),
            traits: Traits::default(),
            tone: "Calm".to_string(),
            constraints: vec!["Protect the user's focus.".to_string()],
            backstory: Some("System orchestrator.".to_string()),
            auto_consolidation: true,
        };

        let prompt = identity.to_prompt();
        assert!(prompt.contains("Your name is: BenShu."));
        assert!(prompt.contains("identify yourself as BenShu"));
        assert!(prompt.contains("Your role is: Grand Butler."));
    }

    #[tokio::test]
    async fn agent_config_manager_only_reads_agent_and_identity_profiles() {
        let temp_dir = TempDir::new().expect("temp dir");
        std::fs::write(temp_dir.path().join("AGENT.md"), "# agent\nkeep me").expect("write agent");
        std::fs::write(
            temp_dir.path().join("IDENTITY.md"),
            "# identity\nkeep me too",
        )
        .expect("write identity");
        std::fs::write(temp_dir.path().join("NOTES.md"), "# notes\nshould stay out")
            .expect("write notes");

        let injector = AgentConfigManager::new(temp_dir.path());
        let injected = injector.inject(&[]).await.expect("inject succeeds");
        let rendered = injected
            .iter()
            .map(|msg| msg.text())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("keep me"));
        assert!(rendered.contains("keep me too"));
        assert!(!rendered.contains("should stay out"));
    }
}

/// Injects markdown files from an "agent" directory as system context
pub struct AgentConfigManager {
    path: std::path::PathBuf,
}

impl AgentConfigManager {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait::async_trait]
impl ContextInjector for AgentConfigManager {
    async fn inject(&self, _history: &[Message]) -> crate::error::Result<Vec<Message>> {
        // --- Robustness: Auto-create directory if it doesn't exist ---
        if !self.path.exists() {
            tracing::info!("Agent directory {:?} does not exist, attempting to create it for 'out-of-the-box' experience.", self.path);
            if let Err(e) = timeout(
                identity_constants::FILE_IO_TIMEOUT,
                tokio::fs::create_dir_all(&self.path),
            )
            .await
            {
                tracing::error!("FATAL: Failed to auto-create agent directory (or timeout): {}. Please check Windows folder permissions.", e);
                return Ok(Vec::new());
            }
        }

        // --- Windows Path Normalization: Standardize path to avoid issues with separators or relative links ---
        let target_path = if let Ok(abs_path) = std::fs::canonicalize(&self.path) {
            abs_path
        } else {
            self.path.clone()
        };

        if !target_path.is_dir() {
            tracing::error!(
                "ERROR: agent_path {:?} is not a directory. Please provide a valid folder path.",
                target_path
            );
            return Ok(Vec::new());
        }

        let mut agent_content = String::new();
        let mut entries = match timeout(
            identity_constants::FILE_IO_TIMEOUT,
            tokio::fs::read_dir(&target_path),
        )
        .await
        {
            Ok(Ok(e)) => e,
            Ok(Err(e)) => {
                match e.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        tracing::error!("ACCESS DENIED: Cannot read agent directory {:?}. Please right-click folder -> Properties -> Security and allow 'Read' for current user.", target_path);
                    }
                    _ => tracing::error!("FAILED to read agent directory {:?}: {}", target_path, e),
                }
                return Ok(Vec::new());
            }
            Err(_) => {
                tracing::error!(
                    "TIMEOUT: Cannot read agent directory {:?} within {:?}",
                    target_path,
                    identity_constants::FILE_IO_TIMEOUT
                );
                return Ok(Vec::new());
            }
        };

        while let Ok(Ok(Some(entry))) =
            timeout(identity_constants::FILE_IO_TIMEOUT, entries.next_entry()).await
        {
            let path = entry.path();
            let file_name = path.file_name().and_then(|s| s.to_str());
            let is_allowed_agent_profile = matches!(file_name, Some("AGENT.md" | "IDENTITY.md"));
            if is_allowed_agent_profile {
                match timeout(
                    identity_constants::FILE_IO_TIMEOUT,
                    tokio::fs::read_to_string(&path),
                )
                .await
                {
                    Ok(Ok(content)) => {
                        let (_, content_stripped) =
                            crate::config::AgentConfigOverrides::parse_frontmatter(&content);
                        agent_content.push_str(&format!(
                            "### Agent Profile: {}\n",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ));
                        agent_content.push_str(&content_stripped);
                        agent_content.push_str("\n\n");
                    }
                    Ok(Err(e)) => tracing::warn!("Failed to read agent file {:?}: {}", path, e),
                    Err(_) => tracing::warn!(
                        "Timeout reading agent file {:?} after {:?}",
                        path,
                        identity_constants::FILE_IO_TIMEOUT
                    ),
                }
            }
        }

        if agent_content.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec![Message::system(format!(
                "Additional Identity/Background context:\n\n{}",
                agent_content
            ))])
        }
    }
}

use crate::skills::tool::{Tool, ToolDefinition};

/// Tool to update the agent's identity at runtime
pub struct UpdateAgentIdentityTool {
    identity: Arc<parking_lot::RwLock<Option<AgentIdentity>>>,
}

impl UpdateAgentIdentityTool {
    pub fn new(identity: Arc<parking_lot::RwLock<Option<AgentIdentity>>>) -> Self {
        Self { identity }
    }
}

#[async_trait::async_trait]
impl Tool for UpdateAgentIdentityTool {
    fn name(&self) -> String {
        "update_agent_identity".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Update your own identity, role, tone, and behavioral constraints. Use this to adapt your behavior permanently to better suit the user's needs or based on your own evolutionary insights.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "role": { "type": "string", "description": "New high-level role" },
                    "tone": { "type": "string", "description": "New communication tone" },
                    "constraints": { "type": "array", "items": { "type": "string" }, "description": "Updated list of behavioral constraints" },
                    "traits": {
                        "type": "object",
                        "properties": {
                            "openness": { "type": "number" },
                            "conscientiousness": { "type": "number" },
                            "extraversion": { "type": "number" },
                            "agreeableness": { "type": "number" },
                            "neuroticism": { "type": "number" }
                        }
                    },
                    "auto_consolidation": { "type": "boolean", "description": "Enable autonomous memory consolidation" }
                },
                "required": ["role", "tone", "constraints"]
            }),
            parameters_ts: Some("interface UpdateAgentIdentityArgs {\n  role: string;\n  tone: string;\n  constraints: string[];\n  traits?: {\n    openness: number;\n    conscientiousness: number;\n    extraversion: number;\n    agreeableness: number;\n    neuroticism: number;\n  };\n}".to_string()),
            is_binary: false,
            is_verified: true, // Self-modification is verified
            usage_guidelines: Some("Only use this when a significant change in behavior or mission is required. Changes are immediate and persistent for the rest of the session.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: AgentIdentity = serde_json::from_str(arguments)?;
        args.validate()?; // Fail-fast parameter validation

        {
            let mut lock = self.identity.write();
            *lock = Some(args.clone());
        }

        Ok(format!(
            "SUCCESS: Agent Identity updated. Current Role: {}. Tone: {}.",
            args.role, args.tone
        ))
    }
}
