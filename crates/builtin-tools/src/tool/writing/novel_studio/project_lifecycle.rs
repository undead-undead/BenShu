use super::*;

impl NovelStudioTool {
    pub(super) async fn list_projects(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let output_root = self.output_root_for_args(args);
        let root = self.resolve_workspace_path(output_root.as_ref())?;
        let mut projects = Vec::new();
        let mut entries = match tokio::fs::read_dir(&root).await {
            Ok(entries) => entries,
            Err(_) => {
                return Ok(json!({
                    "success": true,
                    "root": root.to_string_lossy(),
                    "projects": projects
                }));
            }
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if project_directory_is_internal_staging(&path) {
                continue;
            }
            let manifest_path = path.join("project.json");
            if !manifest_path.exists() {
                continue;
            }
            if let Ok(manifest) = self.read_manifest(&path).await {
                projects.push((
                    manifest.updated_at.clone(),
                    json!({
                    "path": path.to_string_lossy(),
                    "state": project_state_summary(&manifest)
                    }),
                ));
            }
        }
        projects.sort_by(|left, right| right.0.cmp(&left.0));
        let projects = projects
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        Ok(json!({
            "success": true,
            "root": root.to_string_lossy(),
            "projects": projects
        }))
    }

    pub(super) async fn draft_project(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let now = now_iso();
        let language = normalize_language(&args.language);
        let inferred_title = novel_draft_title_from_args(args, &language);
        let structured_contract_v2 = contract_v2_from_args(args);
        let mut draft = NovelCreationDraft {
            schema_version: "benshu.novel_creation_draft.v1".to_string(),
            title: inferred_title,
            language,
            genre: args.genre.trim().to_string(),
            brief: args.brief.trim().to_string(),
            target_units: args.target_units,
            chapter_unit_target: longform_policy::normalize_chapter_unit_target(
                args.chapter_unit_target,
                args.target_units,
            )
            .or_else(|| inferred_chapter_unit_target(args.target_units)),
            max_chapters_per_turn: args.max_chapters_per_turn.filter(|value| *value > 0),
            export_format: export::normalize_export_format(args.format.trim()),
            export_when_complete: args.export_when_complete,
            approved_only: args.approved_only,
            premise: args.premise.trim().to_string(),
            ending_direction: args.ending_direction.trim().to_string(),
            authority_contract: args.authority_contract.clone(),
            protagonist_arc: args.protagonist_arc.trim().to_string(),
            world_imagery: args.world_imagery.trim().to_string(),
            main_causal_spine: args.main_causal_spine.trim().to_string(),
            title_rationale: args.title_rationale.trim().to_string(),
            themes: clean_list(&args.themes),
            characters: clean_list(&args.characters),
            world_rules: clean_list(&args.world_rules),
            style_rules: clean_list(&args.style_rules),
            must_avoid: clean_list(&args.must_avoid),
            outline: args.outline.trim().to_string(),
            structured_contract_v2,
            created_at: now.clone(),
            updated_at: now,
        };
        govern_novel_creation_draft_authority(&mut draft);
        let draft_path = if args.draft_path.trim().is_empty() {
            self.new_draft_path(args, &draft.title).await?
        } else {
            self.resolve_workspace_path(&args.draft_path)?
        };
        self.write_draft_file(&draft_path, &draft).await?;
        Ok(json!({
            "success": true,
            "runtime_effect": "contract.drafted",
            "draft_path": draft_path.to_string_lossy(),
            "draft": novel_draft_summary(&draft),
            "next_action": "approve_draft or update_draft",
            "receipt": {
                "kind": "writing_creation_draft",
                "tool": "novel_studio",
                "commits_to": ["init_project", "set_contract", "run_project", "export"]
            }
        }))
    }

    pub(super) async fn update_draft(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let draft_path = self.require_draft_path(args)?;
        let mut draft = self.read_draft_file(&draft_path).await?;
        let before = novel_draft_summary(&draft);
        apply_novel_draft_updates(&mut draft, args);
        govern_novel_creation_draft_authority(&mut draft);
        draft.updated_at = now_iso();
        self.write_draft_file(&draft_path, &draft).await?;
        Ok(json!({
            "success": true,
            "runtime_effect": "contract.updated",
            "draft_path": draft_path.to_string_lossy(),
            "before": before,
            "draft": novel_draft_summary(&draft),
            "next_action": "approve_draft or update_draft"
        }))
    }

    pub(super) async fn show_draft(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let draft_path = self.require_draft_path(args)?;
        let draft = self.read_draft_file(&draft_path).await?;
        Ok(json!({
            "success": true,
            "read_only": true,
            "draft_path": draft_path.to_string_lossy(),
            "draft": novel_draft_summary(&draft),
            "next_action": "approve_draft or update_draft"
        }))
    }

    pub(super) async fn approve_draft(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let draft_path = self.require_draft_path(args)?;
        let draft = self.read_draft_file(&draft_path).await?;
        let expected_character_authority = character_authority_fingerprint(&draft.characters);
        let readiness_issues = novel_draft_readiness_issues(&draft);
        if !readiness_issues.is_empty() {
            return Ok(json!({
                "success": false,
                "error_kind": "draft_requires_contract_revision",
                "error": "novel draft is not ready for approval",
                "draft_path": draft_path.to_string_lossy(),
                "issues": readiness_issues,
                "next_action": "update_draft"
            }));
        }
        let mut init_args = args.clone();
        init_args.action = "init_project".to_string();
        init_args.title = draft.title.clone();
        init_args.language = draft.language.clone();
        init_args.genre = draft.genre.clone();
        init_args.brief = draft.brief.clone();
        init_args.target_units = draft.target_units;
        init_args.chapter_unit_target = draft.chapter_unit_target;
        init_args.max_chapters_per_turn = draft.max_chapters_per_turn;
        init_args.format = draft.export_format.clone();
        init_args.export_when_complete = draft.export_when_complete;
        init_args.approved_only = draft.approved_only;
        init_args.allow_title_conflict = true;
        apply_contract_v2_to_args(&mut init_args, &draft.structured_contract_v2);
        if project_path_points_to_draft_file(&init_args.project_path, &draft_path) {
            init_args.project_path.clear();
        }
        let requested_project_path = if init_args.project_path.trim().is_empty() {
            None
        } else {
            Some(self.resolve_workspace_path(&init_args.project_path)?)
        };
        let output_root = self.output_root_for_args(&init_args);
        let output_root = self.resolve_workspace_path(output_root.as_ref())?;
        let approval_parent = requested_project_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| output_root.clone());
        let approval_staging_path =
            approval_parent.join(format!(".novel-approve-{}", uuid::Uuid::new_v4().simple()));
        init_args.project_path = approval_staging_path.to_string_lossy().to_string();
        init_args.overwrite = false;
        let mut init = self.init_project(&init_args).await?;
        let recovered_title_conflict: Option<String> = None;
        if init_project_title_conflicted(&init) && args.title.trim().is_empty() {
            init["error_kind"] = json!("title_requires_contract_revision");
            init["next_step_hint"] = json!(
                "The generated title conflicts with an existing project. Ask the LLM to revise the contract title from the ending direction, protagonist arc, world imagery, and causal spine; do not let the tool invent a formal replacement title."
            );
            init["draft_path"] = json!(draft_path.to_string_lossy().to_string());
        }
        if !init
            .get("success")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return Ok(init);
        }
        let project_path = init
            .get("project_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("init_project did not return project_path"))?
            .to_string();
        let staged_project_path = PathBuf::from(&project_path);

        let mut contract_args = init_args.clone();
        contract_args.action = "set_contract".to_string();
        contract_args.project_path = project_path.clone();
        contract_args.premise = draft_premise_with_naming_basis(&draft);
        contract_args.themes = draft.themes.clone();
        contract_args.characters = draft.characters.clone();
        contract_args.world_rules = draft.world_rules.clone();
        contract_args.style_rules = draft.style_rules.clone();
        contract_args.must_avoid = draft.must_avoid.clone();
        contract_args.outline = draft_outline_with_naming_basis(&draft);
        apply_contract_v2_to_args(&mut contract_args, &draft.structured_contract_v2);
        let mut contract = match self.set_contract(&contract_args).await {
            Ok(contract) => contract,
            Err(error) => {
                let _ = tokio::fs::remove_dir_all(&staged_project_path).await;
                return Err(error);
            }
        };
        if !contract
            .get("success")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            let _ = tokio::fs::remove_dir_all(&staged_project_path).await;
            return Ok(contract);
        }
        if let Err(error) = self
            .lock_title_state_from_draft(&staged_project_path, &draft)
            .await
        {
            let _ = tokio::fs::remove_dir_all(&staged_project_path).await;
            return Err(error);
        }
        let approved_manifest = match self.read_manifest(&staged_project_path).await {
            Ok(manifest) => manifest,
            Err(error) => {
                let _ = tokio::fs::remove_dir_all(&staged_project_path).await;
                return Err(error);
            }
        };
        let approved_character_authority = approved_manifest
            .contract
            .as_ref()
            .map(|contract| character_authority_fingerprint(&contract.characters))
            .unwrap_or_default();
        if expected_character_authority != approved_character_authority {
            let approved_contract_characters = approved_manifest
                .contract
                .as_ref()
                .map(|contract| contract.characters.clone())
                .unwrap_or_default();
            let _ = tokio::fs::remove_dir_all(&staged_project_path).await;
            return Ok(json!({
                "success": false,
                "error_kind": "character_authority_mismatch",
                "error": "approved project character authority differs from the confirmed creation draft",
                "draft_path": draft_path.to_string_lossy(),
                "expected_character_authority": expected_character_authority,
                "approved_character_authority": approved_character_authority,
                "confirmed_draft_characters": draft.characters,
                "approved_contract_characters": approved_contract_characters,
                "next_action": "update_draft"
            }));
        }
        let _creation_guard = match self
            .lock_project_creation(approval_parent, draft.title.trim())
            .await
        {
            Ok(guard) => guard,
            Err(error) => {
                let _ = tokio::fs::remove_dir_all(&staged_project_path).await;
                return Err(error);
            }
        };
        let final_project_path = if let Some(path) = requested_project_path {
            path
        } else {
            unique_child_path(&output_root, &slugify(&draft.title))
        };
        if final_project_path.exists() {
            let _ = tokio::fs::remove_dir_all(&staged_project_path).await;
            return Ok(json!({
                "success": false,
                "recoverable": true,
                "error_kind": "project_path_conflict",
                "error": "the requested final project path already exists",
                "project_path": final_project_path.to_string_lossy(),
                "draft_path": draft_path.to_string_lossy(),
                "next_action": "approve_draft"
            }));
        }
        if let Err(error) = tokio::fs::rename(&staged_project_path, &final_project_path).await {
            let _ = tokio::fs::remove_dir_all(&staged_project_path).await;
            return Err(error.into());
        }
        let canonical_draft =
            approved_novel_creation_draft_from_manifest(&draft, &approved_manifest);
        let _ = tokio::fs::remove_file(&draft_path).await;
        let final_project_path_text = final_project_path.to_string_lossy().to_string();
        relocate_receipt_path_prefix(
            &mut init,
            &staged_project_path.to_string_lossy(),
            &final_project_path_text,
        );
        relocate_receipt_path_prefix(
            &mut contract,
            &staged_project_path.to_string_lossy(),
            &final_project_path_text,
        );
        init["project_path"] = json!(final_project_path_text);
        init["manifest_path"] = json!(final_project_path
            .join("project.json")
            .to_string_lossy()
            .to_string());
        if contract.get("project_path").is_some() {
            contract["project_path"] = json!(final_project_path_text);
        }

        Ok(json!({
            "success": true,
            "runtime_effect": "contract.approved",
            "draft_path": draft_path.to_string_lossy(),
            "project_path": final_project_path_text,
            "state": contract.get("state").cloned().unwrap_or_else(|| json!({})),
            "draft": novel_draft_summary(&canonical_draft),
            "init": init,
            "contract": contract,
            "recovered_title_conflict": recovered_title_conflict,
            "next_action": "run_project"
        }))
    }

    pub(super) async fn discard_draft(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let draft_path = self.require_draft_path(args)?;
        let existed = tokio::fs::try_exists(&draft_path).await.unwrap_or(false);
        if existed {
            tokio::fs::remove_file(&draft_path).await?;
        }
        Ok(json!({
            "success": true,
            "runtime_effect": "contract.discarded",
            "draft_path": draft_path.to_string_lossy(),
            "existed": existed
        }))
    }

    async fn lock_title_state_from_draft(
        &self,
        project_dir: &Path,
        draft: &NovelCreationDraft,
    ) -> anyhow::Result<()> {
        let mut manifest = self.read_manifest(project_dir).await?;
        manifest.title = draft.title.trim().to_string();
        manifest.title_state.provisional_title = draft.title.trim().to_string();
        manifest.title_state.canonical_title = draft.title.trim().to_string();
        manifest.title_state.source = "llm_contract".to_string();
        manifest.title_state.locked = true;
        manifest.title_state.rationale = draft.title_rationale.trim().to_string();
        manifest.title_state.updated_at = now_iso();
        self.write_manifest(project_dir, &manifest).await
    }
}

fn project_directory_is_internal_staging(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(".novel-approve-") || name.starts_with(".novel-init-"))
}

fn relocate_receipt_path_prefix(
    value: &mut serde_json::Value,
    temporary_root: &str,
    final_root: &str,
) {
    match value {
        serde_json::Value::String(text) if text.starts_with(temporary_root) => {
            *text = format!("{final_root}{}", &text[temporary_root.len()..]);
        }
        serde_json::Value::Array(items) => {
            for item in items {
                relocate_receipt_path_prefix(item, temporary_root, final_root);
            }
        }
        serde_json::Value::Object(fields) => {
            for item in fields.values_mut() {
                relocate_receipt_path_prefix(item, temporary_root, final_root);
            }
        }
        _ => {}
    }
}
