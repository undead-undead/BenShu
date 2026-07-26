use super::*;

impl NovelStudioTool {
    pub(in crate::tool::writing::novel_studio) async fn add_chapter(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let raw_content = args.content.clone();
        let content = sanitize_saved_prose(&raw_content);
        ensure_text_size(&content, "content")?;
        if content.trim().is_empty() {
            anyhow::bail!("content is required for add_chapter");
        }
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let content = sanitize_chinese_script_noise(&manifest, &content);
        let content = repair_contract_character_name_typos(&manifest, &content);
        let number = match args.chapter_number {
            Some(number) => number,
            None => {
                durable_chapter_progress(&project_dir, &manifest)
                    .await
                    .next_chapter
            }
        };
        if args.action == "write_draft" {
            require_sealed_chapter_authority(&manifest, number)?;
        }
        let default_title = default_chapter_title(&manifest.language, number);
        let title = first_non_empty(&[
            args.chapter_title.as_str(),
            args.title.as_str(),
            default_title.as_str(),
        ]);
        let title = repair_contract_character_name_typos(&manifest, title);
        let existing_record = manifest
            .chapters
            .iter()
            .find(|chapter| chapter.number == number)
            .cloned();
        let path = existing_record
            .as_ref()
            .map(|chapter| chapter.path.clone())
            .unwrap_or_else(|| stable_chapter_path(number));
        let chapter_path = project_dir.join(&path);
        if chapter_path.exists() && existing_record.is_none() {
            archive_chapter_file(&project_dir, number, &chapter_path).await?;
        }
        if chapter_path.exists() && existing_record.is_some() {
            archive_chapter_file(&project_dir, number, &chapter_path).await?;
        }

        let unit_count = count_units(&content, &manifest.language);
        let status = chapter_lifecycle::ChapterLifecycleStatus::Draft.as_str();
        let now = now_iso();
        let summary_fallback = chapter_summary_fallback(&content, &manifest.language);
        let summary = compact_chapter_summary(
            first_non_empty(&[args.summary.trim(), summary_fallback.as_str()]),
            &manifest.language,
        );
        let title = final_chapter_title_from_body_with_metadata(
            &manifest,
            number,
            &title,
            &summary,
            &args.key_facts,
            &args.continuity_updates,
            &content,
        );
        let (volume_id, volume_title) = chapter_volume_pair(&manifest, number);
        let mut record = ChapterRecord {
            number,
            title: title.to_string(),
            volume_id,
            volume_title,
            path: path.clone(),
            summary: repair_contract_character_name_typos(&manifest, &summary),
            unit_count,
            status: status.to_string(),
            key_facts: clean_contract_character_name_typos(
                &manifest,
                compact_truth_items(clean_list(&args.key_facts), CHAPTER_FACT_LIMIT),
            ),
            continuity_updates: clean_contract_character_name_typos(
                &manifest,
                compact_truth_items(
                    clean_list(&args.continuity_updates),
                    CHAPTER_CONTINUITY_LIMIT,
                ),
            ),
            created_at: existing_record
                .as_ref()
                .map(|chapter| chapter.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        let content = normalize_chapter_body_for_record(&content, &record.title);
        let content = sanitize_chinese_script_noise(&manifest, &content);
        let content = repair_contract_character_name_typos(&manifest, &content);
        normalize_chapter_metadata_against_body(&manifest, &mut record, &content);
        let truth_validation =
            write_truth_validation_record(&project_dir, &mut manifest, &record, &content).await?;
        let mut quality_gate =
            chapter_quality_gate(&manifest, &record, &content, &truth_validation.issues);
        let metadata_gate = chapter_metadata_gate(&manifest, &record, &content);
        let pre_sanitized_issues = pre_sanitized_content_issues(&manifest, &raw_content)
            .into_iter()
            .filter(|issue| pre_sanitized_issue_survives_cleanup(&manifest, issue, &content))
            .collect::<Vec<_>>();
        extend_quality_gate_issues(
            &mut quality_gate,
            &manifest,
            &record,
            &content,
            pre_sanitized_issues,
        );
        let duplicate_issues =
            cross_chapter_duplicate_issues(&project_dir, &manifest, &record, &content).await;
        route_cross_chapter_duplicate_issues(&mut quality_gate, duplicate_issues);
        if !quality_gate.passed {
            record.status = chapter_lifecycle::ChapterLifecycleStatus::NeedsRevision
                .as_str()
                .to_string();
        }
        write_chapter_record(&project_dir, &record, &content).await?;
        manifest.chapters.retain(|chapter| chapter.number != number);
        manifest.chapters.push(record.clone());
        manifest.chapters.sort_by_key(|chapter| chapter.number);
        sync_final_chapter_title_to_support_records(
            &project_dir,
            &mut manifest,
            number,
            &record.title,
        )
        .await?;
        refresh_continuity_truth_file(&project_dir, &mut manifest).await?;
        let hook_debt = write_hook_debt_report_record(&project_dir, &mut manifest, number).await?;
        let stage_authority = write_stage_authority_record(
            &project_dir,
            &manifest,
            number,
            "draft",
            governance::AuthorityRole::Writer,
            &chapter_revision_fingerprint(&record, &content),
        )
        .await?;
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        append_continuity(&project_dir, &record).await?;
        let readable_export = self
            .sync_readable_txt_export(&project_dir, &manifest)
            .await?;
        let total_units = project_total_units(&manifest);
        let target_units = manifest.target_units;
        let durable_progress = durable_chapter_progress(&project_dir, &manifest).await;
        let target_reached = durable_project_target_reached(&manifest, &durable_progress);
        let checkpoint_only = target_units.is_some_and(|target| target > 0) && !target_reached;
        let runtime_effect = if quality_gate.passed {
            if checkpoint_only {
                "artifact.checkpointed"
            } else {
                "artifact.written"
            }
        } else {
            "artifact.needs_revision"
        };
        let metadata_needs_repair = metadata_gate.needs_repair();
        let accepted = quality_gate.passed && !metadata_gate.needs_repair();
        let outcome_status = chapter_outcome_status(&quality_gate, &metadata_gate);
        let quality_decision =
            chapter_quality::chapter_quality_decision(&quality_gate, &metadata_gate);

        let preferred_artifact_path = readable_export.current_path.to_string_lossy().to_string();
        Ok(json!({
            "success": true,
            "accepted": accepted,
            "outcome_status": outcome_status,
            "requires_followup": !accepted || metadata_needs_repair,
            "completion_gate": chapter_completion_gate_json(accepted, outcome_status),
            "runtime_effect": runtime_effect,
            "recoverable": !quality_gate.passed,
            "completion_scope": if checkpoint_only { "checkpoint" } else { "artifact" },
            "project_path": project_dir.to_string_lossy(),
            "artifact_path": chapter_path.to_string_lossy(),
            "draft_artifact_path": if quality_gate.passed { serde_json::Value::Null } else { json!(chapter_path.to_string_lossy()) },
            "approved_export_ready": chapter_is_approved(&record),
            "preferred_artifact_path": preferred_artifact_path,
            "txt_artifact_path": readable_export.current_path.to_string_lossy(),
            "txt_collection_path": readable_export.collection_path.to_string_lossy(),
            "readable_export": readable_export.to_json(),
            "unit_count": record.unit_count,
            "total_units": total_units,
            "target_units": target_units,
            "target_reached": target_reached,
            "chapter": record,
            "quality_gate": quality_gate,
            "metadata_gate": metadata_gate,
            "quality_decision": quality_decision,
            "truth_validation": truth_validation,
            "hook_debt": hook_debt,
            "stage_authority": stage_authority,
            "state": apply_durable_chapter_progress(
                project_state_summary(&manifest),
                &manifest,
                &durable_progress,
            ),
            "audit": audit_manifest(&manifest),
            "next_action": if !quality_gate.passed {
                "revise_chapter"
            } else if metadata_needs_repair {
                "repair_chapter_metadata"
            } else {
                "audit_chapter"
            },
            "next_step_hint": if quality_gate.passed {
                if metadata_needs_repair {
                    "Repair chapter metadata only: update title, summary, key_facts, or continuity_updates without rewriting body text."
                } else if checkpoint_only {
                    "Run audit_chapter, then continue the next bounded chapter/section toward target_units before export."
                } else {
                    "Run audit_chapter before export."
                }
            } else {
                "Revise the chapter body so it preserves the story contract, removes placeholder/omission text, and resolves quality_gate issues before export."
            }
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn write_draft(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let result = self.add_chapter(args).await?;
        let next_action = if result
            .get("quality_gate")
            .and_then(|gate| gate.get("passed"))
            .and_then(|passed| passed.as_bool())
            .unwrap_or(true)
        {
            if result
                .get("metadata_gate")
                .and_then(|gate| gate.get("passed"))
                .and_then(|passed| passed.as_bool())
                .unwrap_or(true)
            {
                "audit_chapter"
            } else {
                "repair_chapter_metadata"
            }
        } else {
            "revise_chapter"
        };
        Ok(with_stage(
            result,
            pipeline::NovelPhase::Draft.as_str(),
            next_action,
        ))
    }

    pub(in crate::tool::writing::novel_studio) async fn run_project(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let mut args = args.clone();
        if args.project_path.trim().is_empty() {
            if args.title.trim().is_empty() {
                anyhow::bail!("project_path or title is required for run_project");
            }
            let init = self.init_project(&args).await?;
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
            args.project_path = project_path;
        }

        let project_dir = self.require_project_path(&args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let durable_progress = durable_chapter_progress(&project_dir, &manifest).await;
        let completion_blockers = durable_project_completion_blockers(&manifest, &durable_progress);
        let complete = durable_project_target_reached(&manifest, &durable_progress)
            && completion_blockers.is_empty();
        let export = if complete && (args.export_when_complete || manifest.export_when_complete) {
            let mut export_args = args.clone();
            export_args.action = "export".to_string();
            if export_args.format.trim().is_empty() {
                export_args.format = manifest
                    .export_format
                    .clone()
                    .unwrap_or_else(|| "txt".to_string());
            }
            export_args.approved_only = args.approved_only || manifest.approved_only;
            Some(self.export(&export_args).await?)
        } else {
            None
        };
        if complete {
            return Ok(json!({
                "success": true,
                "runtime_effect": if export.is_some() { "artifact.exported" } else { "read" },
                "stage": if export.is_some() {
                    pipeline::NovelPhase::Export
                } else {
                    pipeline::NovelPhase::Approval
                },
                "packet_kind": "assigned_worker_policy_packet",
                "project_path": project_dir.to_string_lossy(),
                "state": apply_durable_chapter_progress(
                    project_state_summary(&manifest),
                    &manifest,
                    &durable_progress,
                ),
                "audit": audit_manifest(&manifest),
                "complete": true,
                "completion_gate": {
                    "passed": true,
                    "blockers": completion_blockers
                },
                "export": export,
                "next_action": "status"
            }));
        }

        self.assigned_worker_policy_packet(&args, "run_project")
            .await
    }

    pub(in crate::tool::writing::novel_studio) async fn run_next_chapter(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        self.ensure_project_scaffold(&project_dir).await?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        if manifest.contract.is_none() {
            return self
                .assigned_worker_policy_packet(args, "run_next_chapter")
                .await;
        }
        if !governed_project_readiness_blockers(&manifest).is_empty() {
            return self
                .assigned_worker_policy_packet(args, "run_next_chapter")
                .await;
        }

        let chapter_number = match args.chapter_number {
            Some(number) => number,
            None => {
                durable_chapter_progress(&project_dir, &manifest)
                    .await
                    .next_chapter
            }
        };
        let has_plan = manifest
            .chapter_plans
            .iter()
            .any(|plan| plan.number == chapter_number);
        let mut prepared_plan = None;
        if !has_plan {
            if let Some(plan) = fallback_chapter_plan_from_manifest(&manifest, chapter_number) {
                let mut plan_args = args.clone();
                plan_args.action = "add_chapter_plan".to_string();
                plan_args.project_path = if args.project_path.trim().is_empty() {
                    project_dir.to_string_lossy().to_string()
                } else {
                    args.project_path.clone()
                };
                plan_args.chapter_number = Some(chapter_number);
                plan_args.plan = plan;
                let plan_result = self.add_chapter_plan(&plan_args).await?;
                prepared_plan = Some(plan_result);
                manifest = self.read_manifest(&project_dir).await?;
            }
        }

        let chapter_title = manifest
            .chapter_plans
            .iter()
            .find(|plan| plan.number == chapter_number)
            .map(|plan| plan.title.clone())
            .unwrap_or_else(|| default_chapter_title(&manifest.language, chapter_number));
        let context = build_context_payload(&project_dir, &manifest, chapter_number).await?;
        let draft_project_path = if args.project_path.trim().is_empty() {
            project_dir.to_string_lossy().to_string()
        } else {
            args.project_path.clone()
        };

        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.checkpointed",
            "completion_scope": "checkpoint",
            "stage": pipeline::NovelPhase::Draft,
            "read_only": false,
            "project_path": draft_project_path,
            "chapter_number": chapter_number,
            "chapter_title": chapter_title,
            "state": project_state_summary(&manifest),
            "audit": audit_manifest(&manifest),
            "context": context,
            "prepared_plan": prepared_plan,
            "next_action": "write_draft",
            "writing_phase": runner::writing_phase_contract(
                pipeline::NovelPhase::Draft,
                "write_draft",
                &project_dir,
                chapter_number,
                &chapter_title,
                "Write the complete next chapter from the project contract and context package, then persist it through the content submission contract.",
                manifest.chapter_unit_target,
            ),
            "progress_report_contract": crate::tool::writing::session_surface::longform_progress_report_contract(),
            "writing_policy": policy::fiction_stage_policy(
                pipeline::NovelPhase::Draft.as_str(),
                "write_draft",
            ),
            "next_step_hint": "This is a successful writer phase packet, not a completed chapter. Execute writing_phase, persist the chapter with write_draft, and keep full prose out of the chat transcript."
        }))
    }
}

pub(super) async fn sync_final_chapter_title_to_support_records(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
    number: usize,
    final_title: &str,
) -> anyhow::Result<()> {
    let final_title = final_title.trim();
    if final_title.is_empty() {
        return Ok(());
    }
    let now = now_iso();

    for plan in manifest
        .chapter_plans
        .iter_mut()
        .filter(|plan| plan.number == number)
    {
        if plan.title != final_title {
            plan.title = final_title.to_string();
            plan.updated_at = now.clone();
            sync_markdown_title_lines(&project_dir.join(&plan.path), final_title).await?;
        }
    }

    for contract in manifest
        .chapter_contracts
        .iter_mut()
        .filter(|contract| contract.number == number)
    {
        if contract.title != final_title {
            contract.title = final_title.to_string();
            contract.updated_at = now.clone();
            sync_json_title_field(&project_dir.join(&contract.path), final_title).await?;
            sync_markdown_title_lines(&project_dir.join(&contract.markdown_path), final_title)
                .await?;
        }
    }

    for architecture in manifest
        .chapter_architectures
        .iter_mut()
        .filter(|architecture| architecture.number == number)
    {
        if architecture.title != final_title {
            architecture.title = final_title.to_string();
            architecture.updated_at = now.clone();
            sync_markdown_title_lines(&project_dir.join(&architecture.path), final_title).await?;
        }
    }

    Ok(())
}

async fn sync_json_title_field(path: &Path, final_title: &str) -> anyhow::Result<()> {
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return Ok(());
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Ok(());
    };
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "title".to_string(),
            serde_json::Value::String(final_title.to_string()),
        );
        atomic_write_file(path.to_path_buf(), serde_json::to_string_pretty(&value)?).await?;
    }
    Ok(())
}

async fn sync_markdown_title_lines(path: &Path, final_title: &str) -> anyhow::Result<()> {
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return Ok(());
    };
    let mut changed = false;
    let mut in_frontmatter = false;
    let mut seen_frontmatter_start = false;
    let rendered = raw
        .lines()
        .map(|line| {
            if line.trim() == "---" {
                if !seen_frontmatter_start {
                    seen_frontmatter_start = true;
                    in_frontmatter = true;
                } else if in_frontmatter {
                    in_frontmatter = false;
                }
                return line.to_string();
            }
            if in_frontmatter && line.trim_start().starts_with("title:") {
                changed = true;
                return format!("title: {}", yaml_line(final_title));
            }
            if line.starts_with("- Title: ") {
                changed = true;
                return format!("- Title: {final_title}");
            }
            if line.starts_with("# Plan: ") {
                changed = true;
                return format!("# Plan: {final_title}");
            }
            if line.starts_with("# Architecture: ") {
                changed = true;
                return format!("# Architecture: {final_title}");
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    if changed {
        let suffix = if raw.ends_with('\n') { "\n" } else { "" };
        atomic_write_file(path.to_path_buf(), format!("{rendered}{suffix}")).await?;
    }
    Ok(())
}
