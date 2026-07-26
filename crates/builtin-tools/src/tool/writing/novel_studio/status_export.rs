use super::*;

impl NovelStudioTool {
    pub(in crate::tool::writing::novel_studio) async fn update_truth(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        ensure_text_size(&args.content, "content")?;
        if args.section.trim().is_empty() {
            anyhow::bail!("section is required for update_truth");
        }
        if args.content.trim().is_empty() {
            anyhow::bail!("content is required for update_truth");
        }
        if !args.administrative_override {
            anyhow::bail!(
                "update_truth is an administrative override; administrative_override=true is required"
            );
        }
        let reason = first_non_empty(&[args.notes.as_str(), args.revision_notes.as_str()]).trim();
        if reason.is_empty() {
            anyhow::bail!("update_truth administrative override requires a provenance reason");
        }
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let latest_approved = manifest
            .chapters
            .iter()
            .filter(|chapter| chapter_is_approved(chapter))
            .map(|chapter| chapter.number)
            .max()
            .unwrap_or(0);
        let truth_cutoff = latest_approved;
        if args
            .chapter_number
            .is_some_and(|requested| requested != latest_approved)
        {
            anyhow::bail!(
                "administrative truth override must use the latest approved cutoff {latest_approved}; migrate approved history explicitly first"
            );
        }
        let section = args.section.trim().to_string();
        let path = format!("truth/{}.md", slugify(&section));
        let content = normalize_truth_section_content(&section, &args.content, &manifest.language);
        tokio::fs::create_dir_all(project_dir.join("truth")).await?;
        atomic_write_file(
            project_dir.join(&path),
            render_truth_file(&section, &content),
        )
        .await?;
        let record = TruthFileRecord {
            section,
            path: path.clone(),
            unit_count: count_units(&content, &manifest.language),
            updated_at: now_iso(),
        };
        upsert_truth_record(&mut manifest, record.clone());
        let stale_numbers = manifest
            .chapters
            .iter()
            .filter(|chapter| chapter.number > truth_cutoff && !chapter_is_approved(chapter))
            .map(|chapter| chapter.number)
            .collect::<BTreeSet<_>>();
        project_config::invalidate_unapproved_authority_descendants(
            &project_dir,
            &mut manifest,
            truth_cutoff,
        )
        .await?;
        let provenance_path = project_dir.join(format!(
            "truth/overrides/{}-{}.json",
            safe_timestamp(&now_iso()),
            uuid::Uuid::new_v4().simple()
        ));
        atomic_write_file(
            provenance_path.clone(),
            serde_json::to_string_pretty(&json!({
                "operation": "administrative_truth_override",
                "actor": self.agent_id,
                "section": record.section,
                "reason": reason,
                "truth_cutoff_chapter": truth_cutoff,
                "stale_chapters": stale_numbers,
                "content_fingerprint": governance::authority_fingerprint(&content),
                "created_at": now_iso()
            }))?,
        )
        .await?;
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "truth_file": record,
            "provenance_path": provenance_path.to_string_lossy(),
            "truth_cutoff_chapter": truth_cutoff,
            "stale_chapters": stale_numbers,
            "state": project_state_summary(&manifest),
            "audit": audit_manifest(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn read_truth(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        if args.section.trim().is_empty() {
            return Ok(json!({
                "success": true,
                "project_path": project_dir.to_string_lossy(),
                "truth_files": manifest.truth_files,
                "read_only": true,
                "next_actions": [
                    {
                        "action": "revise_chapter",
                        "requires": ["project_path", "chapter_number", "content or metadata fields"],
                        "metadata_fields": ["summary", "key_facts", "continuity_updates", "chapter_title", "status", "revision_notes", "feedback"],
                        "runtime_effect": "artifact.written"
                    }
                ],
                "next_step_hint": "If the user asked to revise, complete, update, or save a chapter, call revise_chapter next. Reading the truth ledger alone is not completion."
            }));
        }
        let section = args.section.trim();
        let truth = manifest
            .truth_files
            .iter()
            .find(|truth| truth.section.eq_ignore_ascii_case(section))
            .ok_or_else(|| anyhow::anyhow!("truth section '{section}' not found"))?;
        let raw = tokio::fs::read_to_string(project_dir.join(&truth.path)).await?;
        let content = truth_file_body(&truth.section, &raw);
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "truth_file": truth,
            "content": content,
            "read_only": true,
            "next_actions": [
                {
                    "action": "revise_chapter",
                    "requires": ["project_path", "chapter_number", "content or metadata fields"],
                    "metadata_fields": ["summary", "key_facts", "continuity_updates", "chapter_title", "status", "revision_notes", "feedback"],
                    "runtime_effect": "artifact.written"
                }
            ],
            "next_step_hint": "If the user asked to revise, complete, update, or save a chapter, call revise_chapter next. Reading the truth ledger alone is not completion."
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn snapshot(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let id = if args.snapshot_id.trim().is_empty() {
            format!("snapshot-{}", safe_timestamp(&now_iso()))
        } else {
            slugify(&args.snapshot_id)
        };
        let path = format!("snapshots/{id}");
        let target = project_dir.join(&path);
        if target.exists() && !args.overwrite {
            anyhow::bail!("snapshot already exists: {}", target.display());
        }
        snapshot::copy_project_state_for_snapshot(&project_dir, &target).await?;
        let snapshot = SnapshotRecord {
            id,
            path,
            reason: first_non_empty(&[args.notes.as_str(), args.revision_notes.as_str()])
                .to_string(),
            created_at: now_iso(),
        };
        manifest.snapshots.retain(|item| item.id != snapshot.id);
        manifest.snapshots.push(snapshot.clone());
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "snapshot": snapshot,
            "state": project_state_summary(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn restore_snapshot(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        if args.snapshot_id.trim().is_empty() {
            anyhow::bail!("snapshot_id is required for restore_snapshot");
        }
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let snapshot = manifest
            .snapshots
            .iter()
            .find(|snapshot| snapshot.id == slugify(&args.snapshot_id))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("snapshot '{}' not found", args.snapshot_id.trim()))?;
        let source = project_dir.join(&snapshot.path);
        if !source.exists() {
            anyhow::bail!("snapshot directory is missing: {}", source.display());
        }
        snapshot::copy_project_state_from_snapshot(&source, &project_dir).await?;
        let mut restored = self.read_manifest(&project_dir).await?;
        for retained in manifest.snapshots {
            if project_dir.join(&retained.path).exists()
                && restored.snapshots.iter().all(|item| item.id != retained.id)
            {
                restored.snapshots.push(retained);
            }
        }
        restored
            .snapshots
            .sort_by(|left, right| left.id.cmp(&right.id));
        restored.updated_at = now_iso();
        self.write_manifest(&project_dir, &restored).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "restored_snapshot": snapshot,
            "state": project_state_summary(&restored),
            "audit": audit_manifest(&restored)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn analytics(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let durable_progress = durable_chapter_progress(&project_dir, &manifest).await;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "state": apply_durable_chapter_progress(
                project_state_summary(&manifest),
                &manifest,
                &durable_progress,
            ),
            "analytics": analytics_report(&manifest),
            "audit": apply_durable_progress_to_audit(
                audit_manifest(&manifest),
                &manifest,
                &durable_progress,
            )
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn audit(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let durable_progress = durable_chapter_progress(&project_dir, &manifest).await;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "state": apply_durable_chapter_progress(
                project_state_summary(&manifest),
                &manifest,
                &durable_progress,
            ),
            "audit": apply_durable_progress_to_audit(
                audit_manifest(&manifest),
                &manifest,
                &durable_progress,
            )
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn status(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let durable_progress = durable_chapter_progress(&project_dir, &manifest).await;
        let identity_integrity_blockers = Vec::<serde_json::Value>::new();
        let state = apply_durable_chapter_progress(
            project_state_summary_light(&manifest),
            &manifest,
            &durable_progress,
        );
        let mut status = json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "state": state,
            "audit": apply_durable_progress_to_audit(
                light_status_audit_manifest(&manifest),
                &manifest,
                &durable_progress,
            ),
            "identity_integrity_blockers": identity_integrity_blockers,
            "integrity_scan": {
                "mode": "light_status",
                "full_scan_skipped": true,
                "full_scan_actions": ["audit", "repair_project_state"],
                "reason": "status is used on hot paths such as progress refresh and export completion; full approved-chapter body scans are reserved for explicit audit/repair so long projects can finish predictably."
            }
        });
        if args.include_draft {
            status["draft"] = novel_draft_summary(&novel_creation_draft_from_manifest(&manifest));
        }
        Ok(status)
    }

    pub(in crate::tool::writing::novel_studio) async fn export(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let approved_only = args.approved_only || manifest.approved_only;
        if !approved_only {
            ensure_export_ready(&manifest)?;
        }
        let format = match args.format.trim() {
            "" | "txt" => "txt",
            "md" => "md",
            other => anyhow::bail!("unsupported export format: {other}"),
        };
        let output_path = if args.output.trim().is_empty() {
            project_dir.join("exports").join(format!(
                "{}.{}",
                slugify(canonical_project_title(&manifest)),
                format
            ))
        } else {
            self.resolve_workspace_path(&args.output)?
        };
        let export_scan = export::write_export_to_path(
            &project_dir,
            &manifest,
            format,
            approved_only,
            &output_path,
        )
        .await?;
        let artifact_registration = self
            .register_export_artifact(&manifest, &output_path, format)
            .await?;
        let format_effect = format!("artifact.{format}");

        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.written",
            "runtime_effects": ["artifact.written", "artifact.exported", format_effect],
            "project_path": project_dir.to_string_lossy(),
            "artifact_path": output_path.to_string_lossy(),
            "output_path": output_path.to_string_lossy(),
            "format": format,
            "approved_only": approved_only,
            "state": project_state_summary(&manifest),
            "export_scan": export_scan,
            "artifact_registration": artifact_registration
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn sync_readable_txt_export(
        &self,
        project_dir: &Path,
        manifest: &NovelProjectManifest,
    ) -> anyhow::Result<export::ReadableTxtExport> {
        export::sync_readable_txt_export(project_dir, manifest).await
    }

    pub(in crate::tool::writing::novel_studio) async fn register_export_artifact(
        &self,
        manifest: &NovelProjectManifest,
        output_path: &Path,
        format: &str,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        let Some(manager) = self.artifact_manager.as_deref() else {
            return Ok(None);
        };
        let mut metadata = HashMap::new();
        metadata.insert(
            "title".to_string(),
            canonical_project_title(manifest).to_string(),
        );
        metadata.insert("format".to_string(), format.to_string());
        metadata.insert("chapters".to_string(), manifest.chapters.len().to_string());
        metadata.insert(
            "approved_chapters".to_string(),
            manifest
                .chapters
                .iter()
                .filter(|chapter| chapter_is_approved(chapter))
                .count()
                .to_string(),
        );
        metadata.insert(
            "units".to_string(),
            manifest
                .chapters
                .iter()
                .map(|chapter| chapter.unit_count)
                .sum::<usize>()
                .to_string(),
        );
        let record = register_tool_output_artifact(
            manager,
            &self.agent_id,
            "novel_studio",
            &output_path.to_string_lossy(),
            ArtifactLifecycle::Session,
            "novel_export",
            metadata,
        )
        .await?;
        Ok(Some(
            ToolArtifactRegistration::from_record(&record).as_json(),
        ))
    }
}
