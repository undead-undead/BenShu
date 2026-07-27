use super::*;
use std::time::Instant;

#[derive(Debug, Clone)]
pub(super) struct StreamProgressThrottleState {
    pub(super) last_emit: Instant,
    pub(super) last_chars: usize,
}

impl NovelChapterRunner {
    pub(super) async fn persist_draft_candidate(
        &self,
        chapter_number: usize,
        iteration: usize,
        record: &DraftCandidateRecord,
    ) -> anyhow::Result<String> {
        let directory = PathBuf::from(&self.project_path)
            .join("reviews")
            .join("candidates");
        tokio::fs::create_dir_all(&directory).await?;
        let short_fingerprint = record
            .body_fingerprint
            .get(..12)
            .unwrap_or(record.body_fingerprint.as_str());
        let path = directory.join(format!(
            "chapter-{chapter_number:04}.candidate-{iteration:04}.{short_fingerprint}.json"
        ));
        let temporary = path.with_extension(format!("json.tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&temporary, serde_json::to_vec_pretty(record)?).await?;
        if let Err(error) = tokio::fs::rename(&temporary, &path).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error.into());
        }
        Ok(path.to_string_lossy().to_string())
    }

    pub(super) async fn persist_best_draft_candidate(
        &self,
        chapter_number: usize,
        record: &DraftCandidateRecord,
    ) -> anyhow::Result<String> {
        let path = PathBuf::from(&self.project_path)
            .join("reviews")
            .join("candidates")
            .join(format!("chapter-{chapter_number:04}.best.json"));
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("best candidate path has no parent"))?;
        tokio::fs::create_dir_all(parent).await?;
        let temporary = path.with_extension(format!("json.tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&temporary, serde_json::to_vec_pretty(record)?).await?;
        if let Err(error) = tokio::fs::rename(&temporary, &path).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error.into());
        }
        Ok(path.to_string_lossy().to_string())
    }

    pub(super) fn recover_last_accepted_candidate(
        &self,
        chapter_number: usize,
        authority_fingerprint: &str,
    ) -> Option<(String, DraftCandidateRecord)> {
        let directory = PathBuf::from(&self.project_path)
            .join("reviews")
            .join("candidates");
        let best_path = directory.join(format!("chapter-{chapter_number:04}.best.json"));
        if let Some(record) = fs::read(&best_path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<DraftCandidateRecord>(&raw).ok())
            .filter(|record| {
                record.accepted_as_best && record.authority_fingerprint == authority_fingerprint
            })
        {
            return Some((best_path.to_string_lossy().to_string(), record));
        }
        let prefix = format!("chapter-{chapter_number:04}.candidate-");
        let mut records = fs::read_dir(directory)
            .ok()?
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_str()?;
                if !name.starts_with(&prefix) || !name.ends_with(".json") {
                    return None;
                }
                let raw = fs::read(&path).ok()?;
                let record = serde_json::from_slice::<DraftCandidateRecord>(&raw).ok()?;
                if !record.accepted_as_best || record.authority_fingerprint != authority_fingerprint
                {
                    return None;
                }
                Some((name.to_string(), path, record))
            })
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.0.cmp(&right.0));
        records
            .pop()
            .map(|(_, path, record)| (path.to_string_lossy().to_string(), record))
    }

    pub(super) fn recover_revision_state(
        &self,
        chapter_number: usize,
        authority_fingerprint: &str,
    ) -> RevisionState {
        let directory = PathBuf::from(&self.project_path)
            .join("reviews")
            .join("candidates");
        let prefix = format!("chapter-{chapter_number:04}.candidate-");
        let mut state = RevisionState::default();
        let Ok(entries) = fs::read_dir(directory) else {
            return state;
        };
        let mut records = Vec::new();
        let mut next_candidate_iteration = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.starts_with(&prefix) || !name.ends_with(".json") {
                continue;
            }
            if let Some(iteration) = name
                .strip_prefix(&prefix)
                .and_then(|tail| tail.split('.').next())
                .and_then(|value| value.parse::<usize>().ok())
            {
                next_candidate_iteration =
                    next_candidate_iteration.max(iteration.saturating_add(1));
            }
            let Some(record) = fs::read(&path)
                .ok()
                .and_then(|raw| serde_json::from_slice::<DraftCandidateRecord>(&raw).ok())
                .filter(|record| record.authority_fingerprint == authority_fingerprint)
            else {
                continue;
            };
            records.push((name.to_string(), path, record));
        }
        let body_by_candidate_id = records
            .iter()
            .map(|(_, _, record)| (record.candidate_id.clone(), record.body_fingerprint.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut accepted = Vec::new();
        for (name, path, record) in records {
            if record.provenance == CandidateProvenance::LocalCleanup {
                if let Some(parent_body_fingerprint) = record
                    .parent_candidate_id
                    .as_deref()
                    .and_then(|parent| body_by_candidate_id.get(parent))
                {
                    state
                        .budget
                        .local_cleanup_fingerprints
                        .insert(parent_body_fingerprint.clone());
                }
            } else {
                restore_recovered_attempt_budget(&mut state.budget, record.provenance);
            }
            if record.accepted_as_best {
                accepted.push((name, path, record));
            }
        }
        accepted.sort_by(|left, right| left.0.cmp(&right.0));
        if let Some((_, path, record)) = accepted.pop() {
            state.best_candidate_id = Some(record.candidate_id);
            state.best_candidate_path = Some(path.to_string_lossy().to_string());
        }
        state.next_candidate_iteration = next_candidate_iteration;
        state
    }

    pub(super) fn progress_sink(
        &self,
        chapter_number: usize,
        phase: impl Into<String>,
    ) -> Option<TextGenerationProgressSink> {
        let task_id = self.runtime.task_id?;
        let event_manager = self.runtime.event_manager.clone();
        let worker_label = self.worker_label.clone();
        let project_path = self.project_path.clone();
        let phase = phase.into();
        let throttle = self.progress_throttle.clone();
        Some(Arc::new(move |progress: TextGenerationProgress| {
            if !stream_progress_should_emit(
                &throttle,
                chapter_number,
                &phase,
                progress.generated_chars,
                progress.stage,
            ) {
                return;
            }
            let event_manager = event_manager.clone();
            let worker_label = worker_label.clone();
            let project_path = project_path.clone();
            let phase = phase.clone();
            tokio::spawn(async move {
                let stage = match progress.stage {
                    TextGenerationProgressStage::Started => "started",
                    TextGenerationProgressStage::Streaming => "streaming",
                    TextGenerationProgressStage::Completed => "completed",
                };
                let substantive_chars = progress
                    .snapshot
                    .as_deref()
                    .map(clean_stream_progress_text)
                    .map(|text| text.chars().filter(|ch| !ch.is_whitespace()).count())
                    .unwrap_or(progress.generated_chars);
                let preview = progress.preview.as_deref().map(clean_stream_progress_text);
                if let Some(manager) = event_manager {
                    let _ = manager
                        .append(
                            benshu_state::RuntimeEventRecord::new("continuous.text.streaming")
                                .with_task(task_id)
                                .with_actor(worker_label.clone())
                                .with_payload(json!({
                                    "chapter_number": chapter_number,
                                    "phase": phase,
                                    "stage": stage,
                                    "generated_chars": progress.generated_chars,
                                    "substantive_chars": substantive_chars,
                                    "project_path": project_path,
                                    "preview": preview,
                                })),
                        )
                        .await;
                }
            });
        }))
    }
}

pub(super) fn restore_recovered_attempt_budget(
    budget: &mut RevisionBudget,
    provenance: CandidateProvenance,
) {
    match provenance {
        CandidateProvenance::LengthTopup => budget.length_topup_attempted = true,
        CandidateProvenance::TailCompletion | CandidateProvenance::TruncatedRecovery => {
            budget.tail_completion_attempted = true
        }
        CandidateProvenance::MetadataRepair => {
            budget.metadata_repair_attempts = budget.metadata_repair_attempts.saturating_add(1)
        }
        CandidateProvenance::SemanticRevision | CandidateProvenance::Regenerated => {
            budget.semantic_attempts += 1
        }
        CandidateProvenance::InitialDraft
        | CandidateProvenance::RecoveredBest
        | CandidateProvenance::LegacyCandidate
        | CandidateProvenance::LocalCleanup => {}
    }
}
