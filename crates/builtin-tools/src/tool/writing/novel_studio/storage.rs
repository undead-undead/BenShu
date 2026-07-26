use super::*;
use std::io::Write;

pub(super) async fn atomic_write_file(path: PathBuf, content: String) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            anyhow::anyhow!("atomic write target has no parent: {}", path.display())
        })?;
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(content.as_bytes())?;
        temporary.as_file_mut().sync_all()?;
        temporary
            .persist(&path)
            .map_err(|error| anyhow::anyhow!(error.error))?;
        Ok(())
    })
    .await
    .map_err(|error| anyhow::anyhow!("atomic write worker failed: {error}"))??;
    Ok(())
}

impl NovelStudioTool {
    pub(super) fn require_project_path(&self, args: &NovelStudioArgs) -> anyhow::Result<PathBuf> {
        let requested_project = args.project_path.trim();
        let output_root = self.output_root_for_args(args);

        if !requested_project.is_empty() {
            match self.resolve_workspace_path(requested_project) {
                Ok(resolved) => {
                    if resolved.join("project.json").exists() {
                        return Ok(resolved);
                    }
                }
                Err(resolve_error) => {
                    if let Some(recovered) =
                        self.recover_project_path_by_title(requested_project, output_root.as_ref())?
                    {
                        return Ok(recovered);
                    }
                    return Err(resolve_error);
                }
            }
            if let Some(recovered) =
                self.recover_project_path_by_title(requested_project, output_root.as_ref())?
            {
                return Ok(recovered);
            }
        }

        let output_root_path = self.resolve_workspace_path(output_root.as_ref())?;
        if output_root_path.join("project.json").exists() {
            return Ok(output_root_path);
        }
        if !requested_project.is_empty() {
            let Some(parent_root) = output_root_path.parent() else {
                anyhow::bail!("project_path is required for {}", args.action);
            };
            if let Some(parent_root) = parent_root.to_str() {
                if let Some(recovered) =
                    self.recover_project_path_by_title(requested_project, parent_root)?
                {
                    return Ok(recovered);
                }
            }
            return self.resolve_workspace_path(requested_project);
        }
        anyhow::bail!("project_path is required for {}", args.action)
    }

    pub(super) async fn ensure_project_scaffold(&self, project_dir: &Path) -> anyhow::Result<()> {
        for dir in [
            "sources",
            "chapters",
            "exports",
            "truth",
            "plans",
            "reviews",
            "runtime",
            "snapshots",
            "archives",
        ] {
            tokio::fs::create_dir_all(project_dir.join(dir)).await?;
        }
        Ok(())
    }

    pub(super) fn resolve_workspace_path(&self, path: &str) -> anyhow::Result<PathBuf> {
        let raw = path.trim();
        if raw.is_empty() {
            anyhow::bail!("path is empty");
        }
        let normalized = self.normalize_storage_relative_path(raw);
        let normalized = normalized.as_ref();
        let joined = if Path::new(normalized).is_absolute() {
            PathBuf::from(normalized)
        } else {
            self.workspace.join(normalized)
        };
        reject_parent_components(&joined)?;
        let workspace = canonical_or_self(&self.workspace);
        let candidate = canonical_parent_join(&joined)?;
        if candidate.starts_with(&workspace) {
            return Ok(candidate);
        }
        if let Ok(trusted) = benshu_brain::skills::CURRENT_WORKSPACES.try_with(|w| w.clone()) {
            for root in trusted {
                let root = canonical_or_self(&root);
                if candidate.starts_with(root) {
                    return Ok(candidate);
                }
            }
        }
        anyhow::bail!(
            "Access Denied: path '{}' is outside authorized workspaces",
            path
        )
    }

    pub(super) fn default_output_root(&self) -> &'static str {
        if self
            .workspace
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("data"))
        {
            "generated/novels"
        } else {
            "data/generated/novels"
        }
    }

    pub(super) fn output_root_for_args<'a>(&self, args: &'a NovelStudioArgs) -> Cow<'a, str> {
        if args.output_root.trim().is_empty() {
            Cow::Borrowed(self.default_output_root())
        } else {
            self.normalize_storage_relative_path(args.output_root.trim())
        }
    }

    pub(super) fn normalize_storage_relative_path<'a>(&self, path: &'a str) -> Cow<'a, str> {
        if Path::new(path).is_absolute()
            || !self
                .workspace
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("data"))
        {
            return Cow::Borrowed(path);
        }
        let normalized = path.replace('\\', "/");
        if let Some(stripped) = normalized.strip_prefix("data/") {
            Cow::Owned(stripped.to_string())
        } else {
            Cow::Borrowed(path)
        }
    }

    pub(super) async fn read_manifest(
        &self,
        project_dir: &Path,
    ) -> anyhow::Result<NovelProjectManifest> {
        let raw = tokio::fs::read_to_string(project_dir.join("project.json")).await?;
        let mut manifest: NovelProjectManifest = serde_json::from_str(&raw)?;
        if manifest.schema_version != SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported novel project schema '{}'",
                manifest.schema_version
            );
        }
        ensure_project_governance(&mut manifest);
        Ok(manifest)
    }

    pub(super) async fn alternative_projects_with_chapter(
        &self,
        args: &NovelStudioArgs,
        selected_project: &Path,
        chapter_number: usize,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let output_root = self.output_root_for_args(args);
        let root = self.resolve_workspace_path(output_root.as_ref())?;
        let mut entries = match tokio::fs::read_dir(root).await {
            Ok(entries) => entries,
            Err(_) => return Ok(Vec::new()),
        };
        let selected_project = canonical_or_self(selected_project);
        let mut alternatives = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if canonical_or_self(&path) == selected_project || !path.join("project.json").exists() {
                continue;
            }
            let Ok(manifest) = self.read_manifest(&path).await else {
                continue;
            };
            let Some(chapter) = manifest
                .chapters
                .iter()
                .find(|chapter| chapter.number == chapter_number)
            else {
                continue;
            };
            alternatives.push((
                manifest.updated_at.clone(),
                json!({
                    "path": path.to_string_lossy(),
                    "title": manifest.title.clone(),
                    "updated_at": manifest.updated_at.clone(),
                    "chapter": {
                        "number": chapter.number,
                        "title": chapter.title.clone(),
                        "status": chapter.status.clone()
                    },
                    "state": project_state_summary(&manifest)
                }),
            ));
        }
        alternatives.sort_by(|left, right| right.0.cmp(&left.0));
        Ok(alternatives
            .into_iter()
            .take(5)
            .map(|(_, value)| value)
            .collect())
    }

    pub(super) fn recover_project_path_by_title(
        &self,
        requested: &str,
        output_root: &str,
    ) -> anyhow::Result<Option<PathBuf>> {
        let normalized_requested = requested.replace('\\', "/");
        let requested_name = normalized_requested
            .rsplit('/')
            .find(|segment| !segment.trim().is_empty())
            .unwrap_or(normalized_requested.as_str());
        let requested_key = normalize_project_lookup_key(requested_name);
        if requested_key.is_empty() {
            return Ok(None);
        }
        let root = self.resolve_workspace_path(output_root)?;
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => return Ok(None),
        };
        let mut exact_matches = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.join("project.json").is_file() {
                continue;
            }
            let folder_key = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(normalize_project_lookup_key)
                .unwrap_or_default();
            let manifest_key = std::fs::read_to_string(path.join("project.json"))
                .ok()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                .and_then(|value| {
                    value
                        .get("title")
                        .and_then(|title| title.as_str())
                        .map(normalize_project_lookup_key)
                })
                .unwrap_or_default();
            if folder_key == requested_key || manifest_key == requested_key {
                exact_matches.push(path);
            }
        }
        exact_matches.sort();
        exact_matches.dedup();
        match exact_matches.len() {
            0 => Ok(None),
            1 => Ok(exact_matches.pop()),
            _ => anyhow::bail!(
                "project title '{}' is ambiguous; matching projects: {}",
                requested,
                exact_matches
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    pub(super) async fn write_manifest(
        &self,
        project_dir: &Path,
        manifest: &NovelProjectManifest,
    ) -> anyhow::Result<()> {
        let mut manifest = manifest.clone();
        ensure_project_governance(&mut manifest);
        let raw = serde_json::to_string_pretty(&manifest)?;
        write_story_bible_artifacts(project_dir, &manifest).await?;
        atomic_write_file(project_dir.join("project.json"), raw).await?;
        Ok(())
    }

    pub(super) async fn new_draft_path(
        &self,
        args: &NovelStudioArgs,
        title: &str,
    ) -> anyhow::Result<PathBuf> {
        let root = self
            .resolve_workspace_path(self.output_root_for_args(args).as_ref())?
            .join("drafts");
        tokio::fs::create_dir_all(&root).await?;
        Ok(root.join(format!(
            "{}-{}.json",
            slugify(title),
            uuid::Uuid::new_v4().simple()
        )))
    }

    pub(super) fn require_draft_path(&self, args: &NovelStudioArgs) -> anyhow::Result<PathBuf> {
        if args.draft_path.trim().is_empty() {
            anyhow::bail!("draft_path is required for {}", args.action);
        }
        self.resolve_workspace_path(&args.draft_path)
    }

    pub(super) async fn read_draft_file(&self, path: &Path) -> anyhow::Result<NovelCreationDraft> {
        let raw = tokio::fs::read_to_string(path).await?;
        let draft: NovelCreationDraft = serde_json::from_str(&raw)?;
        if draft.schema_version != "benshu.novel_creation_draft.v1" {
            anyhow::bail!(
                "draft schema mismatch: expected benshu.novel_creation_draft.v1, found {}",
                draft.schema_version
            );
        }
        Ok(draft)
    }

    pub(super) async fn write_draft_file(
        &self,
        path: &Path,
        draft: &NovelCreationDraft,
    ) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        atomic_write_file(path.to_path_buf(), serde_json::to_string_pretty(draft)?).await?;
        Ok(())
    }
}
