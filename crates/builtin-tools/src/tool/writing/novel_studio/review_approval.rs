use super::*;

fn build_local_review_record(
    args: &NovelStudioArgs,
    chapter: &ChapterRecord,
    body: &str,
    authority_fingerprint: &str,
    quality_gate: &ChapterQualityGate,
) -> ReviewReceipt {
    let body_fingerprint = chapter_quality::chapter_body_fingerprint(body);
    let locally_hard_codes = quality_gate
        .findings
        .iter()
        .filter(|finding| finding.hard_blocking())
        .map(|finding| finding.code.as_str())
        .collect::<BTreeSet<_>>();
    let mut findings = quality_gate.findings.clone();
    findings.extend(
        args.findings
            .iter()
            .filter(|finding| {
                finding.hard_blocking()
                    && locally_hard_codes.contains(finding.code.as_str())
                    && finding.authority_fingerprint == authority_fingerprint
                    && finding.body_fingerprint == body_fingerprint
                    && !finding.authority_evidence.is_empty()
                    && !finding.body_evidence.is_empty()
            })
            .cloned(),
    );
    let mut issues = findings
        .iter()
        .filter(|finding| finding.hard_blocking())
        .map(|finding| finding.message.clone())
        .collect::<Vec<_>>();
    issues.sort();
    issues.dedup();
    let mut advisories = quality_gate.warnings.clone();
    advisories.extend(clean_list(&args.advisories));
    // Legacy free-text issues are retained as advisory telemetry only. They can
    // no longer create a passed review or a hard blocker.
    advisories.extend(clean_list(&args.issues));
    advisories.sort();
    advisories.dedup();
    let has_hard_finding = findings.iter().any(|finding| finding.hard_blocking());
    let verdict = if args.verdict.trim().eq_ignore_ascii_case("rejected") {
        "rejected".to_string()
    } else if !has_hard_finding {
        "passed".to_string()
    } else {
        "needs_revision".to_string()
    };
    ReviewReceipt {
        chapter_number: chapter.number,
        chapter_fingerprint: chapter_quality::chapter_body_fingerprint(body),
        authority_fingerprint: authority_fingerprint.to_string(),
        findings,
        advisories,
        score: args.score,
        locally_validated: true,
        verdict,
        issues,
        feedback: args.feedback.trim().to_string(),
        created_at: now_iso(),
    }
}

async fn evaluate_revision_gates(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    body: &str,
    truth_issues: &[String],
) -> (ChapterQualityGate, ChapterMetadataGate) {
    let mut quality_gate = chapter_quality_gate(manifest, chapter, body, truth_issues);
    let metadata_gate = chapter_metadata_gate(manifest, chapter, body);
    let duplicate_issues =
        cross_chapter_duplicate_issues(project_dir, manifest, chapter, body).await;
    route_cross_chapter_duplicate_issues(&mut quality_gate, duplicate_issues);
    (quality_gate, metadata_gate)
}

impl NovelStudioTool {
    pub(in crate::tool::writing::novel_studio) async fn read_chapter(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let number = args
            .chapter_number
            .or_else(|| latest_chapter_number(&manifest))
            .ok_or_else(|| anyhow::anyhow!("no chapter exists in this project"))?;
        let chapter = manifest
            .chapters
            .iter()
            .find(|chapter| chapter.number == number)
            .cloned();
        let Some(chapter) = chapter else {
            let alternatives = self
                .alternative_projects_with_chapter(args, &project_dir, number)
                .await?;
            return Ok(json!({
                "success": false,
                "recoverable": true,
                "error_kind": "chapter_not_found",
                "error": format!("chapter {number} not found in selected project"),
                "requested_chapter": number,
                "project_path": project_dir.to_string_lossy(),
                "state": project_state_summary(&manifest),
                "alternative_projects": alternatives,
                "next_step_hint": "If one alternative matches the active conversation/project title or has the desired latest chapter, retry read_chapter with that project_path. Otherwise continue by composing from the selected project's latest available chapter."
            }));
        };
        let chapter_path = project_dir.join(&chapter.path);
        let raw = tokio::fs::read_to_string(&chapter_path).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "artifact_path": chapter_path.to_string_lossy(),
            "runtime_effect": "artifact.verified",
            "verification_scope": "chapter",
            "chapter": chapter,
            "content": normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title),
            "read_only": true,
            "next_actions": [
                {
                    "action": "revise_chapter",
                    "requires": ["project_path", "chapter_number", "content or metadata fields"],
                    "metadata_fields": ["summary", "key_facts", "continuity_updates", "chapter_title", "status", "revision_notes", "feedback"],
                    "runtime_effect": "artifact.written"
                },
                {
                    "action": "review_chapter",
                    "requires": ["project_path", "chapter_number", "verdict or issues/feedback"]
                }
            ],
            "next_step_hint": "If the user asked to revise, complete, update, or save this chapter, call revise_chapter next. A read-only result is not a durable artifact mutation."
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn review_chapter(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let number = args
            .chapter_number
            .or_else(|| latest_chapter_number(&manifest))
            .ok_or_else(|| anyhow::anyhow!("no chapter exists in this project"))?;
        if !manifest
            .chapters
            .iter()
            .any(|chapter| chapter.number == number)
        {
            anyhow::bail!("chapter {number} not found");
        }
        let chapter = manifest
            .chapters
            .iter()
            .find(|chapter| chapter.number == number)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("chapter {number} not found"))?;
        let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
        let body = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
        let authority_fingerprint = require_sealed_chapter_authority(&manifest, number)?
            .authority_root_fingerprint
            .clone();
        let truth_validation =
            write_truth_validation_record(&project_dir, &mut manifest, &chapter, &body).await?;
        let mut quality_gate =
            chapter_quality_gate(&manifest, &chapter, &body, &truth_validation.issues);
        let metadata_gate = chapter_metadata_gate(&manifest, &chapter, &body);
        let duplicate_issues =
            cross_chapter_duplicate_issues(&project_dir, &manifest, &chapter, &body).await;
        route_cross_chapter_duplicate_issues(&mut quality_gate, duplicate_issues);
        let review =
            build_local_review_record(args, &chapter, &body, &authority_fingerprint, &quality_gate);
        let verdict = review.verdict.clone();
        tokio::fs::create_dir_all(project_dir.join("reviews")).await?;
        let review_path = project_dir.join("reviews").join(format!(
            "chapter-{number:04}-review-{:04}.md",
            manifest
                .reviews
                .iter()
                .filter(|review| review.chapter_number == number)
                .count()
                + 1
        ));
        atomic_write_file(review_path.clone(), render_review_file(&review)).await?;
        if let Some(chapter) = manifest
            .chapters
            .iter_mut()
            .find(|chapter| chapter.number == number)
        {
            chapter.status = match verdict.as_str() {
                "passed" => chapter_lifecycle::ChapterLifecycleStatus::ReviewPassed
                    .as_str()
                    .to_string(),
                "rejected" => chapter_lifecycle::ChapterLifecycleStatus::Rejected
                    .as_str()
                    .to_string(),
                _ => chapter_lifecycle::ChapterLifecycleStatus::NeedsRevision
                    .as_str()
                    .to_string(),
            };
            chapter.updated_at = now_iso();
        }
        manifest.reviews.push(review.clone());
        let review_cycle = write_review_cycle_record(
            &project_dir,
            &mut manifest,
            number,
            &verdict,
            &review.issues,
        )
        .await?;
        let hook_debt = write_hook_debt_report_record(&project_dir, &mut manifest, number).await?;
        let stage_authority = write_stage_authority_record(
            &project_dir,
            &manifest,
            number,
            "review",
            governance::AuthorityRole::Auditor,
            &review.chapter_fingerprint,
        )
        .await?;
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        let durable_progress = durable_chapter_progress(&project_dir, &manifest).await;
        let next_action =
            review_cycle_next_action(&review_cycle, &verdict, metadata_gate.needs_repair());

        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.reviewed",
            "next_action": next_action,
            "project_path": project_dir.to_string_lossy(),
            "artifact_path": review_path.to_string_lossy(),
            "review_path": review_path.to_string_lossy(),
            "review": review,
            "quality_gate": quality_gate,
            "metadata_gate": metadata_gate,
            "truth_validation": truth_validation,
            "review_cycle": review_cycle,
            "hook_debt": hook_debt,
            "stage_authority": stage_authority,
            "state": apply_durable_chapter_progress(
                project_state_summary(&manifest),
                &manifest,
                &durable_progress,
            ),
            "audit": audit_manifest(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn audit_chapter(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let number = args
            .chapter_number
            .or_else(|| latest_chapter_number(&manifest))
            .ok_or_else(|| anyhow::anyhow!("no chapter exists in this project"))?;
        let chapter = manifest
            .chapters
            .iter()
            .find(|chapter| chapter.number == number)
            .ok_or_else(|| anyhow::anyhow!("chapter {number} not found"))?
            .clone();
        let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
        let content = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
        let authority_fingerprint = require_sealed_chapter_authority(&manifest, number)?
            .authority_root_fingerprint
            .clone();
        let truth_validation =
            write_truth_validation_record(&project_dir, &mut manifest, &chapter, &content).await?;
        let mut quality_gate =
            chapter_quality_gate(&manifest, &chapter, &content, &truth_validation.issues);
        let metadata_gate = chapter_metadata_gate(&manifest, &chapter, &content);
        let duplicate_issues =
            cross_chapter_duplicate_issues(&project_dir, &manifest, &chapter, &content).await;
        route_cross_chapter_duplicate_issues(&mut quality_gate, duplicate_issues);
        let review = build_local_review_record(
            args,
            &chapter,
            &content,
            &authority_fingerprint,
            &quality_gate,
        );
        let verdict = review.verdict.clone();
        tokio::fs::create_dir_all(project_dir.join("reviews")).await?;
        let review_path = project_dir.join("reviews").join(format!(
            "chapter-{number:04}-audit-{:04}.md",
            manifest
                .reviews
                .iter()
                .filter(|review| review.chapter_number == number)
                .count()
                + 1
        ));
        atomic_write_file(review_path.clone(), render_review_file(&review)).await?;
        if let Some(chapter) = manifest
            .chapters
            .iter_mut()
            .find(|chapter| chapter.number == number)
        {
            chapter.status = if verdict == "passed" {
                chapter_lifecycle::ChapterLifecycleStatus::ReviewPassed
                    .as_str()
                    .to_string()
            } else {
                chapter_lifecycle::ChapterLifecycleStatus::NeedsRevision
                    .as_str()
                    .to_string()
            };
            chapter.updated_at = now_iso();
        }
        manifest.reviews.push(review.clone());
        let review_cycle = write_review_cycle_record(
            &project_dir,
            &mut manifest,
            number,
            &verdict,
            &review.issues,
        )
        .await?;
        let hook_debt = write_hook_debt_report_record(&project_dir, &mut manifest, number).await?;
        let stage_authority = write_stage_authority_record(
            &project_dir,
            &manifest,
            number,
            "audit",
            governance::AuthorityRole::Auditor,
            &review.chapter_fingerprint,
        )
        .await?;
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        let durable_progress = durable_chapter_progress(&project_dir, &manifest).await;
        let next_action =
            review_cycle_next_action(&review_cycle, &verdict, metadata_gate.needs_repair());

        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.reviewed",
            "stage": pipeline::NovelPhase::Audit,
            "next_action": next_action,
            "writing_policy": policy::fiction_stage_policy(
                pipeline::NovelPhase::Audit.as_str(),
                &next_action,
            ),
            "project_path": project_dir.to_string_lossy(),
            "artifact_path": review_path.to_string_lossy(),
            "review_path": review_path.to_string_lossy(),
            "review": review,
            "quality_gate": quality_gate,
            "metadata_gate": metadata_gate,
            "review_cycle": review_cycle,
            "truth_validation": truth_validation,
            "hook_debt": hook_debt,
            "stage_authority": stage_authority,
            "state": apply_durable_chapter_progress(
                project_state_summary(&manifest),
                &manifest,
                &durable_progress,
            ),
            "audit": audit_manifest(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn revise_chapter(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        ensure_text_size(&args.content, "content")?;
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let number = args
            .chapter_number
            .or_else(|| latest_chapter_number(&manifest))
            .ok_or_else(|| anyhow::anyhow!("no chapter exists in this project"))?;
        let chapter_index = manifest
            .chapters
            .iter()
            .position(|chapter| chapter.number == number)
            .ok_or_else(|| anyhow::anyhow!("chapter {number} not found"))?;
        if args.action == "revise_draft" {
            require_sealed_chapter_authority(&manifest, number)?;
        }
        let mut chapter = manifest.chapters[chapter_index].clone();
        let chapter_path = project_dir.join(&chapter.path);
        let existing = tokio::fs::read_to_string(&chapter_path)
            .await
            .unwrap_or_default();
        let existing_body =
            normalize_chapter_body_for_record(&strip_frontmatter(&existing), &chapter.title);
        let metadata_revision = !args.summary.trim().is_empty()
            || !args.key_facts.is_empty()
            || !args.continuity_updates.is_empty()
            || !args.chapter_title.trim().is_empty()
            || !args.status.trim().is_empty()
            || !args.revision_notes.trim().is_empty()
            || !args.feedback.trim().is_empty();
        let revised_content = if args.content.trim().is_empty() {
            if !metadata_revision {
                anyhow::bail!("content or revision metadata is required for revise_chapter");
            }
            existing_body
        } else {
            sanitize_saved_prose(&args.content)
        };
        if !args.chapter_title.trim().is_empty() {
            chapter.title = args.chapter_title.trim().to_string();
        }
        if !args.summary.trim().is_empty() {
            let summary = compact_chapter_summary(args.summary.trim(), &manifest.language);
            chapter.summary = repair_contract_character_name_typos(&manifest, &summary);
        }
        let new_key_facts = clean_list(&args.key_facts);
        if !new_key_facts.is_empty() {
            chapter.key_facts = clean_contract_character_name_typos(
                &manifest,
                compact_truth_items(new_key_facts, CHAPTER_FACT_LIMIT),
            );
        }
        let continuity_updates = clean_list(&args.continuity_updates);
        if !continuity_updates.is_empty() {
            chapter.continuity_updates = clean_contract_character_name_typos(
                &manifest,
                compact_truth_items(continuity_updates, CHAPTER_CONTINUITY_LIMIT),
            );
        }
        let revised_content = normalize_chapter_body_for_record(&revised_content, &chapter.title);
        let revised_content = sanitize_chinese_script_noise(&manifest, &revised_content);
        let revised_content = repair_contract_character_name_typos(&manifest, &revised_content);
        normalize_chapter_metadata_against_body(&manifest, &mut chapter, &revised_content);
        if args.content.trim().is_empty() && metadata_revision {
            apply_explicit_chapter_metadata_args(&manifest, &mut chapter, args);
        }
        chapter.unit_count = count_units(&revised_content, &manifest.language);
        chapter.status = chapter_lifecycle::ChapterLifecycleStatus::Draft
            .as_str()
            .to_string();
        chapter.updated_at = now_iso();
        let chapter_record = chapter.clone();
        if args.candidate_only {
            let truth_validation = governance::validate_truth_against_chapter(
                chapter_record.number,
                &revised_content,
                &chapter_record.key_facts,
                &chapter_record.continuity_updates,
                now_iso(),
            );
            let (quality_gate, metadata_gate) = evaluate_revision_gates(
                &project_dir,
                &manifest,
                &chapter_record,
                &revised_content,
                &truth_validation.issues,
            )
            .await;
            let accepted = quality_gate.passed && !metadata_gate.needs_repair();
            let outcome_status = chapter_outcome_status(&quality_gate, &metadata_gate);
            let quality_decision =
                chapter_quality::chapter_quality_decision(&quality_gate, &metadata_gate);
            return Ok(json!({
                "success": true,
                "candidate_only": true,
                "accepted": accepted,
                "outcome_status": outcome_status,
                "requires_followup": !accepted,
                "runtime_effect": "artifact.candidate_evaluated",
                "project_path": project_dir.to_string_lossy(),
                "artifact_path": project_dir.join(&chapter_record.path).to_string_lossy(),
                "candidate_body": revised_content,
                "unit_count": chapter_record.unit_count,
                "chapter": chapter_record,
                "quality_gate": quality_gate,
                "metadata_gate": metadata_gate,
                "quality_decision": quality_decision,
                "truth_validation": truth_validation,
                "read_only": true
            }));
        }
        if !existing.is_empty() {
            archive_chapter_content(&project_dir, number, existing).await?;
        }
        write_chapter_record(&project_dir, &chapter_record, &revised_content).await?;
        append_continuity(&project_dir, &chapter_record).await?;
        manifest.chapters[chapter_index] = chapter_record.clone();
        refresh_continuity_truth_file(&project_dir, &mut manifest).await?;
        let truth_validation = write_truth_validation_record(
            &project_dir,
            &mut manifest,
            &chapter_record,
            &revised_content,
        )
        .await?;
        let (quality_gate, metadata_gate) = evaluate_revision_gates(
            &project_dir,
            &manifest,
            &chapter_record,
            &revised_content,
            &truth_validation.issues,
        )
        .await;
        let metadata_only = args.content.trim().is_empty();
        if !metadata_only {
            mark_pending_settlement_stale(
                &project_dir,
                number,
                "final chapter body changed after settlement",
            )
            .await?;
        }
        let quality_allows_write_receipt = metadata_only || quality_gate.passed;
        let mut output_chapter_record = chapter_record.clone();
        if !quality_allows_write_receipt {
            chapter.status = chapter_lifecycle::ChapterLifecycleStatus::NeedsRevision
                .as_str()
                .to_string();
            manifest.chapters[chapter_index] = chapter.clone();
            write_chapter_record(&project_dir, &chapter, &revised_content).await?;
            output_chapter_record = chapter.clone();
        }
        let hook_debt = write_hook_debt_report_record(&project_dir, &mut manifest, number).await?;
        let stage_authority = write_stage_authority_record(
            &project_dir,
            &manifest,
            number,
            "revision",
            governance::AuthorityRole::Reviser,
            &chapter_quality::chapter_body_fingerprint(&revised_content),
        )
        .await?;
        if !args.revision_notes.trim().is_empty() || !args.feedback.trim().is_empty() {
            let note = ReviewReceipt {
                chapter_number: number,
                chapter_fingerprint: chapter_quality::chapter_body_fingerprint(&revised_content),
                authority_fingerprint: require_sealed_chapter_authority(&manifest, number)?
                    .authority_root_fingerprint
                    .clone(),
                findings: Vec::new(),
                advisories: Vec::new(),
                score: None,
                locally_validated: false,
                verdict: "revision_note".to_string(),
                issues: Vec::new(),
                feedback: first_non_empty(&[args.revision_notes.as_str(), args.feedback.as_str()])
                    .to_string(),
                created_at: now_iso(),
            };
            manifest.reviews.push(note);
        }
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        let readable_export = self
            .sync_readable_txt_export(&project_dir, &manifest)
            .await?;
        let total_units = project_total_units(&manifest);
        let target_units = manifest.target_units;
        let durable_progress = durable_chapter_progress(&project_dir, &manifest).await;
        let target_reached = durable_project_target_reached(&manifest, &durable_progress);
        let checkpoint_only = target_units.is_some_and(|target| target > 0)
            && !target_reached
            && quality_allows_write_receipt;
        let runtime_effect = if quality_allows_write_receipt {
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

        Ok(json!({
            "success": true,
            "accepted": accepted,
            "outcome_status": outcome_status,
            "requires_followup": !accepted || metadata_needs_repair,
            "completion_gate": chapter_completion_gate_json(accepted, outcome_status),
            "runtime_effect": runtime_effect,
            "recoverable": !quality_allows_write_receipt,
            "completion_scope": if checkpoint_only { "checkpoint" } else { "artifact" },
            "project_path": project_dir.to_string_lossy(),
            "artifact_path": chapter_path.to_string_lossy(),
            "preferred_artifact_path": readable_export.current_path.to_string_lossy(),
            "txt_artifact_path": readable_export.current_path.to_string_lossy(),
            "txt_collection_path": readable_export.collection_path.to_string_lossy(),
            "readable_export": readable_export.to_json(),
            "metadata_only": metadata_only,
            "unit_count": output_chapter_record.unit_count,
            "total_units": total_units,
            "target_units": target_units,
            "target_reached": target_reached,
            "chapter": output_chapter_record,
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
            "next_action": if !quality_allows_write_receipt {
                "revise_chapter"
            } else if metadata_needs_repair {
                "repair_chapter_metadata"
            } else {
                "audit_chapter"
            },
            "next_step_hint": if quality_allows_write_receipt {
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

    pub(in crate::tool::writing::novel_studio) async fn approve_all(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let ready_numbers = manifest
            .chapters
            .iter()
            .filter(|chapter| !chapter_is_approved(chapter))
            .filter(|chapter| chapter_ready_for_approval(&manifest, chapter.number, chapter))
            .map(|chapter| chapter.number)
            .collect::<BTreeSet<_>>();
        let mut changed = 0usize;
        let mut approvals = Vec::new();
        for number in ready_numbers {
            let mut chapter_args = args.clone();
            chapter_args.chapter_number = Some(number);
            let result = self.approve_chapter_transaction(&chapter_args).await?;
            if result
                .get("success")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                changed += 1;
            }
            approvals.push(result);
        }
        let manifest = self.read_manifest(&project_dir).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "approved_count": changed,
            "approvals": approvals,
            "state": project_state_summary(&manifest),
            "audit": audit_manifest(&manifest)
        }))
    }
}
