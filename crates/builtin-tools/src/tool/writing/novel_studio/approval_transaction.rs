use super::*;

impl NovelStudioTool {
    pub(super) async fn reject_chapter_transaction(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let number = args
            .chapter_number
            .or_else(|| latest_chapter_number(&manifest))
            .ok_or_else(|| anyhow::anyhow!("no chapter exists in this project"))?;
        let index = manifest
            .chapters
            .iter()
            .position(|chapter| chapter.number == number)
            .ok_or_else(|| anyhow::anyhow!("chapter {number} not found"))?;
        if chapter_is_approved(&manifest.chapters[index]) {
            return Ok(approval_blocked(
                &project_dir,
                number,
                "approved_chapter_is_immutable",
                "an approved chapter cannot be rejected without an explicit administrative migration",
                "status",
            ));
        }
        manifest.chapters[index].status = chapter_lifecycle::ChapterLifecycleStatus::Rejected
            .as_str()
            .to_string();
        manifest.chapters[index].updated_at = now_iso();
        let chapter = manifest.chapters[index].clone();
        discard_chapter_character_registrations(&mut manifest, number);
        sync_chapter_record_file(&project_dir, &chapter).await?;
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.rejected",
            "project_path": project_dir.to_string_lossy(),
            "chapter": chapter,
            "state": project_state_summary(&manifest),
            "audit": audit_manifest(&manifest)
        }))
    }

    pub(super) async fn approve_chapter_transaction(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
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
        let chapter = manifest.chapters[chapter_index].clone();
        let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
        let body = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
        let body_fingerprint = chapter_quality::chapter_body_fingerprint(&body);
        let authority = read_sealed_chapter_authority(&project_dir, &manifest, number).await?;
        let authority_fingerprint = authority.authority_root_fingerprint.clone();

        if let Some(receipt) = read_approval_receipt(&project_dir, number).await? {
            let metadata_fingerprint = chapter_metadata_fingerprint(&chapter);
            let settlement_matches = read_approved_settlement(&project_dir, number)
                .await?
                .is_some_and(|settlement| {
                    governance::authority_fingerprint(&settlement) == receipt.settlement_fingerprint
                });
            let review_matches =
                latest_passing_review(&manifest, number, &body_fingerprint, &authority_fingerprint)
                    .is_some_and(|review| {
                        governance::authority_fingerprint(review) == receipt.review_fingerprint
                    });
            let is_latest_approved = manifest
                .chapters
                .iter()
                .filter(|candidate| chapter_is_approved(candidate))
                .map(|candidate| candidate.number)
                .max()
                == Some(number);
            let truth_matches = !is_latest_approved
                || (!receipt.truth_fingerprint.is_empty()
                    && receipt.truth_fingerprint == approval_truth_fingerprint(&manifest));
            if receipt.body_fingerprint != body_fingerprint
                || receipt.authority_fingerprint != authority_fingerprint
                || receipt.metadata_fingerprint != metadata_fingerprint
                || !settlement_matches
                || !review_matches
                || !truth_matches
            {
                return Ok(approval_blocked(
                    &project_dir,
                    number,
                    "approval_receipt_stale",
                    "the existing approval receipt does not match the current body and authority",
                    "restore_or_explicitly_reopen_chapter",
                ));
            }
            if let Some(mut journal) = read_approval_journal(&project_dir, number).await? {
                if journal.transaction_id != receipt.transaction_id
                    || journal.body_fingerprint != receipt.body_fingerprint
                    || journal.authority_fingerprint != receipt.authority_fingerprint
                {
                    return Ok(approval_blocked(
                        &project_dir,
                        number,
                        "approval_transaction_stale",
                        "the approval receipt and transaction journal do not describe the same commit",
                        "restore_or_explicitly_reopen_chapter",
                    ));
                }
                if journal.state == ApprovalJournalState::Prepared {
                    journal.state = ApprovalJournalState::Committed;
                    journal.committed_at = receipt.committed_at.clone();
                    journal.receipt_path = relative_project_path(
                        &project_dir,
                        &approval_receipt_path(&project_dir, number),
                    );
                    write_approval_journal(&project_dir, &journal).await?;
                }
                cleanup_approval_backup(&project_dir, &journal).await?;
            }
            return Ok(json!({
                "success": true,
                "idempotent_replay": true,
                "runtime_effect": "artifact.verified",
                "project_path": project_dir.to_string_lossy(),
                "chapter": chapter,
                "approval_receipt": receipt,
                "state": project_state_summary(&manifest),
                "audit": audit_manifest(&manifest)
            }));
        }

        if let Some(journal) = read_approval_journal(&project_dir, number).await? {
            if journal.body_fingerprint != body_fingerprint
                || journal.authority_fingerprint != authority_fingerprint
            {
                return Ok(approval_blocked(
                    &project_dir,
                    number,
                    "approval_transaction_stale",
                    "a prepared approval transaction belongs to a different body or authority",
                    "audit_chapter",
                ));
            }
            if journal.state == ApprovalJournalState::Prepared
                && chapter_lifecycle::ChapterLifecycleStatus::parse(&chapter.status)
                    == chapter_lifecycle::ChapterLifecycleStatus::Approved
            {
                let settlement = read_approved_settlement(&project_dir, number)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "prepared approval {number} committed its manifest without settlement"
                        )
                    })?;
                let review = latest_passing_review(
                    &manifest,
                    number,
                    &body_fingerprint,
                    &authority_fingerprint,
                )
                .ok_or_else(|| anyhow::anyhow!("prepared approval lost its review dependency"))?;
                if !accepted_best_candidate_matches(
                    &project_dir,
                    number,
                    &authority_fingerprint,
                    &body_fingerprint,
                )
                .await?
                {
                    anyhow::bail!("prepared approval lost its accepted best candidate dependency");
                }
                let validation =
                    validate_settlement_for_chapter(&chapter, &body, &authority, &settlement);
                if !validation.passed {
                    anyhow::bail!("prepared approval settlement no longer validates against final body and authority");
                }
                let truth_matches = manifest
                    .truth_validations
                    .iter()
                    .rev()
                    .find(|record| record.chapter_number == number)
                    .is_some_and(|record| {
                        record.verdict == "passed"
                            && record.issues.is_empty()
                            && record.chapter_fingerprint == body_fingerprint
                    });
                if !truth_matches {
                    anyhow::bail!("prepared approval lost its deterministic truth validation");
                }
                let receipt = build_approval_receipt(
                    &journal.transaction_id,
                    &chapter,
                    &body_fingerprint,
                    &authority_fingerprint,
                    review,
                    &settlement,
                    &approval_truth_fingerprint(&manifest),
                );
                write_approval_receipt(&project_dir, &receipt).await?;
                let mut committed = journal;
                committed.state = ApprovalJournalState::Committed;
                committed.committed_at = receipt.committed_at.clone();
                committed.receipt_path = relative_project_path(
                    &project_dir,
                    &approval_receipt_path(&project_dir, number),
                );
                write_approval_journal(&project_dir, &committed).await?;
                cleanup_approval_backup(&project_dir, &committed).await?;
                return Ok(json!({
                    "success": true,
                    "recovered_prepared_transaction": true,
                    "runtime_effect": "artifact.recovered",
                    "project_path": project_dir.to_string_lossy(),
                    "chapter": chapter,
                    "approval_receipt": receipt,
                    "state": project_state_summary(&manifest),
                    "audit": audit_manifest(&manifest)
                }));
            }
            if journal.state == ApprovalJournalState::Prepared
                && chapter_lifecycle::ChapterLifecycleStatus::parse(&chapter.status)
                    != chapter_lifecycle::ChapterLifecycleStatus::Approved
                && !journal.backup_path.trim().is_empty()
            {
                let backup = project_dir.join(&journal.backup_path);
                if backup.exists() {
                    snapshot::copy_project_state_from_snapshot(&backup, &project_dir).await?;
                    let backup_root = backup.parent().map(Path::to_path_buf);
                    if let Some(backup_root) = backup_root {
                        if backup_root.exists() {
                            tokio::fs::remove_dir_all(backup_root).await?;
                        }
                    }
                    return Ok(json!({
                        "success": false,
                        "recoverable": true,
                        "recovered_prepared_transaction": true,
                        "runtime_effect": "artifact.rolled_back",
                        "project_path": project_dir.to_string_lossy(),
                        "chapter_number": number,
                        "error_kind": "approval_transaction_rolled_back",
                        "error": "an interrupted approval was restored to its complete pre-commit state",
                        "next_action": "approve_chapter"
                    }));
                }
            }
        }

        let current_status = chapter_lifecycle::ChapterLifecycleStatus::parse(&chapter.status);
        if !matches!(
            current_status,
            chapter_lifecycle::ChapterLifecycleStatus::ReviewPassed
                | chapter_lifecycle::ChapterLifecycleStatus::StateReady
        ) {
            return Ok(approval_blocked(
                &project_dir,
                number,
                "invalid_chapter_lifecycle_transition",
                &format!(
                    "chapter {number} cannot transition from {} to approved",
                    current_status.as_str()
                ),
                if current_status == chapter_lifecycle::ChapterLifecycleStatus::StateRepairRequired
                {
                    "settle_chapter_state"
                } else {
                    "audit_chapter"
                },
            ));
        }

        let Some(review) =
            latest_passing_review(&manifest, number, &body_fingerprint, &authority_fingerprint)
                .cloned()
        else {
            return Ok(approval_blocked(
                &project_dir,
                number,
                "approval_requires_current_review",
                "the latest locally validated review does not match the current body and authority",
                "audit_chapter",
            ));
        };
        let truth_matches = manifest
            .truth_validations
            .iter()
            .rev()
            .find(|record| record.chapter_number == number)
            .is_some_and(|record| {
                record.verdict == "passed"
                    && record.issues.is_empty()
                    && record.chapter_fingerprint == body_fingerprint
            });
        if !truth_matches {
            return Ok(approval_blocked(
                &project_dir,
                number,
                "approval_requires_current_truth_validation",
                "the current body has no passing deterministic truth validation",
                "audit_chapter",
            ));
        }

        if !accepted_best_candidate_matches(
            &project_dir,
            number,
            &authority_fingerprint,
            &body_fingerprint,
        )
        .await?
        {
            return Ok(approval_blocked(
                &project_dir,
                number,
                "approval_requires_accepted_best_candidate",
                "the current body is not backed by a durable accepted_as_best candidate bound to this authority",
                "run_next_chapter",
            ));
        }

        let Some(settlement) = read_pending_settlement(&project_dir, number).await? else {
            return Ok(approval_blocked(
                &project_dir,
                number,
                "approval_requires_state_settlement",
                "the current final body has no pending state settlement",
                "settle_chapter_state",
            ));
        };
        let validation = validate_settlement_for_chapter(&chapter, &body, &authority, &settlement);
        if !validation.passed {
            return Ok(json!({
                "success": false,
                "recoverable": true,
                "error_kind": "approval_requires_valid_state_settlement",
                "error": "pending settlement does not match the current final body and sealed authority",
                "project_path": project_dir.to_string_lossy(),
                "chapter_number": number,
                "validation": validation,
                "next_action": "settle_chapter_state"
            }));
        }

        let (final_chapter, settlement, metadata_gate) =
            settlement_display_metadata_or_body_validated_best(
                &manifest,
                &chapter,
                &settlement,
                &body,
            );
        if metadata_gate.needs_repair() {
            return Ok(json!({
                "success": false,
                "recoverable": true,
                "error_kind": "approval_requires_metadata_repair",
                "error": "display metadata derived from the validated settlement did not pass the metadata gate",
                "project_path": project_dir.to_string_lossy(),
                "chapter_number": number,
                "metadata_gate": metadata_gate,
                "next_action": "repair_chapter_metadata"
            }));
        }
        let truth_issues = latest_truth_validation_issues(&manifest, number);
        let mut quality_gate =
            chapter_quality_gate(&manifest, &final_chapter, &body, &truth_issues);
        let duplicate_issues =
            cross_chapter_duplicate_issues(&project_dir, &manifest, &final_chapter, &body).await;
        route_cross_chapter_duplicate_issues(&mut quality_gate, duplicate_issues);
        if !quality_gate.passed {
            return Ok(json!({
                "success": false,
                "recoverable": true,
                "error_kind": "approval_requires_quality_gate",
                "error": "the final body still has deterministic hard blockers",
                "project_path": project_dir.to_string_lossy(),
                "chapter_number": number,
                "quality_gate": quality_gate,
                "next_action": "revise_chapter"
            }));
        }

        let transaction_id = read_approval_journal(&project_dir, number)
            .await?
            .filter(|journal| journal.state == ApprovalJournalState::Prepared)
            .map(|journal| journal.transaction_id)
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let prepared_at = now_iso();
        let backup_path = format!(".approval-transactions/{transaction_id}/before");
        let backup = project_dir.join(&backup_path);
        if backup.exists() {
            tokio::fs::remove_dir_all(&backup).await?;
        }
        snapshot::copy_project_state_for_snapshot(&project_dir, &backup).await?;
        let mut journal = ApprovalJournal {
            transaction_id: transaction_id.clone(),
            chapter_number: number,
            state: ApprovalJournalState::Prepared,
            body_fingerprint: body_fingerprint.clone(),
            authority_fingerprint: authority_fingerprint.clone(),
            prepared_at,
            committed_at: String::new(),
            receipt_path: String::new(),
            backup_path,
        };
        write_approval_journal(&project_dir, &journal).await?;

        manifest.chapters[chapter_index] = final_chapter.clone();
        manifest.chapters[chapter_index].status =
            chapter_lifecycle::ChapterLifecycleStatus::Approved
                .as_str()
                .to_string();
        manifest.chapters[chapter_index].updated_at = now_iso();
        let final_chapter = manifest.chapters[chapter_index].clone();
        promote_approved_chapter_character_identity_markers(&mut manifest, &body);
        promote_chapter_character_registrations(&mut manifest, number);
        write_approved_settlement(&project_dir, number, &settlement).await?;
        sync_chapter_record_file(&project_dir, &final_chapter).await?;

        let mut truth_updates = self
            .apply_pending_settlement_to_truth(&project_dir, &mut manifest, number, &settlement)
            .await?;
        update_story_bible_after_approved_chapter(&project_dir, &mut manifest, number).await?;
        if let Some(current_state) = manifest
            .story_bible
            .as_ref()
            .map(novel_bible::approved_state_truth)
        {
            truth_updates.push(
                write_truth_section_direct(
                    &project_dir,
                    &mut manifest,
                    "current_state",
                    &current_state,
                )
                .await?,
            );
        }
        if let Some(pending_hooks) = manifest
            .story_bible
            .as_ref()
            .map(novel_bible::pending_hook_truth)
        {
            truth_updates.push(
                write_truth_section_direct(
                    &project_dir,
                    &mut manifest,
                    "pending_hooks",
                    &pending_hooks,
                )
                .await?,
            );
        }
        compact_longform_state(&project_dir, &mut manifest).await?;
        manifest.updated_at = now_iso();
        let receipt = build_approval_receipt(
            &transaction_id,
            &final_chapter,
            &body_fingerprint,
            &authority_fingerprint,
            &review,
            &settlement,
            &approval_truth_fingerprint(&manifest),
        );
        self.write_manifest(&project_dir, &manifest).await?;

        write_approval_receipt(&project_dir, &receipt).await?;
        journal.state = ApprovalJournalState::Committed;
        journal.committed_at = receipt.committed_at.clone();
        journal.receipt_path =
            relative_project_path(&project_dir, &approval_receipt_path(&project_dir, number));
        write_approval_journal(&project_dir, &journal).await?;
        cleanup_approval_backup(&project_dir, &journal).await?;

        if snapshot::should_write_auto_chapter_snapshot(number) {
            snapshot::upsert_auto_chapter_snapshot(&project_dir, &mut manifest, number).await?;
            manifest.updated_at = now_iso();
            self.write_manifest(&project_dir, &manifest).await?;
        }
        self.sync_readable_txt_export(&project_dir, &manifest)
            .await?;

        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.approved",
            "project_path": project_dir.to_string_lossy(),
            "artifact_path": project_dir.join("project.json").to_string_lossy(),
            "chapter": final_chapter,
            "approval_receipt": receipt,
            "approval_journal": journal,
            "truth_updates": truth_updates,
            "state": project_state_summary(&manifest),
            "audit": audit_manifest(&manifest)
        }))
    }
}

pub(super) fn settlement_display_metadata_or_body_validated_best(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    settlement: &SettlementOutput,
    body: &str,
) -> (ChapterRecord, SettlementOutput, ChapterMetadataGate) {
    let settlement_updates = settlement
        .continuity_updates
        .iter()
        .cloned()
        .chain(
            clean_list(&settlement.resolved_hooks)
                .into_iter()
                .map(|hook| payoff_continuity_update(&hook)),
        )
        .collect::<Vec<_>>();
    let mut projected = chapter.clone();
    if !settlement.chapter_summary.trim().is_empty() {
        projected.summary =
            compact_chapter_summary(&settlement.chapter_summary, &manifest.language);
    }
    projected.continuity_updates =
        compact_truth_items(settlement_updates.clone(), CHAPTER_CONTINUITY_LIMIT);
    normalize_chapter_metadata_against_body(manifest, &mut projected, body);
    let projected_gate = chapter_metadata_gate(manifest, &projected, body);
    if !projected_gate.needs_repair() {
        let selected_settlement = settlement_with_selected_display_metadata(settlement, &projected);
        return (projected, selected_settlement, projected_gate);
    }

    let body_validated = chapter.clone();
    let body_validated_gate = chapter_metadata_gate(manifest, &body_validated, body);
    if !body_validated_gate.needs_repair() {
        let selected_settlement =
            settlement_with_selected_display_metadata(settlement, &body_validated);
        (body_validated, selected_settlement, body_validated_gate)
    } else {
        let selected_settlement = settlement_with_selected_display_metadata(settlement, &projected);
        (projected, selected_settlement, projected_gate)
    }
}

fn settlement_with_selected_display_metadata(
    settlement: &SettlementOutput,
    selected: &ChapterRecord,
) -> SettlementOutput {
    let mut selected_settlement = settlement.clone();
    selected_settlement.chapter_summary = selected.summary.clone();
    selected_settlement.continuity_updates = selected.continuity_updates.clone();
    selected_settlement
}

async fn cleanup_approval_backup(
    project_dir: &Path,
    journal: &ApprovalJournal,
) -> anyhow::Result<()> {
    if journal.backup_path.trim().is_empty() {
        return Ok(());
    }
    let backup = project_dir.join(&journal.backup_path);
    let root = backup.parent().unwrap_or(&backup);
    if root.exists() {
        tokio::fs::remove_dir_all(root).await?;
    }
    Ok(())
}

fn latest_passing_review<'a>(
    manifest: &'a NovelProjectManifest,
    chapter_number: usize,
    body_fingerprint: &str,
    authority_fingerprint: &str,
) -> Option<&'a ReviewReceipt> {
    manifest
        .reviews
        .iter()
        .rev()
        .find(|review| review.chapter_number == chapter_number)
        .filter(|review| {
            review.verdict == "passed"
                && review.locally_validated
                && review.chapter_fingerprint == body_fingerprint
                && review.authority_fingerprint == authority_fingerprint
                && review
                    .findings
                    .iter()
                    .all(|finding| !finding.hard_blocking())
        })
}

fn build_approval_receipt(
    transaction_id: &str,
    chapter: &ChapterRecord,
    body_fingerprint: &str,
    authority_fingerprint: &str,
    review: &ReviewReceipt,
    settlement: &SettlementOutput,
    truth_fingerprint: &str,
) -> ApprovalReceipt {
    ApprovalReceipt {
        transaction_id: transaction_id.to_string(),
        chapter_number: chapter.number,
        body_fingerprint: body_fingerprint.to_string(),
        metadata_fingerprint: chapter_metadata_fingerprint(chapter),
        authority_fingerprint: authority_fingerprint.to_string(),
        review_fingerprint: governance::authority_fingerprint(review),
        settlement_fingerprint: governance::authority_fingerprint(settlement),
        truth_fingerprint: truth_fingerprint.to_string(),
        committed_at: now_iso(),
        legacy: false,
    }
}

pub(super) fn approval_truth_fingerprint(manifest: &NovelProjectManifest) -> String {
    governance::authority_fingerprint(&json!({
        "truth_files": manifest.truth_files,
        "story_bible": manifest.story_bible
    }))
}

pub(super) fn chapter_metadata_fingerprint(chapter: &ChapterRecord) -> String {
    governance::authority_fingerprint(&json!({
        "title": chapter.title,
        "summary": chapter.summary,
        "key_facts": chapter.key_facts,
        "continuity_updates": chapter.continuity_updates,
        "unit_count": chapter.unit_count
    }))
}

fn relative_project_path(project_dir: &Path, path: &Path) -> String {
    path.strip_prefix(project_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn approval_blocked(
    project_dir: &Path,
    chapter_number: usize,
    error_kind: &str,
    error: &str,
    next_action: &str,
) -> serde_json::Value {
    json!({
        "success": false,
        "recoverable": true,
        "error_kind": error_kind,
        "error": error,
        "project_path": project_dir.to_string_lossy(),
        "chapter_number": chapter_number,
        "next_action": next_action
    })
}
