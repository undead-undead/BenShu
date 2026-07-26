use crate::SkillMetadata;
use benshu_infra::error::{Error, Result};
use serde_yaml_ng;
use std::path::{Component, Path};

pub struct SkillParser;

impl SkillParser {
    pub async fn parse_file(path: &Path) -> Result<(SkillMetadata, String)> {
        let manifest_path = path.join("SKILL.md");
        if !manifest_path.exists() {
            return Err(Error::Internal(format!("No SKILL.md found at {:?}", path)));
        }

        let content = tokio::fs::read_to_string(&manifest_path).await?;
        Self::parse_str(&content, path)
    }

    pub fn parse_str(content: &str, base_path: &Path) -> Result<(SkillMetadata, String)> {
        // Find frontmatter delimiters
        let start_delimiter = "---\n";
        let end_delimiter = "\n---";

        let yaml_str;
        let instructions;

        // Ensure file starts with YAML frontmatter
        if content.starts_with(start_delimiter) || content.starts_with("---\r\n") {
            // Find end of frontmatter
            if let Some(end_idx) = content[4..].find(end_delimiter) {
                let actual_end_idx = end_idx + 4; // Add back the initial offset
                yaml_str = &content[4..actual_end_idx];

                let rest_start = actual_end_idx + 4;
                if rest_start < content.len() {
                    instructions = content[rest_start..].trim().to_string();
                } else {
                    instructions = String::new();
                }
            } else {
                return Err(Error::Internal(
                    "SKILL.md frontmatter unclosed (missing closing ---)".to_string(),
                ));
            }
        } else {
            return Err(Error::Internal("SKILL.md must start with ---".to_string()));
        }

        let mut metadata: SkillMetadata = serde_yaml_ng::from_str(yaml_str)
            .map_err(|e| Error::Internal(format!("Failed to parse Skill YAML: {}", e)))?;

        // --- Compatibility Fixes: Inference ---
        if metadata.script.is_none() || metadata.runtime.is_none() {
            if metadata.script.is_none() {
                let scripts_dir = base_path.join("scripts");
                if scripts_dir.exists() {
                    if let Ok(mut entries) = std::fs::read_dir(scripts_dir) {
                        if let Some(Ok(first_entry)) = entries.next() {
                            let filename = first_entry.file_name().to_string_lossy().to_string();
                            metadata.script = Some(filename.clone());

                            if metadata.runtime.is_none() {
                                if filename.ends_with(".py") {
                                    metadata.runtime = Some("python3".into());
                                } else if filename.ends_with(".js") {
                                    metadata.runtime = Some("node".into());
                                } else if filename.ends_with(".sh") {
                                    metadata.runtime = Some("bash".into());
                                }
                            }
                        }
                    }
                }
            }

            if metadata.runtime.is_none() {
                if instructions.contains("python3") {
                    metadata.runtime = Some("python3".into());
                } else if instructions.contains("node") {
                    metadata.runtime = Some("node".into());
                } else if instructions.contains("bash") || instructions.contains("sh ") {
                    metadata.runtime = Some("bash".into());
                }
            }
        }

        Self::validate_manifest(&metadata, base_path)?;

        Ok((metadata, instructions))
    }

    fn validate_manifest(metadata: &SkillMetadata, base_path: &Path) -> Result<()> {
        if let Some(script) = metadata.script.as_deref() {
            Self::validate_relative_script_path(script)?;
            let script_path = base_path.join("scripts").join(script);
            if !script_path.starts_with(base_path.join("scripts")) {
                return Err(Error::Internal(format!(
                    "Skill '{}' script path escapes scripts directory",
                    metadata.name
                )));
            }
        }

        if metadata.runtime.as_deref().is_some_and(is_wasm_runtime) {
            let Some(script) = metadata.script.as_deref() else {
                return Err(Error::Internal(format!(
                    "Wasm skill '{}' must declare script",
                    metadata.name
                )));
            };

            if !script.to_ascii_lowercase().ends_with(".wasm") {
                return Err(Error::Internal(format!(
                    "Wasm skill '{}' script must end with .wasm",
                    metadata.name
                )));
            }

            let contract = metadata.wasm.clone().unwrap_or_default();
            if contract.entrypoint.trim().is_empty() {
                return Err(Error::Internal(format!(
                    "Wasm skill '{}' must declare a non-empty entrypoint",
                    metadata.name
                )));
            }

            if contract.abi != "wasi-component-run-string-v1" {
                return Err(Error::Internal(format!(
                    "Wasm skill '{}' uses unsupported ABI '{}'",
                    metadata.name, contract.abi
                )));
            }

            if metadata.use_browser || metadata.permissions.browser {
                return Err(Error::Internal(format!(
                    "Wasm skill '{}' cannot request browser access in the current protocol",
                    metadata.name
                )));
            }

            if metadata.permissions.network {
                return Err(Error::Internal(format!(
                    "Wasm skill '{}' cannot request network access in the current protocol",
                    metadata.name
                )));
            }
        }

        Ok(())
    }

    fn validate_relative_script_path(script: &str) -> Result<()> {
        let path = Path::new(script);
        if path.is_absolute() {
            return Err(Error::Internal(format!(
                "Skill script path must be relative: {}",
                script
            )));
        }

        if path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(Error::Internal(format!(
                "Skill script path must not escape scripts/: {}",
                script
            )));
        }

        Ok(())
    }
}

fn is_wasm_runtime(runtime: &str) -> bool {
    runtime.eq_ignore_ascii_case("wasm")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn wasm_manifest_accepts_explicit_local_contract() {
        let temp = tempdir().expect("tempdir");
        let content = r#"---
name: clean_markdown
description: Clean webpage text into Markdown.
runtime: wasm
script: clean_markdown.wasm
interface: "type Args = { input: string }"
permissions:
  filesystem: read_skill
  network: false
resources:
  timeout_secs: 5
  max_memory_mb: 64
wasm:
  abi: wasi-component-run-string-v1
  entrypoint: run
---
# Clean Markdown
"#;

        let (metadata, _) =
            SkillParser::parse_str(content, temp.path()).expect("valid wasm manifest");

        assert_eq!(metadata.runtime.as_deref(), Some("wasm"));
        assert_eq!(metadata.script.as_deref(), Some("clean_markdown.wasm"));
        assert_eq!(metadata.resources.timeout_secs, Some(5));
        assert_eq!(
            metadata.wasm.expect("wasm contract").abi,
            "wasi-component-run-string-v1"
        );
    }

    #[test]
    fn wasm_manifest_rejects_network_permission() {
        let temp = tempdir().expect("tempdir");
        let content = r#"---
name: net_wasm
description: Networked wasm skill
runtime: wasm
script: net.wasm
permissions:
  network: true
---
# Net Wasm
"#;

        let err = SkillParser::parse_str(content, temp.path())
            .expect_err("networked wasm is not supported yet");
        assert!(err.to_string().contains("cannot request network access"));
    }

    #[test]
    fn manifest_rejects_script_path_traversal() {
        let temp = tempdir().expect("tempdir");
        let content = r#"---
name: escape
description: Bad script path
runtime: wasm
script: ../escape.wasm
---
# Escape
"#;

        let err = SkillParser::parse_str(content, temp.path()).expect_err("traversal rejected");
        assert!(err.to_string().contains("must not escape"));
    }
}
