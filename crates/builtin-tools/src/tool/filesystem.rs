//! Filesystem tools (Reader, Writer, Editor, Lister)
//!
//! Provides robust file operations with strict workspace sandboxing.
//! Ported and adapted for BenShu.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::debug;

use super::{Tool, ToolDefinition};

const MAX_TEXT_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// Helper function to validate that a path stays within the workspace root.
/// Returns the absolute path if valid, or an error if outside.
fn validate_path(workspace: &Path, relative_path: &str) -> anyhow::Result<PathBuf> {
    // 1. Resolve relative path
    let full_path =
        if relative_path.starts_with('/') || (cfg!(windows) && relative_path.contains(':')) {
            // If absolute, key check is it must start with one of the authorized workspaces
            PathBuf::from(relative_path)
        } else {
            workspace.join(relative_path)
        };

    // 2. Normalize components to prevent traversal
    let path_to_check = if full_path.exists() {
        full_path.canonicalize()?
    } else if let Some(parent) = full_path.parent() {
        if parent.exists() {
            let canon_parent = parent.canonicalize()?;
            let fname = full_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Invalid path: no file name component"))?;
            canon_parent.join(fname)
        } else {
            full_path
        }
    } else {
        full_path
    };

    // 3. Check against primary workspace
    let workspace_canon = if workspace.exists() {
        workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf())
    } else {
        workspace.to_path_buf()
    };

    if path_to_check.starts_with(&workspace_canon) {
        return Ok(path_to_check);
    }

    // 4. Check against dynamic trusted workspaces (Roadmap Phase 7.1)
    if let Ok(trusted) = benshu_brain::skills::CURRENT_WORKSPACES.try_with(|w| w.clone()) {
        for root in trusted {
            let root_canon = if root.exists() {
                root.canonicalize().unwrap_or_else(|_| root.clone())
            } else {
                root.clone()
            };
            if path_to_check.starts_with(&root_canon) {
                return Ok(path_to_check);
            }
        }
    }

    anyhow::bail!("Access Denied: Path '{}' is outside authorized workspaces. You can add trusted paths in the UI settings.", relative_path)
}

async fn ensure_small_text_file(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::metadata(path).await?;
    if metadata.len() > MAX_TEXT_FILE_BYTES {
        anyhow::bail!(
            "File is larger than the 20MB text-tool safety limit: {} bytes",
            metadata.len()
        );
    }
    Ok(())
}

async fn reject_near_miss_parent_directory(path: &Path) -> anyhow::Result<()> {
    reject_ambiguous_existing_near_miss_component(path).await?;
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.exists() {
        return Ok(());
    }
    let Some((existing_parent, missing_name)) = first_missing_parent_component(path) else {
        return Ok(());
    };
    let mut entries = fs::read_dir(&existing_parent).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name == missing_name {
            continue;
        }
        if looks_like_near_miss_path_component(&missing_name, &file_name) {
            anyhow::bail!(
                "Refusing to create new directory '{}' because existing sibling '{}' looks like a likely path typo under '{}'. Reuse the existing path or choose a clearly different directory name.",
                missing_name,
                file_name,
                existing_parent.display()
            );
        }
    }
    Ok(())
}

async fn reject_ambiguous_existing_near_miss_component(path: &Path) -> anyhow::Result<()> {
    let components = path
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect::<Vec<_>>();
    if components.len() < 3 {
        return Ok(());
    }
    let mut current = PathBuf::new();
    for idx in 0..components.len().saturating_sub(2) {
        current.push(&components[idx]);
        if !current.exists() {
            continue;
        }
        let candidate = PathBuf::from_iter(components.iter().take(idx + 2));
        if !candidate.exists() {
            continue;
        }
        let current_name = components[idx + 1].to_string_lossy().to_string();
        let next_component = &components[idx + 2];
        let mut entries = fs::read_dir(&current).await?;
        while let Some(entry) = entries.next_entry().await? {
            let sibling_name = entry.file_name().to_string_lossy().to_string();
            if sibling_name == current_name
                || !looks_like_near_miss_path_component(&current_name, &sibling_name)
            {
                continue;
            }
            let sibling_next = current.join(&sibling_name).join(next_component);
            if sibling_next.exists() {
                anyhow::bail!(
                    "Refusing ambiguous path component '{}' because existing sibling '{}' has the same next path component '{}' under '{}'. Reuse the intended existing path explicitly.",
                    current_name,
                    sibling_name,
                    next_component.to_string_lossy(),
                    current.display()
                );
            }
        }
    }
    Ok(())
}

fn first_missing_parent_component(path: &Path) -> Option<(PathBuf, String)> {
    let parent = path.parent()?;
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component.as_os_str());
        if current.exists() {
            continue;
        }
        let existing_parent = current.parent()?.to_path_buf();
        if !existing_parent.exists() {
            return None;
        }
        let missing_name = current.file_name()?.to_string_lossy().to_string();
        return Some((existing_parent, missing_name));
    }
    None
}

fn looks_like_near_miss_path_component(left: &str, right: &str) -> bool {
    let left = left.trim().to_ascii_lowercase();
    let right = right.trim().to_ascii_lowercase();
    if left.starts_with('.') || right.starts_with('.') {
        return false;
    }
    if left.len() < 4 || right.len() < 4 {
        return false;
    }
    let len_delta = left.len().abs_diff(right.len());
    len_delta <= 1 && levenshtein_at_most_one(&left, &right)
}

fn levenshtein_at_most_one(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left == right {
        return true;
    }
    if left.len().abs_diff(right.len()) > 1 {
        return false;
    }
    let mut i = 0;
    let mut j = 0;
    let mut edits = 0;
    while i < left.len() && j < right.len() {
        if left[i] == right[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        match left.len().cmp(&right.len()) {
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Greater => i += 1,
            std::cmp::Ordering::Less => j += 1,
        }
    }
    edits + usize::from(i < left.len() || j < right.len()) <= 1
}

// ─── 1. ReadFileTool ─────────────────────────────────────────────────────────

pub struct ReadFileTool {
    workspace: PathBuf,
}

impl ReadFileTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> String {
        "read_file".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the full content of a file from the filesystem.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file (relative to workspace root)"
                    }
                },
                "required": ["path"]
            }),
            parameters_ts: Some("interface ReadFile {\n  path: string;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this to read code, configuration, or text files. If you need to search, use `list_dir` first.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
        }
        let args: Args = serde_json::from_str(arguments)?;

        let safe_path = validate_path(&self.workspace, &args.path)?;
        ensure_small_text_file(&safe_path).await?;

        debug!("Reading file: {:?}", safe_path);
        let content = fs::read_to_string(&safe_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read file: {}", e))?;

        Ok(content)
    }
}

// ─── 2. WriteFileTool ────────────────────────────────────────────────────────

pub struct WriteFileTool {
    workspace: PathBuf,
}

impl WriteFileTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> String {
        "write_file".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write content to a file. Overwrites existing files. Creates parent directories if needed.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file"
                    },
                    "content": {
                        "type": "string",
                        "description": "Full content to write"
                    }
                },
                "required": ["path", "content"]
            }),
            parameters_ts: Some("interface WriteFile {\n  path: string;\n  content: string;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use to create new files or overwrite existing ones. careful with overwrites.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            content: String,
        }
        let args: Args = serde_json::from_str(arguments)?;

        if args.content.len() as u64 > MAX_TEXT_FILE_BYTES {
            anyhow::bail!("content is larger than the 20MB text-tool safety limit");
        }
        if content_is_bare_tool_invocation(&args.content) {
            anyhow::bail!(
                "content looks like a bare tool invocation, not file content; call that tool directly and write only the resulting material"
            );
        }

        let safe_path = validate_path(&self.workspace, &args.path)?;
        reject_near_miss_parent_directory(&safe_path).await?;

        if let Some(parent) = safe_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await?;
            }
        }

        fs::write(&safe_path, &args.content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write file: {}", e))?;

        Ok(format!(
            "runtime_effect: artifact.written\npath: {}\nbytes: {}\n\nSuccessfully wrote {} bytes to {}",
            args.path,
            args.content.len(),
            args.content.len(),
            args.path
        ))
    }

    async fn pre_call(&self, arguments: &str) -> anyhow::Result<()> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
        }
        let args: Args =
            serde_json::from_str(arguments).map_err(|e| anyhow::anyhow!("Invalid args: {}", e))?;
        let safe_path = validate_path(&self.workspace, &args.path)?;

        // Roadmap Phase 6.2: Automatic Backup before modification
        let bak = benshu_security::internal_backup::ShadowBak::new();
        if let Some(backup_path) = bak.backup(&safe_path).await? {
            // Signal to the agent that a backup was created
            if let Ok(current_backup) = benshu_brain::skills::CURRENT_BACKUP.try_with(|b| b.clone())
            {
                let mut lock = current_backup.lock();
                *lock = Some(benshu_brain::skills::BackupInfo {
                    original_path: safe_path.to_string_lossy().to_string(),
                    backup_path: backup_path.to_string_lossy().to_string(),
                });
            }
        }
        Ok(())
    }
}

// ─── 3. ListDirTool ──────────────────────────────────────────────────────────

pub struct ListDirTool {
    workspace: PathBuf,
}

impl ListDirTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> String {
        "list_dir".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_dir".to_string(),
            description: "List files and directories in a given path.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path (use '.' for root)"
                    }
                },
                "required": ["path"]
            }),
            parameters_ts: Some("interface ListDir {\n  path: string;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
        }
        let args: Args = serde_json::from_str(arguments)?;

        let safe_path = validate_path(&self.workspace, &args.path)?;

        let mut entries = fs::read_dir(&safe_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read directory: {}", e))?;

        let mut items = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let meta = entry.metadata().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            let suffix = if meta.is_dir() { "/" } else { "" };
            items.push(format!("{}{}", name, suffix));
        }

        items.sort();

        if items.is_empty() {
            Ok("(empty directory)".to_string())
        } else {
            Ok(items.join("\n"))
        }
    }
}

// ─── 4. EditFileTool ─────────────────────────────────────────────────────────

pub struct EditFileTool {
    workspace: PathBuf,
}

impl EditFileTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> String {
        "edit_file".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "edit_file".to_string(),
            description: "Edit a file by replacing a specific block of text.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Exact text block to replace"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "New content to insert"
                    }
                },
                "required": ["path", "old_text", "new_text"]
            }),
            parameters_ts: Some("interface EditFile {\n  path: string;\n  old_text: string;\n  new_text: string;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use for small edits. Provide enough context in `old_text` to be unique.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            old_text: String,
            new_text: String,
        }
        let args: Args = serde_json::from_str(arguments)?;

        let safe_path = validate_path(&self.workspace, &args.path)?;
        ensure_small_text_file(&safe_path).await?;
        let content = fs::read_to_string(&safe_path).await?;

        // Normalize line endings? ZeptoClaw doesn't, maybe we should?
        // Let's stick to strict replacement first.

        if !content.contains(&args.old_text) {
            // Fallback: Try with trimmed whitespace if exact match fails?
            // We are strict for safety.
            anyhow::bail!(
                "Text to replace not found in file. Ensure exact match including whitespace."
            );
        }

        let new_content = content.replace(&args.old_text, &args.new_text);
        if new_content.len() as u64 > MAX_TEXT_FILE_BYTES {
            anyhow::bail!("edited file would exceed the 20MB text-tool safety limit");
        }

        fs::write(&safe_path, &new_content).await?;

        Ok(format!("Successfully modified {}", args.path))
    }

    async fn pre_call(&self, arguments: &str) -> anyhow::Result<()> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
        }
        let args: Args =
            serde_json::from_str(arguments).map_err(|e| anyhow::anyhow!("Invalid args: {}", e))?;
        let safe_path = validate_path(&self.workspace, &args.path)?;

        // Roadmap Phase 6.2: Automatic Backup before modification
        let bak = benshu_security::internal_backup::ShadowBak::new();
        if let Some(backup_path) = bak.backup(&safe_path).await? {
            // Signal to the agent that a backup was created
            if let Ok(current_backup) = benshu_brain::skills::CURRENT_BACKUP.try_with(|b| b.clone())
            {
                let mut lock = current_backup.lock();
                *lock = Some(benshu_brain::skills::BackupInfo {
                    original_path: safe_path.to_string_lossy().to_string(),
                    backup_path: backup_path.to_string_lossy().to_string(),
                });
            }
        }
        Ok(())
    }
}

fn content_is_bare_tool_invocation(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty()
        || trimmed.len() > 512
        || trimmed
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
            != 1
    {
        return false;
    }
    let Some((name, rest)) = trimmed.split_once('(') else {
        return false;
    };
    let name = name.trim();
    let rest = rest.trim();
    if !rest.ends_with(')') || !tool_like_identifier(name) {
        return false;
    }
    matches!(
        name,
        "read_file"
            | "write_file"
            | "edit_file"
            | "list_dir"
            | "fetch_document"
            | "knowledge_search"
            | "tiered_search"
            | "knowledge_import_url"
            | "knowledge_manage_document"
            | "web_fetch"
            | "web_search"
            | "browser_browse"
            | "novel_studio"
            | "writing_studio"
    )
}

fn tool_like_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn write_file_rejects_near_miss_parent_directory_typo() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("generated/novels"))
            .await
            .expect("create existing sibling");
        let tool = WriteFileTool::new(dir.path().to_path_buf());

        let result = tool
            .call(r#"{"path":"generated/novals/project/chapter.md","content":"chapter body"}"#)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("likely path typo"));
        assert!(!dir.path().join("generated/novals").exists());
    }

    #[tokio::test]
    async fn write_file_allows_clearly_distinct_new_parent_directory() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("generated/novels"))
            .await
            .expect("create existing sibling");
        let tool = WriteFileTool::new(dir.path().to_path_buf());

        let result = tool
            .call(r#"{"path":"generated/reports/project.md","content":"report body"}"#)
            .await;

        assert!(result.is_ok(), "{result:?}");
        assert!(dir.path().join("generated/reports/project.md").exists());
    }

    #[tokio::test]
    async fn write_file_rejects_existing_ambiguous_near_miss_parent() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("generated/novels/project"))
            .await
            .expect("create intended project");
        fs::create_dir_all(dir.path().join("generated/novals/project"))
            .await
            .expect("create mistaken project");
        let tool = WriteFileTool::new(dir.path().to_path_buf());

        let result = tool
            .call(r#"{"path":"generated/novals/project/chapter.md","content":"chapter body"}"#)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ambiguous path"));
    }

    #[tokio::test]
    async fn write_file_ignores_hidden_directory_near_miss() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join(".benshu/data"))
            .await
            .expect("create hidden sibling");
        fs::create_dir_all(dir.path().join("benshu/data"))
            .await
            .expect("create normal sibling");
        let tool = WriteFileTool::new(dir.path().to_path_buf());

        let result = tool
            .call(r#"{"path":"benshu/data/out.md","content":"body"}"#)
            .await;

        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn write_file_rejects_bare_tool_invocation_content() {
        let dir = tempdir().expect("tempdir");
        let tool = WriteFileTool::new(dir.path().to_path_buf());

        let result = tool
            .call(
                &serde_json::json!({
                    "path": "data/recovery_notes.txt",
                    "content": "fetch_document(collection=\"references\", path=\"web/example\")"
                })
                .to_string(),
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("call that tool directly"));
        assert!(!dir.path().join("data/recovery_notes.txt").exists());
    }
}
