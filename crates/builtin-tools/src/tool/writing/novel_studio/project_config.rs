use super::*;

impl NovelStudioTool {
    pub(in crate::tool::writing::novel_studio) async fn init_project(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        if args.title.trim().is_empty() {
            anyhow::bail!("title is required for init_project");
        }
        let normalized_language = normalize_language(&args.language);
        if is_chinese_language(&normalized_language)
            && chinese_title_control_surface_issue(args.title.trim()).is_some()
        {
            return Ok(json!({
                "success": false,
                "recoverable": true,
                "error_kind": "language_contract_violation",
                "error": "Chinese-language fiction project title appears to contain workflow/control text.",
                "title": args.title.trim(),
                "language": normalized_language,
                "next_step_hint": "Choose a fresh Chinese title inferred from the user's request, then retry init_project. Do not copy field names, workflow text, or English metadata into the title."
            }));
        }
        let output_root = if args.output_root.trim().is_empty() {
            self.default_output_root()
        } else {
            args.output_root.trim()
        };
        let root = self.resolve_workspace_path(output_root)?;
        let _creation_guard = self
            .lock_project_creation(root.clone(), args.title.trim())
            .await?;
        let title_conflicts = if args.project_path.trim().is_empty() && !args.overwrite {
            find_existing_title_conflicts(&root, &args.title)
        } else {
            Vec::new()
        };
        if !title_conflicts.is_empty() && !args.allow_title_conflict {
            return Ok(json!({
                "success": false,
                "recoverable": true,
                "error": "title_conflict",
                "title": args.title.trim(),
                "title_conflicts": title_conflicts,
                "title_conflict_policy": "blocked_by_default_for_new_project",
                "next_step_hint": "This title or its normalized core was already used by an existing novel project. If the user asked to continue the old project, call list_projects/status and reuse the existing project_path. If the user explicitly asked to create a separate same-title project, retry with allow_title_conflict=true. Otherwise choose a fresh title and retry init_project."
            }));
        }
        let project_dir = if args.project_path.trim().is_empty()
            || project_path_looks_like_draft_file(args.project_path.trim())
        {
            unique_child_path(&root, &slugify(&args.title))
        } else {
            self.resolve_workspace_path(&args.project_path)?
        };

        if project_dir.exists() && !args.overwrite {
            if project_dir.is_file() {
                return Ok(json!({
                    "success": false,
                    "recoverable": true,
                    "error": format!("project_path points to a file, not a novel project directory: {}", project_dir.display()),
                    "project_path": project_dir.to_string_lossy(),
                    "next_step_hint": "Omit project_path so the tool can create a unique project folder from the title, or pass an existing novel project directory containing project.json."
                }));
            }
            if let Ok(manifest) = self.read_manifest(&project_dir).await {
                return Ok(json!({
                    "success": true,
                    "reused_existing": true,
                    "project_path": project_dir.to_string_lossy(),
                    "manifest_path": project_dir.join("project.json").to_string_lossy(),
                    "next_action": "status, set_contract, write_draft, or export",
                    "state": project_state_summary(&manifest),
                    "audit": audit_manifest(&manifest)
                }));
            }
            return Ok(json!({
                "success": false,
                "recoverable": true,
                "error": format!("path exists but is not a readable novel project: {}", project_dir.display()),
                "project_path": project_dir.to_string_lossy(),
                "next_step_hint": "Choose a different project_path, omit project_path so the tool can create a unique folder from title, or pass overwrite=true only when intentionally replacing the existing folder."
            }));
        }

        let now = now_iso();
        let manifest_title = args.title.trim().to_string();
        let title_is_temporary = project_title_is_temporary_placeholder(&manifest_title);
        let structured_contract_v2 = contract_v2_from_args(args);
        let manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: manifest_title.clone(),
            title_state: TitleState {
                provisional_title: manifest_title.clone(),
                canonical_title: manifest_title,
                source: if title_is_temporary {
                    "temporary_placeholder".to_string()
                } else {
                    "user_or_llm_contract".to_string()
                },
                locked: !title_is_temporary,
                rationale: if title_is_temporary {
                    "Temporary internal project title; replace through the story contract before formal writing approval."
                        .to_string()
                } else {
                    "Initial project title from user instruction or LLM story contract.".to_string()
                },
                updated_at: now.clone(),
            },
            language: normalized_language,
            genre: args.genre.trim().to_string(),
            brief: args.brief.trim().to_string(),
            target_units: args.target_units,
            chapter_unit_target: longform_policy::normalize_chapter_unit_target(
                args.chapter_unit_target,
                args.target_units,
            )
            .or_else(|| inferred_chapter_unit_target(args.target_units)),
            max_chapters_per_turn: args.max_chapters_per_turn.filter(|value| *value > 0),
            export_format: Some(export::normalize_export_format(args.format.trim())),
            export_when_complete: args.export_when_complete,
            approved_only: args.approved_only,
            created_at: now.clone(),
            updated_at: now,
            sources: Vec::new(),
            chapter_plans: Vec::new(),
            chapter_contracts: Vec::new(),
            context_packages: Vec::new(),
            chapter_architectures: Vec::new(),
            chapters: Vec::new(),
            reviews: Vec::new(),
            review_cycles: Vec::new(),
            truth_validations: Vec::new(),
            hook_debt_reports: Vec::new(),
            truth_files: Vec::new(),
            archives: Vec::new(),
            contract: None,
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: Vec::new(),
            story_bible: None,
            structured_contract_v2,
        };

        let initialize_at = if project_dir.exists() {
            project_dir.clone()
        } else {
            project_dir.with_file_name(format!(".novel-init-{}", uuid::Uuid::new_v4().simple()))
        };
        let staged = initialize_at != project_dir;
        let initialize_result = async {
            self.ensure_project_scaffold(&initialize_at).await?;
            atomic_write_file(
                initialize_at.join("README.md"),
                render_project_readme(&manifest),
            )
            .await?;
            atomic_write_file(initialize_at.join("contract.md"), String::new()).await?;
            atomic_write_file(
                initialize_at.join("continuity.md"),
                "# Continuity\n\n".to_string(),
            )
            .await?;
            self.write_manifest(&initialize_at, &manifest).await?;
            if staged {
                tokio::fs::rename(&initialize_at, &project_dir).await?;
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = initialize_result {
            if staged {
                let _ = tokio::fs::remove_dir_all(&initialize_at).await;
            }
            return Err(error);
        }

        let title_conflict_policy = if title_conflicts.is_empty() {
            "none"
        } else if args.allow_title_conflict {
            "explicit_allow_unique_project_path_allocated"
        } else {
            "blocked_by_default_for_new_project"
        };

        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "manifest_path": project_dir.join("project.json").to_string_lossy(),
            "next_action": "add_source or set_contract",
            "state": project_state_summary(&manifest),
            "title_conflicts": title_conflicts,
            "title_conflict_policy": title_conflict_policy
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn update_project(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        if !args.title.trim().is_empty() {
            manifest.title = args.title.trim().to_string();
        }
        if !args.language.trim().is_empty() {
            manifest.language = normalize_language(&args.language);
        }
        if !args.genre.trim().is_empty() {
            manifest.genre = args.genre.trim().to_string();
        }
        if !args.brief.trim().is_empty() {
            manifest.brief = args.brief.trim().to_string();
        }
        if args.target_units.is_some() {
            manifest.target_units = args.target_units;
        }
        if args.chapter_unit_target.is_some() {
            manifest.chapter_unit_target = longform_policy::normalize_chapter_unit_target(
                args.chapter_unit_target,
                manifest.target_units,
            );
        } else if manifest.chapter_unit_target.is_none() {
            manifest.chapter_unit_target = inferred_chapter_unit_target(manifest.target_units);
        } else if args.target_units.is_some() {
            manifest.chapter_unit_target = longform_policy::normalize_chapter_unit_target(
                manifest.chapter_unit_target,
                manifest.target_units,
            );
        }
        if args.max_chapters_per_turn.is_some() {
            manifest.max_chapters_per_turn = args.max_chapters_per_turn.filter(|value| *value > 0);
        }
        if !args.format.trim().is_empty() {
            manifest.export_format = Some(export::normalize_export_format(args.format.trim()));
        }
        if args.export_when_complete {
            manifest.export_when_complete = true;
        }
        if args.approved_only {
            manifest.approved_only = true;
        }
        if manifest.contract.is_some() {
            rebuild_story_bible_from_manifest(&mut manifest);
            ensure_volume_records_from_story_bible(&mut manifest);
        }
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        atomic_write_file(
            project_dir.join("README.md"),
            render_project_readme(&manifest),
        )
        .await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "state": project_state_summary(&manifest),
            "audit": audit_manifest(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn clone_project(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        if args.source_project_path.trim().is_empty() {
            anyhow::bail!("source_project_path is required for clone_project");
        }
        let source = self.resolve_workspace_path(&args.source_project_path)?;
        let source_manifest = self.read_manifest(&source).await?;
        let target = if args.project_path.trim().is_empty() {
            let output_root = self.output_root_for_args(args);
            let root = self.resolve_workspace_path(output_root.as_ref())?;
            root.join(format!("{}-clone", slugify(&source_manifest.title)))
        } else {
            self.resolve_workspace_path(&args.project_path)?
        };
        let target_parent = target
            .parent()
            .ok_or_else(|| anyhow::anyhow!("clone target has no parent: {}", target.display()))?;
        let target_name = target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("clone");
        let _clone_guard = self
            .lock_project_creation(target_parent.to_path_buf(), target_name)
            .await?;
        if target.exists() && !args.overwrite {
            anyhow::bail!("target project already exists: {}", target.display());
        }
        let staging =
            target.with_file_name(format!(".novel-clone-{}", uuid::Uuid::new_v4().simple()));
        let staged_manifest = async {
            copy_dir_recursive(&source, &staging).await?;
            let mut manifest = self.read_manifest(&staging).await?;
            if !args.title.trim().is_empty() {
                manifest.title = args.title.trim().to_string();
            }
            manifest.updated_at = now_iso();
            self.write_manifest(&staging, &manifest).await?;
            Ok::<NovelProjectManifest, anyhow::Error>(manifest)
        }
        .await;
        let manifest = match staged_manifest {
            Ok(manifest) => manifest,
            Err(error) => {
                let _ = tokio::fs::remove_dir_all(&staging).await;
                return Err(error);
            }
        };
        let backup = target.with_file_name(format!(
            ".novel-clone-backup-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let had_target = target.exists();
        if had_target {
            tokio::fs::rename(&target, &backup).await?;
        }
        if let Err(error) = tokio::fs::rename(&staging, &target).await {
            if had_target {
                let _ = tokio::fs::rename(&backup, &target).await;
            }
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return Err(error.into());
        }
        if had_target {
            tokio::fs::remove_dir_all(&backup).await?;
        }
        Ok(json!({
            "success": true,
            "project_path": target.to_string_lossy(),
            "state": project_state_summary(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn add_source(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        ensure_text_size(&args.content, "content")?;
        if args.content.trim().is_empty() {
            anyhow::bail!("content is required for add_source");
        }
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let id = format!("source-{:04}", manifest.sources.len() + 1);
        let title =
            first_non_empty(&[args.source_title.as_str(), args.title.as_str(), &id]).to_string();
        let path = format!("sources/{id}.md");
        let full_path = project_dir.join(&path);
        let body = render_source_file(
            &title,
            args.source_url.trim(),
            args.notes.trim(),
            &args.content,
        );
        atomic_write_file(full_path, body).await?;

        let record = SourceRecord {
            id,
            title,
            path: path.clone(),
            source_url: non_empty(args.source_url.trim()),
            notes: non_empty(args.notes.trim()),
            unit_count: count_units(&args.content, &manifest.language),
            created_at: now_iso(),
        };
        manifest.sources.push(record.clone());
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;

        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.checkpointed",
            "completion_scope": "checkpoint",
            "stage": pipeline::NovelPhase::SourceIntake,
            "next_action": if manifest.contract.is_none() { "set_contract" } else { "run_next_chapter" },
            "next_actions": if manifest.contract.is_none() {
                json!([
                    {
                        "action": "set_contract",
                        "requires": ["project_path", "premise or brief/outline", "continuity rules"],
                        "then": "run_next_chapter"
                    }
                ])
            } else {
                json!([
                    {
                        "action": "run_next_chapter",
                        "requires": ["project_path"],
                        "then": "persist_execution_package"
                    }
                ])
            },
            "project_path": project_dir.to_string_lossy(),
            "source": record,
            "state": project_state_summary(&manifest),
            "writing_policy": policy::fiction_stage_policy(
                "source_intake",
                if manifest.contract.is_none() { "set_contract" } else { "run_next_chapter" },
            ),
            "next_step_hint": if manifest.contract.is_none() {
                "Source material is saved as a governed checkpoint. Continue by setting the story/document contract; do not report completion yet."
            } else {
                "Source material is saved as a governed checkpoint. Continue the next bounded chapter/section and export only after target_units is satisfied."
            }
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn import_chapters(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        ensure_text_size(&args.content, "content")?;
        if args.content.trim().is_empty() {
            anyhow::bail!("content is required for import_chapters");
        }
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let start_number = durable_chapter_progress(&project_dir, &manifest)
            .await
            .next_chapter;
        let chapters = split_chapters(&args.content, &args.split_pattern)?;
        let mut imported = Vec::new();

        for (offset, (title, body)) in chapters.into_iter().enumerate() {
            let number = start_number + offset;
            let default_title = default_chapter_title(&manifest.language, number);
            let title =
                first_non_empty(&[title.as_str(), args.chapter_title.as_str(), &default_title])
                    .to_string();
            let body = normalize_chapter_body_for_record(&body, &title);
            let unit_count = count_units(&body, &manifest.language);
            let (volume_id, volume_title) = chapter_volume_pair(&manifest, number);
            let status = chapter_lifecycle::ChapterLifecycleStatus::ImportedUnverified.as_str();
            let record = ChapterRecord {
                number,
                title: title.clone(),
                volume_id,
                volume_title,
                path: format!("chapters/{number:04}_{}.md", slugify(&title)),
                summary: String::new(),
                unit_count,
                status: status.to_string(),
                key_facts: Vec::new(),
                continuity_updates: Vec::new(),
                created_at: now_iso(),
                updated_at: now_iso(),
            };
            write_chapter_record(&project_dir, &record, &body).await?;
            manifest.chapters.retain(|chapter| chapter.number != number);
            manifest.chapters.push(record.clone());
            imported.push(record);
        }

        manifest.chapters.sort_by_key(|chapter| chapter.number);
        refresh_continuity_truth_file(&project_dir, &mut manifest).await?;
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;

        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "imported_chapters": imported,
            "state": project_state_summary(&manifest),
            "audit": audit_manifest(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn update_style(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        ensure_text_size(&args.content, "content")?;
        if args.content.trim().is_empty() {
            anyhow::bail!("content is required for update_style");
        }
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let id = format!("style-{:04}", manifest.style_profiles.len() + 1);
        let title = first_non_empty(&[args.title.as_str(), "style profile"]).to_string();
        let path = format!("truth/{}.md", slugify(&title));
        tokio::fs::create_dir_all(project_dir.join("truth")).await?;
        atomic_write_file(
            project_dir.join(&path),
            render_style_file(&title, args.notes.trim(), args.content.trim()),
        )
        .await?;
        let profile = StyleProfileRecord {
            id,
            title: title.clone(),
            path: path.clone(),
            unit_count: count_units(&args.content, &manifest.language),
            created_at: now_iso(),
        };
        manifest.style_profiles.push(profile.clone());
        upsert_truth_record(
            &mut manifest,
            TruthFileRecord {
                section: "style".to_string(),
                path,
                unit_count: profile.unit_count,
                updated_at: now_iso(),
            },
        );
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "style_profile": profile,
            "state": project_state_summary(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn read_style(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let latest = manifest
            .style_profiles
            .last()
            .ok_or_else(|| anyhow::anyhow!("no style profile has been recorded"))?;
        let content = tokio::fs::read_to_string(project_dir.join(&latest.path)).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "style_profile": latest,
            "content": content
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn set_contract(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let governed_characters = normalize_contract_character_authority_lines(&args.characters);
        let mut structured_contract_v2 = contract_v2_from_args(args);
        resolve_relationship_character_ids(
            &mut structured_contract_v2.relationship_ledger,
            &governed_characters,
        );
        let contract = StoryContract {
            premise: sanitize_contract_text(args.premise.trim()),
            themes: clean_contract_list(&args.themes),
            characters: governed_characters,
            world_rules: clean_contract_list(&args.world_rules),
            style_rules: clean_contract_list(&args.style_rules),
            must_avoid: clean_contract_list(&args.must_avoid),
            outline: sanitize_contract_text(args.outline.trim()),
            structured_contract_v2,
            authority_contract: args.authority_contract.clone(),
            updated_at: now_iso(),
        };
        let previous_contract_fingerprint =
            governance::authority_fingerprint(&canonical_project_contract_projection(&manifest));
        let mut prospective_manifest = manifest.clone();
        prospective_manifest.structured_contract_v2 = contract.structured_contract_v2.clone();
        prospective_manifest.contract = Some(contract.clone());
        let contract_changed = manifest.contract.is_some()
            && previous_contract_fingerprint
                != governance::authority_fingerprint(&canonical_project_contract_projection(
                    &prospective_manifest,
                ));
        if contract_changed && manifest.chapters.iter().any(chapter_is_approved) {
            anyhow::bail!(
                "the canonical story contract cannot be replaced after approved history exists; create a new project or run an explicit history migration"
            );
        }
        if contract_changed {
            invalidate_unapproved_authority_descendants(&project_dir, &mut manifest, 0).await?;
        }
        manifest.structured_contract_v2 = contract.structured_contract_v2.clone();
        manifest.contract = Some(contract.clone());
        rebuild_story_bible_from_manifest(&mut manifest);
        ensure_volume_records_from_story_bible(&mut manifest);
        ensure_character_authority_ledger(&mut manifest);
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        atomic_write_file(project_dir.join("contract.md"), render_contract(&contract)).await?;

        let state = project_state_summary(&manifest);
        let audit = audit_manifest(&manifest);
        let writing_policy = policy::fiction_stage_policy(
            pipeline::NovelPhase::StoryContract.as_str(),
            "run_next_chapter",
        );
        let mut response = serde_json::Map::new();
        response.insert("success".to_string(), json!(true));
        response.insert("runtime_effect".to_string(), json!("artifact.checkpointed"));
        response.insert("completion_scope".to_string(), json!("checkpoint"));
        response.insert(
            "stage".to_string(),
            json!(pipeline::NovelPhase::StoryContract),
        );
        response.insert("next_action".to_string(), json!("run_next_chapter"));
        response.insert(
            "next_actions".to_string(),
            json!([{
                "action": "run_next_chapter",
                "requires": ["project_path"],
                "then": "persist_execution_package"
            }]),
        );
        response.insert(
            "project_path".to_string(),
            json!(project_dir.to_string_lossy()),
        );
        response.insert(
            "contract_path".to_string(),
            json!(project_dir.join("contract.md").to_string_lossy()),
        );
        response.insert(
            "story_bible_path".to_string(),
            json!(project_dir.join("story_bible.md").to_string_lossy()),
        );
        response.insert("state".to_string(), state);
        response.insert("audit".to_string(), audit);
        response.insert("writing_policy".to_string(), writing_policy);
        response.insert(
            "next_step_hint".to_string(),
            json!("The governed contract is saved as a checkpoint. Continue with run_next_chapter, draft bounded prose, audit/revise, update truth, approve, and export only after target_units is satisfied."),
        );
        Ok(serde_json::Value::Object(response))
    }
}

pub(super) async fn invalidate_unapproved_authority_descendants(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
    authority_cutoff_chapter: usize,
) -> anyhow::Result<()> {
    let unapproved = manifest
        .chapters
        .iter()
        .filter(|chapter| {
            chapter.number > authority_cutoff_chapter && !chapter_is_approved(chapter)
        })
        .map(|chapter| chapter.number)
        .collect::<BTreeSet<_>>();
    let mut stale_numbers = unapproved.clone();
    stale_numbers.extend(
        manifest
            .context_packages
            .iter()
            .filter(|record| record.number > authority_cutoff_chapter)
            .map(|record| record.number),
    );
    stale_numbers.extend(
        manifest
            .chapter_plans
            .iter()
            .filter(|record| record.number > authority_cutoff_chapter)
            .map(|record| record.number),
    );

    let mut artifact_paths = Vec::new();
    for record in manifest
        .context_packages
        .iter()
        .filter(|record| record.number > authority_cutoff_chapter)
    {
        artifact_paths.extend([
            record.path.clone(),
            record.rules_path.clone(),
            record.trace_path.clone(),
        ]);
    }
    for record in manifest
        .chapter_plans
        .iter()
        .filter(|record| record.number > authority_cutoff_chapter)
    {
        artifact_paths.push(record.path.clone());
    }
    for record in manifest
        .chapter_contracts
        .iter()
        .filter(|record| record.number > authority_cutoff_chapter)
    {
        artifact_paths.extend([record.path.clone(), record.markdown_path.clone()]);
    }
    for record in manifest
        .chapter_architectures
        .iter()
        .filter(|record| record.number > authority_cutoff_chapter)
    {
        artifact_paths.push(record.path.clone());
    }
    for record in manifest
        .review_cycles
        .iter()
        .filter(|record| record.chapter_number > authority_cutoff_chapter)
    {
        artifact_paths.push(record.path.clone());
    }
    for path in artifact_paths {
        let path = project_dir.join(path);
        if path.exists() {
            tokio::fs::remove_file(path).await?;
        }
    }
    for number in &stale_numbers {
        for path in [
            pending_settlement_path(project_dir, *number),
            approved_settlement_path(project_dir, *number),
            approval_receipt_path(project_dir, *number),
            approval_journal_path(project_dir, *number),
        ] {
            if path.exists() {
                tokio::fs::remove_file(path).await?;
            }
        }
        let candidate_dir = project_dir.join("reviews/candidates");
        if let Ok(mut entries) = tokio::fs::read_dir(&candidate_dir).await {
            let prefix = format!("chapter-{number:04}.candidate-");
            let best_name = format!("chapter-{number:04}.best.json");
            while let Some(entry) = entries.next_entry().await? {
                if entry.file_name().to_str().is_some_and(|name| {
                    (name.starts_with(&prefix) && name.ends_with(".json")) || name == best_name
                }) {
                    tokio::fs::remove_file(entry.path()).await?;
                }
            }
        }
    }
    manifest
        .context_packages
        .retain(|record| record.number <= authority_cutoff_chapter);
    manifest
        .chapter_plans
        .retain(|record| record.number <= authority_cutoff_chapter);
    manifest
        .chapter_contracts
        .retain(|record| record.number <= authority_cutoff_chapter);
    manifest
        .chapter_architectures
        .retain(|record| record.number <= authority_cutoff_chapter);
    manifest
        .reviews
        .retain(|record| record.chapter_number <= authority_cutoff_chapter);
    manifest
        .review_cycles
        .retain(|record| record.chapter_number <= authority_cutoff_chapter);
    manifest
        .truth_validations
        .retain(|record| record.chapter_number <= authority_cutoff_chapter);
    invalidate_story_bible_planning_after(manifest.story_bible.as_mut(), authority_cutoff_chapter);
    for chapter in manifest
        .chapters
        .iter_mut()
        .filter(|chapter| unapproved.contains(&chapter.number))
    {
        chapter.status = chapter_lifecycle::ChapterLifecycleStatus::Draft
            .as_str()
            .to_string();
        chapter.updated_at = now_iso();
    }
    Ok(())
}

pub(super) fn govern_novel_creation_draft_authority(draft: &mut NovelCreationDraft) {
    let characters = normalize_contract_character_authority_lines(&draft.characters);
    if characters.is_empty() {
        return;
    }
    draft.characters = characters;
    resolve_relationship_character_ids(
        &mut draft.structured_contract_v2.relationship_ledger,
        &draft.characters,
    );
}

pub(super) fn character_authority_fingerprint(lines: &[String]) -> Vec<(String, String, String)> {
    let mut fingerprint = lines
        .iter()
        .map(|line| {
            let character = super::super::creation_contract::draft_character_line_to_contract(line);
            (
                character.character_id.trim().to_string(),
                character.canonical_name.trim().to_string(),
                character.role.trim().to_string(),
            )
        })
        .filter(|(_, name, _)| !name.is_empty())
        .collect::<Vec<_>>();
    fingerprint.sort();
    fingerprint
}

fn normalize_contract_character_authority_lines(lines: &[String]) -> Vec<String> {
    clean_contract_list(lines)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let mut character =
                super::super::creation_contract::draft_character_line_to_contract(&line);
            if character.character_id.trim().is_empty() {
                character.character_id = format!("character-{:04}", index + 1);
            }
            if character.name_source.trim().is_empty() {
                character.name_source = "contract_authority".to_string();
            }
            character.to_draft_line()
        })
        .collect()
}

fn resolve_relationship_character_ids(
    relationships: &mut [RelationshipLedgerEntry],
    character_lines: &[String],
) {
    let characters = character_lines
        .iter()
        .map(|line| super::super::creation_contract::draft_character_line_to_contract(line))
        .filter(|character| {
            !character.character_id.trim().is_empty() && !character.canonical_name.trim().is_empty()
        })
        .collect::<Vec<_>>();
    let ids_by_name = characters
        .iter()
        .map(|character| {
            (
                character.canonical_name.clone(),
                character.character_id.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for relationship in relationships {
        let mut resolved_names = Vec::with_capacity(relationship.characters.len());
        let mut resolved_ids = Vec::with_capacity(relationship.characters.len());
        for reference in &relationship.characters {
            if let Some(id) = ids_by_name.get(reference.trim()) {
                resolved_names.push(reference.trim().to_string());
                resolved_ids.push(id.clone());
                continue;
            }
            let reference_role = super::super::creation_contract::draft_character_role_from_basis(
                reference,
                &reference.to_ascii_lowercase(),
            );
            if reference_role == "角色" {
                continue;
            }
            let Some(character) = characters.iter().find(|character| {
                super::super::creation_contract::draft_character_role_from_basis(
                    &character.role,
                    &character.role.to_ascii_lowercase(),
                ) == reference_role
            }) else {
                continue;
            };
            resolved_names.push(character.canonical_name.clone());
            resolved_ids.push(character.character_id.clone());
        }
        if resolved_ids.len() == relationship.characters.len() {
            relationship.characters = resolved_names;
            relationship.character_ids = resolved_ids;
        }
    }
}
