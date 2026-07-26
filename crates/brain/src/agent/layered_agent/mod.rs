//! Phase 15: Agent Layering — AGENT + IDENTITY dual-layer architecture.
//!
//! Separates agent identity into two layers:
//! - AGENT.md: Core agentlity, immutable values, system defense directives
//! - IDENTITY.md: Visual aesthetics, communication tone, scenario settings
//!
//! Also provides the `.vessel` packaging format for portable agent export.

pub mod vessel_pack;

use std::path::{Path, PathBuf};

/// Represents the dual-layer identity of an agent
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayeredAgent {
    /// Role name
    pub role: String,
    /// AGENT layer: core agentlity and values (immutable foundation)
    pub agent: String,
    /// IDENTITY layer: tone, aesthetics, scenario (mutable overlay)
    pub identity: String,
}

impl LayeredAgent {
    /// Load a layered identity from a role directory
    pub async fn load(role_dir: &Path) -> anyhow::Result<Self> {
        let role = role_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Load AGENT: try AGENT.md first, fall back to AGENT.md then AGENT.md
        let agent_path = role_dir.join("AGENT.md");

        let agent = if agent_path.exists() {
            tokio::fs::read_to_string(&agent_path).await?
        } else {
            String::new()
        };

        // Load IDENTITY
        let identity_path = role_dir.join("IDENTITY.md");
        let identity = if identity_path.exists() {
            tokio::fs::read_to_string(&identity_path).await?
        } else {
            String::new()
        };

        Ok(Self {
            role,
            agent,
            identity,
        })
    }

    /// Save the layered identity to a role directory
    pub async fn save(&self, role_dir: &Path) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(role_dir).await?;

        if !self.agent.is_empty() {
            tokio::fs::write(role_dir.join("AGENT.md"), &self.agent).await?;
        }
        if !self.identity.is_empty() {
            tokio::fs::write(role_dir.join("IDENTITY.md"), &self.identity).await?;
        }

        Ok(())
    }

    /// Compose the full system prompt from both layers
    pub fn compose_system_prompt(&self) -> String {
        let mut prompt = String::new();

        if !self.agent.is_empty() {
            prompt.push_str("## AGENT (Core Agentlity)\n\n");
            prompt.push_str(&self.agent);
            prompt.push_str("\n\n");
        }

        if !self.identity.is_empty() {
            prompt.push_str("## IDENTITY (Visual Agent)\n\n");
            prompt.push_str(&self.identity);
        }

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compose_system_prompt() {
        let agent = LayeredAgent {
            role: "benshu".into(),
            agent: "Be helpful, precise, and concise.".into(),
            identity: "Tone: professional. Style: minimal.".into(),
        };
        let prompt = agent.compose_system_prompt();
        assert!(prompt.contains("AGENT"));
        assert!(prompt.contains("IDENTITY"));
        assert!(prompt.contains("Be helpful"));
        assert!(prompt.contains("professional"));
    }

    #[test]
    fn test_compose_empty_identity() {
        let agent = LayeredAgent {
            role: "test".into(),
            agent: "Core values.".into(),
            identity: String::new(),
        };
        let prompt = agent.compose_system_prompt();
        assert!(prompt.contains("Core values"));
        assert!(!prompt.contains("IDENTITY"));
    }
}
