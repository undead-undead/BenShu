use super::storage::atomic_write_file;
use super::*;
use sha2::{Digest, Sha256};

pub(super) async fn accepted_best_candidate_matches(
    project_dir: &Path,
    chapter_number: usize,
    authority_fingerprint: &str,
    body_fingerprint: &str,
) -> anyhow::Result<bool> {
    let directory = project_dir.join("reviews/candidates");
    let canonical = directory.join(format!("chapter-{chapter_number:04}.best.json"));
    if canonical.exists() {
        let raw = tokio::fs::read(&canonical).await?;
        let candidate = serde_json::from_slice::<governance::DraftCandidateRecord>(&raw)?;
        return Ok(candidate.accepted_as_best
            && candidate.authority_fingerprint == authority_fingerprint
            && candidate.body_fingerprint == body_fingerprint
            && chapter_quality::chapter_body_fingerprint(&candidate.draft.content)
                == body_fingerprint);
    }
    let mut entries = match tokio::fs::read_dir(&directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let prefix = format!("chapter-{chapter_number:04}.candidate-");
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix) || !name.ends_with(".json") {
            continue;
        }
        let raw = tokio::fs::read(&path).await?;
        let Ok(candidate) = serde_json::from_slice::<governance::DraftCandidateRecord>(&raw) else {
            continue;
        };
        if candidate.accepted_as_best
            && candidate.authority_fingerprint == authority_fingerprint
            && candidate.body_fingerprint == body_fingerprint
            && chapter_quality::chapter_body_fingerprint(&candidate.draft.content)
                == body_fingerprint
        {
            return Ok(true);
        }
    }
    Ok(false)
}

impl NovelStudioTool {
    pub(in crate::tool::writing::novel_studio) async fn record_candidate_decision(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let chapter_number = args
            .chapter_number
            .ok_or_else(|| anyhow::anyhow!("chapter_number is required"))?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let cycle = manifest
            .review_cycles
            .iter_mut()
            .filter(|cycle| cycle.chapter_number == chapter_number)
            .max_by_key(|cycle| cycle.iteration)
            .ok_or_else(|| anyhow::anyhow!("chapter has no review cycle to bind"))?;
        cycle.attempt_kind = args.attempt_kind.trim().to_string();
        cycle.candidate_fingerprint = args.candidate_fingerprint.trim().to_string();
        cycle.quality_vector = args.quality_vector.clone();
        cycle.accepted_as_best = args.accepted_as_best;
        cycle.best_candidate_path = args.best_candidate_path.trim().to_string();
        let record = cycle.clone();
        atomic_write_file(
            project_dir.join(&record.path),
            serde_json::to_string_pretty(&record)?,
        )
        .await?;
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.checkpointed",
            "review_cycle": record
        }))
    }
}

pub(super) async fn write_stage_authority_record(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    chapter_number: usize,
    stage: &str,
    role: governance::AuthorityRole,
    artifact_fingerprint: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let Some(context) = manifest
        .context_packages
        .iter()
        .find(|record| record.number == chapter_number && record.sealed)
    else {
        return Ok(None);
    };
    let Some(projection_fingerprint) = context
        .role_projection_fingerprints
        .get(role.as_str())
        .filter(|fingerprint| !fingerprint.trim().is_empty())
    else {
        anyhow::bail!(
            "sealed chapter authority lacks {} projection fingerprint",
            role.as_str()
        );
    };
    if context.authority_root_fingerprint.trim().is_empty() {
        anyhow::bail!("sealed chapter authority lacks root fingerprint");
    }
    let record = json!({
        "schema_version": "benshu.stage_authority_record.v1",
        "chapter_number": chapter_number,
        "stage": stage,
        "role": role.as_str(),
        "authority_root_fingerprint": context.authority_root_fingerprint,
        "authority_projection_fingerprint": projection_fingerprint,
        "artifact_fingerprint": artifact_fingerprint,
        "created_at": now_iso()
    });
    let path = format!(
        "runtime/chapter-{chapter_number:04}.{}.authority.json",
        slugify(stage)
    );
    atomic_write_file(
        project_dir.join(&path),
        serde_json::to_string_pretty(&record)?,
    )
    .await?;
    Ok(Some(json!({
        "path": path,
        "record": record
    })))
}

pub(super) fn chapter_revision_fingerprint(chapter: &ChapterRecord, content: &str) -> String {
    let mut digest = Sha256::new();
    for value in [
        chapter.title.as_str(),
        chapter.volume_id.as_str(),
        chapter.volume_title.as_str(),
        chapter.summary.as_str(),
        content,
    ] {
        digest.update(value.len().to_le_bytes());
        digest.update(value.as_bytes());
    }
    for values in [&chapter.key_facts, &chapter.continuity_updates] {
        digest.update(values.len().to_le_bytes());
        for value in values {
            digest.update(value.len().to_le_bytes());
            digest.update(value.as_bytes());
        }
    }
    hex::encode(digest.finalize())
}

pub(super) async fn write_chapter_control_contract(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    number: usize,
    title: &str,
    plan: &str,
    notes: &str,
    key_facts: &[String],
    execution_contract_v2: ChapterExecutionContractV2,
) -> anyhow::Result<ChapterContractRecord> {
    tokio::fs::create_dir_all(project_dir.join("runtime")).await?;
    let source_refs = manifest
        .sources
        .iter()
        .map(|source| format!("{}:{}", source.id, source.title))
        .chain(
            manifest
                .truth_files
                .iter()
                .map(|truth| truth.section.clone()),
        )
        .collect::<Vec<_>>();
    let mut must_keep = clean_list(key_facts);
    if let Some(contract) = &manifest.contract {
        must_keep.extend(contract.world_rules.iter().cloned());
    }
    must_keep.extend(
        super::context_packaging::relevant_character_subgraph(
            manifest,
            number,
            manifest.chapter_plans.iter().find(|item| item.number == number),
        )
            .into_iter()
            .map(|character| {
                format!(
                    "canonical_identity_only: {}; preserve this name if the character appears, but do not introduce the character unless the current chapter goal requires it",
                    character.canonical_name
                )
            }),
    );
    must_keep.sort();
    must_keep.dedup();
    let must_avoid = manifest
        .contract
        .as_ref()
        .map(|contract| contract.must_avoid.clone())
        .unwrap_or_default();
    let directive = [plan.trim(), notes.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let contract = governance::build_chapter_control_contract(
        number,
        title,
        &directive,
        source_refs,
        must_keep,
        must_avoid,
        now_iso(),
    );
    let json_path = format!("runtime/chapter-{number:04}.contract.json");
    let markdown_path = format!("runtime/chapter-{number:04}.contract.md");
    atomic_write_file(
        project_dir.join(&json_path),
        serde_json::to_string_pretty(&contract)?,
    )
    .await?;
    atomic_write_file(
        project_dir.join(&markdown_path),
        governance::render_contract_markdown(&contract),
    )
    .await?;
    Ok(ChapterContractRecord {
        number,
        title: contract.title,
        path: json_path,
        markdown_path,
        goal: contract.goal,
        scene_goal: execution_contract_v2.scene_goal,
        conflict: execution_contract_v2.conflict,
        choice: execution_contract_v2.choice,
        cost: execution_contract_v2.cost,
        reveal: execution_contract_v2.reveal,
        emotional_beat: execution_contract_v2.emotional_beat,
        relationship_delta: execution_contract_v2.relationship_delta,
        power_delta: execution_contract_v2.power_delta,
        resource_delta: execution_contract_v2.resource_delta,
        hook_opened: execution_contract_v2.hook_opened,
        hook_paid_off: execution_contract_v2.hook_paid_off,
        character_change: execution_contract_v2.character_change,
        world_change: execution_contract_v2.world_change,
        payoff_target: execution_contract_v2.payoff_target,
        new_character_requests: execution_contract_v2.new_character_requests,
        character_registrations: execution_contract_v2.character_registrations,
        status: "ready".to_string(),
        created_at: contract.created_at.clone(),
        updated_at: contract.created_at,
    })
}

pub(super) async fn write_truth_validation_record(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
) -> anyhow::Result<TruthValidationRecord> {
    tokio::fs::create_dir_all(project_dir.join("runtime")).await?;
    let validation = governance::validate_truth_against_chapter(
        chapter.number,
        content,
        &chapter.key_facts,
        &chapter.continuity_updates,
        now_iso(),
    );
    let path = format!(
        "runtime/chapter-{:04}.truth_validation.json",
        chapter.number
    );
    atomic_write_file(
        project_dir.join(&path),
        serde_json::to_string_pretty(&validation)?,
    )
    .await?;
    let record = TruthValidationRecord {
        chapter_number: chapter.number,
        chapter_fingerprint: chapter_quality::chapter_body_fingerprint(content),
        path,
        verdict: validation.verdict,
        issues: validation.issues,
        created_at: validation.created_at,
    };
    upsert_truth_validation_record(manifest, record.clone());
    Ok(record)
}

pub(super) fn review_cycle_next_action(
    cycle: &ReviewCycleRecord,
    verdict: &str,
    metadata_needs_repair: bool,
) -> String {
    if cycle.next_action == "blocked" {
        return "blocked".to_string();
    }
    if verdict == "passed" {
        if metadata_needs_repair {
            "repair_chapter_metadata".to_string()
        } else {
            "approve_chapter".to_string()
        }
    } else {
        cycle.next_action.clone()
    }
}

pub(super) fn pending_settlement_path(project_dir: &Path, chapter_number: usize) -> PathBuf {
    project_dir.join(format!(
        "reviews/settlements/chapter-{chapter_number:04}.pending.json"
    ))
}

fn legacy_pending_settlement_path(project_dir: &Path, chapter_number: usize) -> PathBuf {
    project_dir.join(format!(
        "runtime/chapter-{chapter_number:04}.settlement.json"
    ))
}

pub(super) async fn write_pending_settlement(
    project_dir: &Path,
    chapter_number: usize,
    settlement: &SettlementOutput,
) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(project_dir.join("reviews/settlements")).await?;
    let path = pending_settlement_path(project_dir, chapter_number);
    atomic_write_file(path.clone(), serde_json::to_string_pretty(settlement)?).await?;
    Ok(path)
}

pub(super) async fn read_pending_settlement(
    project_dir: &Path,
    chapter_number: usize,
) -> anyhow::Result<Option<SettlementOutput>> {
    let current = pending_settlement_path(project_dir, chapter_number);
    let path = if current.exists() {
        current
    } else {
        legacy_pending_settlement_path(project_dir, chapter_number)
    };
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_json::from_str::<SettlementOutput>(&raw)?))
}

pub(super) async fn mark_pending_settlement_stale(
    project_dir: &Path,
    chapter_number: usize,
    reason: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let path = pending_settlement_path(project_dir, chapter_number);
    if !path.exists() {
        return Ok(None);
    }
    let stale_dir = project_dir.join("reviews/settlements/stale");
    tokio::fs::create_dir_all(&stale_dir).await?;
    let stale_path = stale_dir.join(format!(
        "chapter-{chapter_number:04}.{}.json",
        uuid::Uuid::new_v4().simple()
    ));
    tokio::fs::rename(&path, &stale_path).await?;
    atomic_write_file(
        stale_path.with_extension("reason.json"),
        serde_json::to_string_pretty(&json!({
            "chapter_number": chapter_number,
            "reason": reason,
            "staled_at": now_iso()
        }))?,
    )
    .await?;
    Ok(Some(stale_path))
}

pub(super) fn approved_settlement_path(project_dir: &Path, chapter_number: usize) -> PathBuf {
    project_dir.join(format!(
        "reviews/settlements/chapter-{chapter_number:04}.approved.json"
    ))
}

pub(super) async fn write_approved_settlement(
    project_dir: &Path,
    chapter_number: usize,
    settlement: &SettlementOutput,
) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(project_dir.join("reviews/settlements")).await?;
    let path = approved_settlement_path(project_dir, chapter_number);
    atomic_write_file(path.clone(), serde_json::to_string_pretty(settlement)?).await?;
    Ok(path)
}

pub(super) async fn read_approved_settlement(
    project_dir: &Path,
    chapter_number: usize,
) -> anyhow::Result<Option<SettlementOutput>> {
    let path = approved_settlement_path(project_dir, chapter_number);
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub(super) fn approval_receipt_path(project_dir: &Path, chapter_number: usize) -> PathBuf {
    project_dir.join(format!(
        "reviews/approvals/chapter-{chapter_number:04}.receipt.json"
    ))
}

pub(super) async fn read_approval_receipt(
    project_dir: &Path,
    chapter_number: usize,
) -> anyhow::Result<Option<ApprovalReceipt>> {
    let path = approval_receipt_path(project_dir, chapter_number);
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub(super) async fn write_approval_receipt(
    project_dir: &Path,
    receipt: &ApprovalReceipt,
) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(project_dir.join("reviews/approvals")).await?;
    let path = approval_receipt_path(project_dir, receipt.chapter_number);
    atomic_write_file(path.clone(), serde_json::to_string_pretty(receipt)?).await?;
    Ok(path)
}

pub(super) fn approval_journal_path(project_dir: &Path, chapter_number: usize) -> PathBuf {
    project_dir.join(format!(
        "reviews/approvals/chapter-{chapter_number:04}.journal.json"
    ))
}

pub(super) async fn read_approval_journal(
    project_dir: &Path,
    chapter_number: usize,
) -> anyhow::Result<Option<ApprovalJournal>> {
    let path = approval_journal_path(project_dir, chapter_number);
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub(super) async fn write_approval_journal(
    project_dir: &Path,
    journal: &ApprovalJournal,
) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(project_dir.join("reviews/approvals")).await?;
    let path = approval_journal_path(project_dir, journal.chapter_number);
    atomic_write_file(path.clone(), serde_json::to_string_pretty(journal)?).await?;
    Ok(path)
}

pub(super) async fn write_truth_section_direct(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
    section: &str,
    content: &str,
) -> anyhow::Result<serde_json::Value> {
    let section = section.trim();
    let content = normalize_truth_section_content(section, content, &manifest.language);
    tokio::fs::create_dir_all(project_dir.join("truth")).await?;
    let path = format!("truth/{}.md", slugify(section));
    atomic_write_file(
        project_dir.join(&path),
        render_truth_file(section, &content),
    )
    .await?;
    let record = TruthFileRecord {
        section: section.to_string(),
        path: path.clone(),
        unit_count: count_units(&content, &manifest.language),
        updated_at: now_iso(),
    };
    upsert_truth_record(manifest, record.clone());
    Ok(json!({
        "truth_file": record,
        "runtime_effect": "artifact.written"
    }))
}

pub(super) async fn write_review_cycle_record(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
    chapter_number: usize,
    verdict: &str,
    issues: &[String],
) -> anyhow::Result<ReviewCycleRecord> {
    tokio::fs::create_dir_all(project_dir.join("runtime")).await?;
    let previous_iterations = manifest
        .review_cycles
        .iter()
        .filter(|record| record.chapter_number == chapter_number)
        .count();
    let cycle = governance::build_review_cycle(
        chapter_number,
        previous_iterations,
        verdict,
        issues.to_vec(),
        now_iso(),
    );
    let path = format!(
        "runtime/chapter-{chapter_number:04}.review_cycle-{:04}.json",
        cycle.iteration
    );
    atomic_write_file(
        project_dir.join(&path),
        serde_json::to_string_pretty(&governance::review_cycle_json(&cycle))?,
    )
    .await?;
    let record = ReviewCycleRecord {
        chapter_number,
        path,
        iteration: cycle.iteration,
        verdict: cycle.verdict,
        next_action: cycle.next_action,
        attempt_kind: String::new(),
        candidate_fingerprint: String::new(),
        quality_vector: serde_json::Value::Null,
        accepted_as_best: false,
        best_candidate_path: String::new(),
        created_at: cycle.created_at,
    };
    manifest.review_cycles.push(record.clone());
    manifest
        .review_cycles
        .sort_by_key(|item| (item.chapter_number, item.iteration));
    Ok(record)
}

pub(super) async fn write_hook_debt_report_record(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
    chapter_number: usize,
) -> anyhow::Result<HookDebtReportRecord> {
    tokio::fs::create_dir_all(project_dir.join("runtime")).await?;
    let drafted = manifest
        .chapters
        .iter()
        .map(|chapter| chapter.number)
        .collect::<BTreeSet<_>>();
    let planned_without_draft = manifest
        .chapter_plans
        .iter()
        .filter(|plan| plan.number <= chapter_number && !drafted.contains(&plan.number))
        .map(|plan| plan.number)
        .collect::<Vec<_>>();
    let architecture_without_draft = manifest
        .chapter_architectures
        .iter()
        .filter(|item| item.number <= chapter_number && !drafted.contains(&item.number))
        .map(|item| item.number)
        .collect::<Vec<_>>();
    let chapters_missing_continuity = manifest
        .chapters
        .iter()
        .filter(|chapter| chapter.number <= chapter_number && chapter.continuity_updates.is_empty())
        .map(|chapter| chapter.number)
        .collect::<Vec<_>>();
    let truth_issues = manifest
        .truth_validations
        .iter()
        .rev()
        .find(|record| record.chapter_number == chapter_number)
        .map(|record| record.issues.clone())
        .unwrap_or_default();
    let report = governance::build_hook_debt_report(
        chapter_number,
        planned_without_draft,
        architecture_without_draft,
        chapters_missing_continuity,
        &truth_issues,
        now_iso(),
    );
    let path = format!("runtime/chapter-{chapter_number:04}.hook_debt.json");
    atomic_write_file(
        project_dir.join(&path),
        serde_json::to_string_pretty(&report)?,
    )
    .await?;
    let record = HookDebtReportRecord {
        chapter_number,
        path,
        debts: report.debts,
        created_at: report.created_at,
    };
    upsert_hook_debt_report_record(manifest, record.clone());
    Ok(record)
}

pub(super) async fn update_story_bible_after_approved_chapter(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
    chapter_number: usize,
) -> anyhow::Result<()> {
    let Some(chapter) = manifest
        .chapters
        .iter()
        .find(|chapter| chapter.number == chapter_number && chapter_is_approved(chapter))
        .cloned()
    else {
        return Ok(());
    };
    let settlement = read_approved_settlement(project_dir, chapter_number)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!("approved chapter {chapter_number} is missing its approved settlement")
        })?;
    let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
    let body = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
    let authority = read_sealed_chapter_authority(project_dir, manifest, chapter_number).await?;
    let validation = validate_settlement_for_chapter(&chapter, &body, &authority, &settlement);
    if !validation.passed {
        anyhow::bail!(
            "approved chapter {chapter_number} settlement is invalid: {}",
            validation.warnings.join("; ")
        );
    }
    ensure_story_bible_from_manifest(manifest);
    let character_registrations = manifest
        .chapter_contracts
        .iter()
        .find(|record| record.number == chapter_number)
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
    if let Some(bible) = manifest.story_bible.as_mut() {
        novel_bible::apply_approved_chapter_delta(bible, &delta, now_iso());
        bible.last_rebuilt_chapter = Some(chapter_number);
    }
    Ok(())
}

pub(super) async fn write_story_bible_artifacts(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
) -> anyhow::Result<()> {
    let Some(bible) = manifest.story_bible.as_ref() else {
        return Ok(());
    };
    atomic_write_file(
        project_dir.join("story_bible.json"),
        serde_json::to_string_pretty(bible)?,
    )
    .await?;
    atomic_write_file(
        project_dir.join("story_bible.md"),
        novel_bible::render_story_bible_markdown(bible),
    )
    .await?;
    Ok(())
}
