use super::*;
use crate::tool::writing::novel_studio::chapter_io::sync_final_chapter_title_to_support_records;

impl NovelStudioTool {
    pub(in crate::tool::writing::novel_studio) async fn settle_chapter_state(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let chapter_number = args
            .chapter_number
            .or_else(|| latest_chapter_number(&manifest))
            .ok_or_else(|| anyhow::anyhow!("no chapter exists in this project"))?;
        let chapter = manifest
            .chapters
            .iter()
            .find(|chapter| chapter.number == chapter_number)
            .ok_or_else(|| anyhow::anyhow!("chapter {chapter_number} not found"))?
            .clone();
        let content = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
        let body = normalize_chapter_body_for_record(&strip_frontmatter(&content), &chapter.title);
        let authority =
            read_sealed_chapter_authority(&project_dir, &manifest, chapter_number).await?;
        let (settlement, validation, settlement_source, observer_fallback_reason) =
            validated_settlement_from_final_body(&args.content, &body, &chapter, &authority);
        let settlement_path =
            write_pending_settlement(&project_dir, chapter_number, &settlement).await?;
        let stage_authority = write_stage_authority_record(
            &project_dir,
            &manifest,
            chapter_number,
            "settlement",
            governance::AuthorityRole::Observer,
            &settlement.chapter_fingerprint,
        )
        .await?;
        if let Some(chapter) = manifest
            .chapters
            .iter_mut()
            .find(|chapter| chapter.number == chapter_number)
        {
            let next_status = if validation.passed {
                Some(chapter_lifecycle::ChapterLifecycleStatus::StateReady)
            } else {
                Some(chapter_lifecycle::ChapterLifecycleStatus::StateRepairRequired)
            };
            if let Some(next_status) = next_status {
                chapter.status = next_status.as_str().to_string();
                chapter.updated_at = now_iso();
                manifest.updated_at = now_iso();
                self.write_manifest(&project_dir, &manifest).await?;
            }
        }
        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.checkpointed",
            "stage": pipeline::NovelPhase::TruthSettlement,
            "project_path": project_dir.to_string_lossy(),
            "chapter_number": chapter_number,
            "settlement_path": settlement_path.to_string_lossy(),
            "settlement": settlement,
            "stage_authority": stage_authority,
            "validation": validation,
            "settlement_source": settlement_source.as_str(),
            "observer_fallback_reason": observer_fallback_reason,
            "truth_updates": [],
            "commit_policy": "pending_until_chapter_approval",
            "chapter_status": if validation.passed {
                chapter_lifecycle::ChapterLifecycleStatus::StateReady.as_str()
            } else {
                chapter_lifecycle::ChapterLifecycleStatus::StateRepairRequired.as_str()
            },
            "next_action": if validation.passed { "approve_chapter" } else { "settle_chapter_state" }
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn validate_chapter_state(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let chapter_number = args
            .chapter_number
            .or_else(|| latest_chapter_number(&manifest))
            .ok_or_else(|| anyhow::anyhow!("no chapter exists in this project"))?;
        let chapter = manifest
            .chapters
            .iter()
            .find(|chapter| chapter.number == chapter_number)
            .ok_or_else(|| anyhow::anyhow!("chapter {chapter_number} not found"))?;
        let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
        let body = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
        let Some(settlement) = read_pending_settlement(&project_dir, chapter_number).await? else {
            return Ok(json!({
                "success": false,
                "read_only": true,
                "recoverable": true,
                "stage": pipeline::NovelPhase::TruthValidation,
                "project_path": project_dir.to_string_lossy(),
                "chapter_number": chapter_number,
                "error_kind": "state_settlement_missing",
                "next_action": "settle_chapter_state"
            }));
        };
        let authority =
            read_sealed_chapter_authority(&project_dir, &manifest, chapter_number).await?;
        let validation = validate_settlement_for_chapter(chapter, &body, &authority, &settlement);
        Ok(json!({
            "success": true,
            "read_only": true,
            "stage": pipeline::NovelPhase::TruthValidation,
            "project_path": project_dir.to_string_lossy(),
            "chapter_number": chapter_number,
            "validation": validation,
            "next_action": if validation.passed { "approve_chapter" } else { "settle_chapter_state" }
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn repair_project_state(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let migrated_legacy_candidates = self
            .reconstruct_legacy_unapproved_authorities(&project_dir, args.chapter_number)
            .await?;
        let original = self.read_manifest(&project_dir).await?;
        let mut rebuilt = original.clone();
        rebuild_story_bible_from_contract_only(&mut rebuilt);
        let mut approved = original
            .chapters
            .iter()
            .filter(|chapter| chapter_is_approved(chapter))
            .cloned()
            .collect::<Vec<_>>();
        approved.sort_by_key(|chapter| chapter.number);
        if let Some(number) = args.chapter_number.filter(|number| *number > 0) {
            approved.retain(|chapter| chapter.number <= number);
        }

        let mut blockers = Vec::new();
        let mut accepted = Vec::new();
        let mut migrated_legacy_receipts = Vec::new();
        let mut chapter_summary_lines = Vec::new();
        for chapter in &approved {
            let receipt =
                if let Some(receipt) = read_approval_receipt(&project_dir, chapter.number).await? {
                    receipt
                } else {
                    let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
                    let body =
                        normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
                    let receipt = ApprovalReceipt {
                        transaction_id: format!("legacy-history-{:04}", chapter.number),
                        chapter_number: chapter.number,
                        body_fingerprint: chapter_quality::chapter_body_fingerprint(&body),
                        metadata_fingerprint: approval_transaction::chapter_metadata_fingerprint(
                            chapter,
                        ),
                        authority_fingerprint: original
                            .context_packages
                            .iter()
                            .find(|record| record.number == chapter.number && record.sealed)
                            .map(|record| record.authority_root_fingerprint.clone())
                            .unwrap_or_default(),
                        review_fingerprint: String::new(),
                        settlement_fingerprint: String::new(),
                        truth_fingerprint: String::new(),
                        committed_at: now_iso(),
                        legacy: true,
                    };
                    write_approval_receipt(&project_dir, &receipt).await?;
                    migrated_legacy_receipts.push(chapter.number);
                    receipt
                };
            if receipt.legacy {
                blockers.push(format!(
                    "approved chapter {} is historical and has no fabricated typed audit/settlement; existing truth is preserved",
                    chapter.number
                ));
                continue;
            }
            let Some(settlement) = read_approved_settlement(&project_dir, chapter.number).await?
            else {
                blockers.push(format!(
                    "approved chapter {} has no approved settlement",
                    chapter.number
                ));
                continue;
            };
            let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
            let body = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
            let authority = match read_sealed_chapter_authority(
                &project_dir,
                &original,
                chapter.number,
            )
            .await
            {
                Ok(authority) => authority,
                Err(error) => {
                    blockers.push(format!(
                        "approved chapter {} authority is unavailable: {error}",
                        chapter.number
                    ));
                    continue;
                }
            };
            let validation =
                validate_settlement_for_chapter(chapter, &body, &authority, &settlement);
            let body_fingerprint = chapter_quality::chapter_body_fingerprint(&body);
            let metadata_fingerprint = governance::authority_fingerprint(&json!({
                "title": chapter.title,
                "summary": chapter.summary,
                "key_facts": chapter.key_facts,
                "continuity_updates": chapter.continuity_updates,
                "unit_count": chapter.unit_count
            }));
            if !validation.passed
                || receipt.body_fingerprint != body_fingerprint
                || receipt.metadata_fingerprint != metadata_fingerprint
                || receipt.authority_fingerprint != authority.authority_root_fingerprint
                || receipt.settlement_fingerprint != governance::authority_fingerprint(&settlement)
            {
                blockers.push(format!(
                    "approved chapter {} receipt/settlement dependency chain is invalid: {}",
                    chapter.number,
                    validation.warnings.join("; ")
                ));
                continue;
            }
            let character_registrations = original
                .chapter_contracts
                .iter()
                .find(|record| record.number == chapter.number)
                .map(|record| record.character_registrations.clone())
                .unwrap_or_default();
            let delta = novel_bible::ApprovedChapterDelta {
                number: chapter.number,
                title: chapter.title.clone(),
                summary: settlement.chapter_summary.clone(),
                unit_count: chapter.unit_count,
                key_facts: Vec::new(),
                continuity_updates: settlement.continuity_updates.clone(),
                character_registrations,
                state_changes: settlement.state_changes.clone(),
            };
            if let Some(bible) = rebuilt.story_bible.as_mut() {
                novel_bible::apply_approved_chapter_delta(bible, &delta, now_iso());
                bible.last_rebuilt_chapter = Some(chapter.number);
            }
            if !settlement.chapter_summary.trim().is_empty() {
                chapter_summary_lines.push(if runner::is_chinese_language(&rebuilt.language) {
                    format!("第{}章：{}", chapter.number, settlement.chapter_summary)
                } else {
                    format!("Chapter {}: {}", chapter.number, settlement.chapter_summary)
                });
            }
            accepted.push(chapter.number);
        }

        if !blockers.is_empty() {
            return Ok(json!({
                "success": false,
                "recoverable": true,
                "runtime_effect": "artifact.repair_blocked",
                "project_path": project_dir.to_string_lossy(),
                "repaired_chapters": [],
                "accepted_receipts": accepted,
                "migrated_legacy_receipts": migrated_legacy_receipts,
                "migrated_legacy_candidates": migrated_legacy_candidates,
                "integrity_blockers": blockers,
                "old_truth_preserved": true,
                "next_action": "migrate_or_repair_approval_dependencies"
            }));
        }

        let mut truth_updates = Vec::new();
        if let Some(current_state) = rebuilt
            .story_bible
            .as_ref()
            .map(novel_bible::approved_state_truth)
        {
            truth_updates.push(
                write_truth_section_direct(
                    &project_dir,
                    &mut rebuilt,
                    "current_state",
                    &current_state,
                )
                .await?,
            );
        }
        if !chapter_summary_lines.is_empty() {
            truth_updates.push(
                write_truth_section_direct(
                    &project_dir,
                    &mut rebuilt,
                    "chapter_summaries",
                    &chapter_summary_lines.join("\n"),
                )
                .await?,
            );
        }
        if let Some(pending_hooks) = rebuilt
            .story_bible
            .as_ref()
            .map(novel_bible::pending_hook_truth)
        {
            truth_updates.push(
                write_truth_section_direct(
                    &project_dir,
                    &mut rebuilt,
                    "pending_hooks",
                    &pending_hooks,
                )
                .await?,
            );
        }
        refresh_continuity_truth_file(&project_dir, &mut rebuilt).await?;
        rebuilt.updated_at = now_iso();
        self.write_manifest(&project_dir, &rebuilt).await?;
        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.repaired, artifact.verified",
            "project_path": project_dir.to_string_lossy(),
            "repaired_chapters": accepted,
            "migrated_legacy_receipts": migrated_legacy_receipts,
            "migrated_legacy_candidates": migrated_legacy_candidates,
            "truth_updates": truth_updates,
            "integrity_blockers": [],
            "state": project_state_summary(&rebuilt),
            "next_action": "run_next_chapter"
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn repair_latest_chapter_metadata(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let Some(number) = args
            .chapter_number
            .filter(|number| *number > 0)
            .or_else(|| manifest.chapters.iter().map(|chapter| chapter.number).max())
        else {
            return Ok(json!({
                "success": true,
                "runtime_effect": "artifact.verified",
                "project_path": project_dir.to_string_lossy(),
                "repaired_chapters": []
            }));
        };
        let Some(index) = manifest
            .chapters
            .iter()
            .position(|chapter| chapter.number == number)
        else {
            anyhow::bail!("chapter {number} not found")
        };

        let before = manifest.chapters[index].clone();
        let raw = tokio::fs::read_to_string(project_dir.join(&before.path))
            .await
            .unwrap_or_default();
        let body = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &before.title);
        let mut repaired = before.clone();
        if !args.chapter_title.trim().is_empty() {
            repaired.title = args.chapter_title.trim().to_string();
        } else {
            let repaired_title = final_chapter_title_from_body_with_metadata(
                &manifest,
                number,
                &before.title,
                &before.summary,
                &before.key_facts,
                &before.continuity_updates,
                &body,
            );
            if repaired_title != before.title {
                repaired.title = repaired_title;
            }
        }
        if !args.summary.trim().is_empty() {
            repaired.summary = compact_chapter_summary(args.summary.trim(), &manifest.language);
        }
        let key_facts = clean_list(&args.key_facts);
        if !key_facts.is_empty() {
            repaired.key_facts = compact_truth_items(key_facts, CHAPTER_FACT_LIMIT);
        }
        let continuity_updates = clean_list(&args.continuity_updates);
        if !continuity_updates.is_empty() {
            repaired.continuity_updates =
                compact_truth_items(continuity_updates, CHAPTER_CONTINUITY_LIMIT);
        }
        repaired.summary = repair_contract_character_name_typos(&manifest, &repaired.summary);
        repaired.key_facts = clean_contract_character_name_typos(&manifest, repaired.key_facts);
        repaired.continuity_updates =
            clean_contract_character_name_typos(&manifest, repaired.continuity_updates);
        normalize_chapter_metadata_against_body(&manifest, &mut repaired, &body);
        let title_after_body = final_chapter_title_from_body_with_metadata(
            &manifest,
            number,
            &repair_contract_character_name_typos(&manifest, &repaired.title),
            &repaired.summary,
            &repaired.key_facts,
            &repaired.continuity_updates,
            &body,
        );
        if title_after_body != repaired.title {
            repaired.title = title_after_body;
        }
        repaired.updated_at = now_iso();
        let metadata_gate = chapter_metadata_gate(&manifest, &repaired, &body);
        if args.candidate_only {
            let quality_gate = chapter_quality_gate(&manifest, &repaired, &body, &[]);
            return Ok(json!({
                "success": true,
                "candidate_only": true,
                "runtime_effect": "artifact.candidate_repaired",
                "project_path": project_dir.to_string_lossy(),
                "chapter_number": number,
                "chapter": repaired,
                "candidate_body": body,
                "quality_gate": quality_gate,
                "metadata_gate": metadata_gate,
                "truth_validation": {
                    "passed": true,
                    "issues": [],
                    "read_only": true
                },
                "read_only": true
            }));
        }
        let truth_validation =
            write_truth_validation_record(&project_dir, &mut manifest, &repaired, &body).await?;
        let quality_gate =
            chapter_quality_gate(&manifest, &repaired, &body, &truth_validation.issues);
        if repaired.title == before.title
            && repaired.summary == before.summary
            && repaired.key_facts == before.key_facts
            && repaired.continuity_updates == before.continuity_updates
        {
            return Ok(json!({
                "success": true,
                "runtime_effect": "artifact.verified",
                "project_path": project_dir.to_string_lossy(),
                "chapter_number": number,
                "repaired_chapters": [],
                "quality_gate": quality_gate,
                "metadata_gate": metadata_gate,
                "truth_validation": truth_validation
            }));
        }

        manifest.chapters[index] = repaired.clone();
        sync_chapter_record_file(&project_dir, &manifest.chapters[index]).await?;
        sync_final_chapter_title_to_support_records(
            &project_dir,
            &mut manifest,
            number,
            &repaired.title,
        )
        .await?;
        self.write_manifest(&project_dir, &manifest).await?;
        self.sync_readable_txt_export(&project_dir, &manifest)
            .await?;
        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.verified",
            "project_path": project_dir.to_string_lossy(),
            "chapter_number": number,
            "repaired_chapters": [{
                "chapter_number": number,
                "status": "repaired",
                "previous_title": before.title,
                "title": repaired.title,
                "summary": repaired.summary,
                "key_facts": repaired.key_facts,
                "continuity_updates": repaired.continuity_updates
            }],
            "quality_gate": quality_gate,
            "metadata_gate": metadata_gate,
            "truth_validation": truth_validation,
            "state": project_state_summary(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn apply_pending_settlement_to_truth(
        &self,
        project_dir: &Path,
        manifest: &mut NovelProjectManifest,
        chapter_number: usize,
        settlement: &SettlementOutput,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let mut truth_results = Vec::new();
        if !settlement.chapter_summary.trim().is_empty() {
            let merged = self
                .merged_chapter_summaries(project_dir, manifest, chapter_number, settlement)
                .await?;
            truth_results.push(
                write_truth_section_direct(project_dir, manifest, "chapter_summaries", &merged)
                    .await?,
            );
        }
        Ok(truth_results)
    }

    pub(in crate::tool::writing::novel_studio) async fn assigned_worker_policy_packet(
        &self,
        args: &NovelStudioArgs,
        action: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        self.ensure_project_scaffold(&project_dir).await?;
        let manifest = self.read_manifest(&project_dir).await?;
        let durable_progress = durable_chapter_progress(&project_dir, &manifest).await;
        let chapter_number = match args.chapter_number {
            Some(number) => number,
            None => durable_progress.next_chapter,
        };
        let context = build_context_payload(&project_dir, &manifest, chapter_number).await?;
        let prompt_context = build_prompt_context_payload(&context);
        let context_json = serde_json::to_string(&prompt_context)?;
        let previous_truth = self.render_truth_snapshot(&project_dir, &manifest).await?;
        let plan = manifest
            .chapter_plans
            .iter()
            .find(|plan| plan.number == chapter_number);
        let memo = plan
            .and_then(|plan| runner::parse_memo(&plan.plan, &manifest.language).ok())
            .unwrap_or_else(|| runner::ChapterMemo {
                goal: plan
                    .map(|plan| plan.title.clone())
                    .unwrap_or_else(|| default_chapter_title(&manifest.language, chapter_number)),
                body: plan
                    .map(|plan| plan.plan.clone())
                    .unwrap_or_else(|| "No chapter plan has been persisted yet.".to_string()),
                sections: Vec::new(),
            });
        let architecture = manifest
            .chapter_architectures
            .iter()
            .find(|item| item.number == chapter_number)
            .map(|item| item.architecture.as_str())
            .unwrap_or("No chapter architecture has been persisted yet.");
        let existing_chapter = manifest
            .chapters
            .iter()
            .find(|chapter| chapter.number == chapter_number);
        let existing_body = match existing_chapter {
            Some(chapter) => tokio::fs::read_to_string(project_dir.join(&chapter.path))
                .await
                .map(|content| strip_frontmatter(&content))
                .unwrap_or_default(),
            None => String::new(),
        };
        let deterministic_issues = existing_chapter
            .map(|chapter| mechanical_chapter_issues(&manifest, chapter, &existing_body))
            .unwrap_or_default();
        let existing_body_reference = policy_packet_chapter_body_reference(existing_chapter);
        let mut stage_prompts = json!({
            "current_stage": pipeline::NovelPhase::ChapterExecutionPackage,
            "prompt_refs": {
                "context_json_chars": context_json.chars().count(),
                "previous_truth_chars": previous_truth.chars().count(),
                "memo_chars": memo.body.chars().count(),
                "architecture_chars": architecture.chars().count(),
                "existing_body": existing_body_reference,
                "deterministic_issues": deterministic_issues
            },
            "stage_sequence": pipeline::phase_ids(&pipeline::NovelPhase::CHAPTER_LOOP),
            "note": "Prompt text is generated on demand by the workflow driver for the current stage; this packet carries refs and budgets only."
        });
        let contract_missing = manifest.contract.is_none();
        let readiness_blockers = governed_project_readiness_blockers(&manifest);
        let integrity_blockers =
            approved_chapter_integrity_blockers(&project_dir, &manifest).await?;
        let needs_contract_repair = contract_missing || !readiness_blockers.is_empty();
        let needs_project_repair = !integrity_blockers.is_empty();
        let settlement_ready = pending_settlement_path(&project_dir, chapter_number).exists();
        let export_ready = project_dir.join("exports/current.txt").exists()
            || project_dir.join("exports/current.md").exists();
        let transition = project_pipeline_transition(
            &manifest,
            chapter_number,
            settlement_ready,
            durable_project_target_reached(&manifest, &durable_progress),
            export_ready,
        );
        let pipeline_next_action = transition
            .next_phase
            .map(pipeline::NovelPhase::tool_action)
            .unwrap_or("run_next_chapter");
        let pipeline_stage = transition
            .next_phase
            .map(pipeline::NovelPhase::as_str)
            .unwrap_or("chapter_complete");
        stage_prompts["current_stage"] = json!(pipeline_stage);
        Ok(json!({
            "success": !needs_contract_repair && !needs_project_repair,
            "recoverable": needs_contract_repair || needs_project_repair,
            "read_only": true,
            "stage": pipeline_stage,
            "packet_kind": "assigned_worker_policy_packet",
            "action": action,
            "project_path": project_dir.to_string_lossy(),
            "chapter_number": chapter_number,
            "state": apply_durable_chapter_progress(
                project_state_summary(&manifest),
                &manifest,
                &durable_progress,
            ),
            "audit": audit_manifest(&manifest),
            "context": context,
            "prompt_context": prompt_context,
            "contract_required": needs_contract_repair,
            "contract_blockers": readiness_blockers,
            "project_integrity_blockers": integrity_blockers,
            "next_action": if needs_contract_repair {
                "set_contract"
            } else if needs_project_repair {
                "repair_project_state"
            } else {
                pipeline_next_action
            },
            "pipeline_transition": {
                "next_phase": transition.next_phase,
                "reason": transition.reason,
            },
            "pipeline": pipeline::action_ids(&pipeline::NovelPhase::ALL),
            "pipeline_contract": {
                "schema_version": pipeline::PIPELINE_CONTRACT_VERSION,
                "owner": "equipped_writer_worker",
                "model_boundary": "Use the worker runtime model. The tool only persists state and returns phase contracts; it must not open a private provider session.",
                "workflow_driver": pipeline::novel_workflow_descriptor_json(),
                "chapter_loop": pipeline::action_ids(&pipeline::NovelPhase::CHAPTER_LOOP),
                "chat_visibility": "Return progress, paths, summaries, and quality status. Keep full prose in project artifacts unless the user explicitly asks to view an excerpt."
            },
            "assigned_worker_policy_packet": stage_prompts.clone(),
            "worker_stage_prompts": stage_prompts,
            "next_step_hint": if needs_contract_repair {
                "BenShu should delegate this stage to the equipped writer worker; that worker must infer or repair a complete story contract with ending, character core anchors, world rules, style rules, and outline before drafting."
            } else if needs_project_repair {
                "BenShu should ask the equipped writer worker to run repair_project_state before drafting; approved chapters with identity drift must be revised before they can feed future context."
            } else {
                "BenShu should keep this stage on the equipped writer worker; that worker uses its model context to execute the next stage and persists each result through explicit novel_studio actions."
            }
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn render_truth_snapshot(
        &self,
        project_dir: &Path,
        manifest: &NovelProjectManifest,
    ) -> anyhow::Result<String> {
        let mut out = String::new();
        for truth in &manifest.truth_files {
            let content = tokio::fs::read_to_string(project_dir.join(&truth.path))
                .await
                .unwrap_or_default();
            out.push_str(&format!("## {}\n{}\n\n", truth.section, content));
        }
        Ok(out)
    }

    pub(in crate::tool::writing::novel_studio) async fn read_truth_section_content(
        &self,
        project_dir: &Path,
        manifest: &NovelProjectManifest,
        section: &str,
    ) -> anyhow::Result<String> {
        let Some(record) = manifest
            .truth_files
            .iter()
            .find(|truth| truth.section.eq_ignore_ascii_case(section))
        else {
            return Ok(String::new());
        };
        let raw = tokio::fs::read_to_string(project_dir.join(&record.path)).await?;
        Ok(truth_file_body(&record.section, &raw))
    }

    pub(in crate::tool::writing::novel_studio) async fn merged_chapter_summaries(
        &self,
        project_dir: &Path,
        manifest: &NovelProjectManifest,
        chapter_number: usize,
        settlement: &SettlementOutput,
    ) -> anyhow::Result<String> {
        let existing = self
            .read_truth_section_content(project_dir, manifest, "chapter_summaries")
            .await
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        let mut lines = existing
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| !line.starts_with('#'))
            .filter(|line| !line.contains(&format!("Chapter {chapter_number}:")))
            .filter(|line| !line.contains(&format!("第{chapter_number}章：")))
            .filter_map(|line| {
                let line = truncate_compact_text(line, TRUTH_SUMMARY_LINE_MAX_CHARS);
                if seen.insert(line.clone()) {
                    Some(line)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let summary =
            truncate_compact_text(&settlement.chapter_summary, TRUTH_SUMMARY_LINE_MAX_CHARS);
        let label = if runner::is_chinese_language(&manifest.language) {
            format!("第{chapter_number}章：{summary}")
        } else {
            format!("Chapter {chapter_number}: {summary}")
        };
        lines.push(label);
        Ok(lines.join("\n"))
    }
}
