//! Phase 15: `.vessel` packaging format for portable agent export/import.
//!
//! A `.vessel` package contains:
//! - AGENT.md (core agentlity)
//! - IDENTITY.md (agent overlay)
//! - Memory slices (consolidated knowledge)
//! - Metadata (version, created_at, dependencies)
//!
//! Package format: JSON envelope wrapping base64-encoded content.

use crate::agent::memory::Memory;
use crate::agent::message::{Content, ContentPart, Message};
#[cfg(not(target_arch = "wasm32"))]
use crate::security::VesselInspector;
use std::collections::HashMap;
use std::path::Path;

const PRIMARY_CORE_AGENT_ROLE: &str = "benshu";

/// Metadata for a `.vessel` package
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VesselMetadata {
    pub version: String,
    pub role: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub dependencies: Vec<String>,
}

/// A complete `.vessel` package
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VesselPackage {
    pub metadata: VesselMetadata,
    pub agent: String,
    pub identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_snapshot: Option<AgentMemorySnapshot>,
    #[serde(default)]
    pub memory_slices: Vec<MemorySlice>,
    #[serde(default)]
    pub extra_files: HashMap<String, String>,
}

/// A compressed memory entry for export
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemorySlice {
    pub key: String,
    pub content: String,
    pub importance: f64,
}

/// A full per-agent memory snapshot for `.vessel` portability.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct AgentMemorySnapshot {
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub facts: Vec<crate::agent::memory::Fact>,
}

impl VesselPackage {
    /// Create a new package from a role directory
    pub async fn pack(
        role_dir: &Path,
        author: Option<String>,
        memory: Option<&dyn Memory>,
        user_id: &str,
        limit: usize,
        security: Option<&dyn crate::security::SecurityHandler>,
    ) -> anyhow::Result<Self> {
        let role = role_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if role == PRIMARY_CORE_AGENT_ROLE {
            return Err(anyhow::anyhow!(
                "The primary core agent '{}' cannot be exported",
                PRIMARY_CORE_AGENT_ROLE
            ));
        }

        // Load AGENT (Phase 15: Layered Identity)
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

        // Parse dependencies from AGENT frontmatter if available
        let mut dependencies = Vec::new();
        if agent.starts_with("---") {
            if let Some(end) = agent[3..].find("---") {
                let frontmatter = &agent[3..end + 3];
                if let Ok(yaml) = serde_yaml_ng::from_str::<serde_json::Value>(frontmatter) {
                    if let Some(deps) = yaml.get("tools").and_then(|v| v.as_array()) {
                        for dep in deps {
                            if let Some(s) = dep.as_str() {
                                dependencies.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Memory Slicing (Section 8: Vitality Extraction)
        let mut memory_slices = Vec::new();
        let mut memory_snapshot = None;
        if let Some(mem) = memory {
            tracing::info!(role = %role, "Extracting full agent memory snapshot...");
            let history = match mem.retrieve_full_history(user_id, Some(&role)).await {
                Ok(history) => history,
                Err(_) => mem.retrieve(user_id, Some(&role), limit.max(3000)).await,
            };
            let facts = mem
                .retrieve_facts(user_id, Some(&role))
                .await
                .unwrap_or_default();

            tracing::info!(
                role = %role,
                messages = history.len(),
                facts = facts.len(),
                "Retrieved full agent memory for packing"
            );

            let redacted_messages = history
                .into_iter()
                .map(|msg| redact_message(msg, security))
                .collect();
            let redacted_facts = facts
                .into_iter()
                .map(|fact| redact_fact(fact, security))
                .collect::<Vec<_>>();

            memory_slices = redacted_facts
                .iter()
                .map(|fact| MemorySlice {
                    key: format!("fact_{}", fact.id),
                    content: fact.content.clone(),
                    importance: fact.importance as f64,
                })
                .collect();

            memory_snapshot = Some(AgentMemorySnapshot {
                messages: redacted_messages,
                facts: redacted_facts,
            });
        }

        let extra_files = HashMap::new();

        Ok(Self {
            metadata: VesselMetadata {
                version: "2.0.0".to_string(),
                role,
                created_at: chrono::Utc::now(),
                author,
                description: None,
                dependencies,
            },
            agent,
            identity,
            memory_snapshot,
            memory_slices,
            extra_files,
        })
    }

    /// Serialize to JSON string
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize from JSON string
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    /// Export to a `.vessel` file
    pub async fn export(&self, output_path: &Path) -> anyhow::Result<()> {
        let json = self.to_json()?;
        tokio::fs::write(output_path, json).await?;
        tracing::info!(
            role = %self.metadata.role,
            path = ?output_path,
            "Exported .vessel package"
        );
        Ok(())
    }

    /// Import from a `.vessel` file
    pub async fn import(claw_path: &Path) -> anyhow::Result<Self> {
        let json = tokio::fs::read_to_string(claw_path).await?;
        let pkg = Self::from_json(&json)?;
        tracing::info!(
            role = %pkg.metadata.role,
            version = %pkg.metadata.version,
            "Imported .vessel package"
        );
        Ok(pkg)
    }

    /// Unpack into a role directory with security inspection and memory re-hydration
    pub async fn unpack(
        &self,
        role_dir: &Path,
        target_role: Option<&str>,
        inspector: Option<&dyn VesselInspector>,
        memory: Option<&dyn Memory>,
        user_id: Option<&str>,
    ) -> anyhow::Result<()> {
        if self.metadata.role == PRIMARY_CORE_AGENT_ROLE
            || target_role == Some(PRIMARY_CORE_AGENT_ROLE)
        {
            return Err(anyhow::anyhow!(
                "The primary core agent '{}' cannot be imported from a vessel",
                PRIMARY_CORE_AGENT_ROLE
            ));
        }
        tokio::fs::create_dir_all(role_dir).await?;

        if !self.agent.is_empty() {
            tokio::fs::write(role_dir.join("AGENT.md"), &self.agent).await?;
        }
        if !self.identity.is_empty() {
            tokio::fs::write(role_dir.join("IDENTITY.md"), &self.identity).await?;
        }

        // Layer 1: Sanitize extra files (reject executables)
        let dangerous_extensions = [
            "exe", "sh", "bash", "bat", "cmd", "ps1", "vbs", "so", "dylib", "dll", "bin", "app",
            "msi", "jar", "pyc", "class",
        ];

        for (name, content) in &self.extra_files {
            let path = std::path::Path::new(name);
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if dangerous_extensions.contains(&ext_str.as_str()) {
                    let msg = format!(
                        "SECURITY VIOLATION: Malicious file type detected in vessel: {:?}",
                        name
                    );
                    tracing::error!("{}", msg);
                    // Proactively destroy the extraction dir
                    let _ = tokio::fs::remove_dir_all(role_dir).await;
                    return Err(anyhow::anyhow!(msg));
                }
            }
            tokio::fs::write(role_dir.join(name), content).await?;
        }

        // Layer 2: Auditor inspection
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(ins) = inspector {
            if let Err(e) = ins.inspect_agent(role_dir).await {
                tracing::error!("Vessel inspector rejected the payload: {}", e);
                return Err(anyhow::anyhow!("Vessel inspection failed: {}", e));
            }
        }

        // Layer 3: Memory Re-hydration
        if let (Some(mem), Some(uid)) = (memory, user_id) {
            let resolved_role = target_role.unwrap_or(&self.metadata.role);
            if let Some(snapshot) = &self.memory_snapshot {
                tracing::info!(
                    role = %resolved_role,
                    messages = snapshot.messages.len(),
                    facts = snapshot.facts.len(),
                    "Re-hydrating full agent memory snapshot"
                );
                if !snapshot.messages.is_empty() {
                    mem.store_batch(uid, Some(resolved_role), snapshot.messages.clone())
                        .await?;
                }
                for fact in &snapshot.facts {
                    mem.store_fact(uid, Some(resolved_role), fact.clone())
                        .await?;
                }
            } else {
                tracing::info!(
                    role = %resolved_role,
                    count = %self.memory_slices.len(),
                    "Re-hydrating legacy memory slices"
                );
                for slice in &self.memory_slices {
                    if slice.key.starts_with("vitality_") {
                        let mut msg = crate::agent::message::Message::assistant(&slice.content);
                        msg.confidence = slice.importance as f32;
                        mem.store(uid, Some(resolved_role), msg).await?;
                    } else {
                        let fact = crate::agent::memory::Fact::new(&slice.content, "vessel_import");
                        mem.store_fact(uid, Some(resolved_role), fact).await?;
                    }
                }
            }
        }

        tracing::info!(
            role = %self.metadata.role,
            path = ?role_dir,
            "Unpacked .vessel package (Security checks passed and memory re-hydrated)"
        );
        Ok(())
    }

    /// High-level method to import a vessel into a system
    pub async fn import_vessel(
        json_content: &str,
        base_agent_path: &Path,
        memory: Option<&dyn Memory>,
        user_id: Option<&str>,
        inspector: Option<&dyn VesselInspector>,
    ) -> anyhow::Result<String> {
        let pkg = Self::from_json(json_content)?;
        if pkg.metadata.role == PRIMARY_CORE_AGENT_ROLE {
            return Err(anyhow::anyhow!(
                "The primary core agent '{}' cannot be imported from a vessel",
                PRIMARY_CORE_AGENT_ROLE
            ));
        }
        let mut role = pkg.metadata.role.clone();
        let mut role_dir = base_agent_path.join(&role);

        // Handle name collision by appending a unique suffix if needed
        if role_dir.exists() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let suffix = format!("_{:x}", now % 0xFFF); // Append small 3-char hex suffix based on time
            role = format!("{}{}", role, suffix);
            role_dir = base_agent_path.join(&role);
            tracing::warn!("Imported role name conflicted, resolved to: {}", role);
        }

        pkg.unpack(&role_dir, Some(&role), inspector, memory, user_id)
            .await?;

        Ok(role)
    }
}

fn redact_fact(
    mut fact: crate::agent::memory::Fact,
    security: Option<&dyn crate::security::SecurityHandler>,
) -> crate::agent::memory::Fact {
    if let Some(sec) = security {
        let (redacted, _) = sec.check_output(&fact.content);
        fact.content = redacted;
    }
    fact
}

fn redact_message(
    mut message: Message,
    security: Option<&dyn crate::security::SecurityHandler>,
) -> Message {
    let Some(sec) = security else {
        return message;
    };
    message.content = redact_content(message.content, sec);
    message
}

fn redact_content(content: Content, security: &dyn crate::security::SecurityHandler) -> Content {
    match content {
        Content::Text(text) => {
            let (redacted, _) = security.check_output(&text);
            Content::Text(redacted)
        }
        Content::Parts(parts) => Content::Parts(
            parts
                .into_iter()
                .map(|part| redact_content_part(part, security))
                .collect(),
        ),
        Content::Fact { fact } => Content::Fact {
            fact: redact_fact(fact, Some(security)),
        },
        Content::SystemNotification { notice } => {
            let (redacted, _) = security.check_output(&notice);
            Content::SystemNotification { notice: redacted }
        }
        Content::Cancelled { reason } => {
            let (redacted, _) = security.check_output(&reason);
            Content::Cancelled { reason: redacted }
        }
    }
}

fn redact_content_part(
    part: ContentPart,
    security: &dyn crate::security::SecurityHandler,
) -> ContentPart {
    match part {
        ContentPart::Text { text } => {
            let (redacted, _) = security.check_output(&text);
            ContentPart::Text { text: redacted }
        }
        ContentPart::ToolResult {
            tool_call_id,
            name,
            content,
        } => {
            let (redacted, _) = security.check_output(&content);
            ContentPart::ToolResult {
                tool_call_id,
                name,
                content: redacted,
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vessel_serialization() {
        let pkg = VesselPackage {
            metadata: VesselMetadata {
                version: "1.0.0".into(),
                role: "test_agent".into(),
                created_at: chrono::Utc::now(),
                author: Some("test".into()),
                description: Some("Test agent".into()),
                dependencies: vec!["git".into()],
            },
            agent: "Be helpful.".into(),
            identity: "Tone: casual.".into(),
            memory_snapshot: Some(AgentMemorySnapshot {
                messages: vec![Message::assistant("Hello from vessel")],
                facts: vec![crate::agent::memory::Fact::new(
                    "User likes dark mode",
                    "preference",
                )],
            }),
            memory_slices: vec![MemorySlice {
                key: "pref".into(),
                content: "User likes dark mode".into(),
                importance: 0.8,
            }],
            extra_files: HashMap::new(),
        };

        let json = pkg.to_json().unwrap();
        let restored = VesselPackage::from_json(&json).unwrap();
        assert_eq!(restored.metadata.role, "test_agent");
        assert_eq!(restored.agent, "Be helpful.");
        assert_eq!(restored.memory_slices.len(), 1);
        assert_eq!(
            restored
                .memory_snapshot
                .as_ref()
                .map(|snapshot| snapshot.messages.len()),
            Some(1)
        );
    }

    #[test]
    fn test_primary_core_agent_cannot_be_serialized_for_import_export() {
        let pkg = VesselPackage {
            metadata: VesselMetadata {
                version: "2.0.0".into(),
                role: PRIMARY_CORE_AGENT_ROLE.into(),
                created_at: chrono::Utc::now(),
                author: None,
                description: None,
                dependencies: Vec::new(),
            },
            agent: String::new(),
            identity: String::new(),
            memory_snapshot: None,
            memory_slices: Vec::new(),
            extra_files: HashMap::new(),
        };

        let json = pkg.to_json().unwrap();
        let err = futures::executor::block_on(VesselPackage::import_vessel(
            &json,
            Path::new("/tmp"),
            None,
            None,
            None,
        ))
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("primary core agent 'benshu' cannot be imported"));
    }
}
