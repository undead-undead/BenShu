use super::*;
use serde_json::Value;

fn apply_character_registrations<T>(
    value: T,
    registrations: &[ChapterCharacterRegistration],
) -> anyhow::Result<T>
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let mut encoded = serde_json::to_value(value)?;
    governance::replace_character_request_ids_in_value(&mut encoded, registrations);
    Ok(serde_json::from_value(encoded)?)
}

async fn approved_truth_snapshot(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> anyhow::Result<Value> {
    let all_chapters = approved_prior_chapters(manifest, chapter_number)
        .map(|chapter| {
            json!({
                "number": chapter.number,
                "title": chapter.title,
                "summary": chapter.summary,
                "key_facts": chapter.key_facts,
                "continuity_updates": chapter.continuity_updates,
                "unit_count": chapter.unit_count,
                "status": chapter.status
            })
        })
        .collect::<Vec<_>>();
    let recent_chapter_records = approved_prior_chapters(manifest, chapter_number)
        .rev()
        .take(8)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let mut recent_chapters = Vec::with_capacity(recent_chapter_records.len());
    for chapter in &recent_chapter_records {
        recent_chapters.push(approved_chapter_context_view(project_dir, manifest, chapter).await?);
    }
    let mut story_state = manifest
        .story_bible
        .as_ref()
        .and_then(|bible| serde_json::to_value(bible).ok())
        .unwrap_or(Value::Null);
    if let Some(object) = story_state.as_object_mut() {
        object.remove("structured_contract_v2");
        object.remove("chapter_summaries");
        object.remove("narrative_graph");
        object.remove("theme_ledger");
    }
    Ok(json!({
        "schema_version": "benshu.approved_truth_snapshot.v2",
        "cutoff_chapter": chapter_number.saturating_sub(1),
        "approved_prefix_chapters": all_chapters.len(),
        "approved_history_fingerprint": governance::authority_fingerprint(&all_chapters),
        "recent_approved_chapters": recent_chapters,
        "story_state": story_state
    }))
}

fn working_context_without_contract_mirrors(mut context: Value) -> Value {
    if let Some(object) = context.as_object_mut() {
        object.remove("contract");
        object.remove("truth_files");
        object.remove("recent_chapters");
        if let Some(story_bible) = object.get_mut("story_bible").and_then(Value::as_object_mut) {
            story_bible.remove("structured_contract_v2");
            story_bible.remove("source_contract_revision");
        }
    }
    context
}

impl NovelStudioTool {
    pub(in crate::tool::writing::novel_studio) async fn reconstruct_legacy_unapproved_authorities(
        &self,
        project_dir: &Path,
        chapter_limit: Option<usize>,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let manifest = self.read_manifest(project_dir).await?;
        let mut candidates = manifest
            .chapters
            .iter()
            .filter(|chapter| !chapter_is_approved(chapter))
            .filter(|chapter| chapter_limit.is_none_or(|limit| chapter.number <= limit))
            .filter(|chapter| {
                manifest
                    .context_packages
                    .iter()
                    .all(|record| record.number != chapter.number || !record.sealed)
            })
            .filter_map(|chapter| {
                let plan = manifest
                    .chapter_plans
                    .iter()
                    .find(|record| record.number == chapter.number)?
                    .clone();
                let contract = manifest
                    .chapter_contracts
                    .iter()
                    .find(|record| record.number == chapter.number)?
                    .clone();
                let architecture = manifest
                    .chapter_architectures
                    .iter()
                    .find(|record| record.number == chapter.number)?
                    .clone();
                Some((chapter.clone(), plan, contract, architecture))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(chapter, _, _, _)| chapter.number);

        let mut migrated = Vec::new();
        for (chapter, plan, contract, architecture) in candidates {
            let project_path = project_dir.to_string_lossy().to_string();
            let compose_args = serde_json::from_value::<NovelStudioArgs>(json!({
                "action": "compose_context",
                "project_path": project_path,
                "chapter_number": chapter.number
            }))?;
            self.compose_context(&compose_args).await?;

            let persist_args = serde_json::from_value::<NovelStudioArgs>(json!({
                "action": "persist_execution_package",
                "project_path": project_path,
                "chapter_number": chapter.number,
                "chapter_title": plan.title,
                "plan": plan.plan,
                "content": architecture.architecture,
                "scene_goal": contract.scene_goal,
                "conflict": contract.conflict,
                "choice": contract.choice,
                "cost": contract.cost,
                "reveal": contract.reveal,
                "emotional_beat": contract.emotional_beat,
                "relationship_delta": contract.relationship_delta,
                "power_delta": contract.power_delta,
                "resource_delta": contract.resource_delta,
                "hook_opened": contract.hook_opened,
                "hook_paid_off": contract.hook_paid_off,
                "character_change": contract.character_change,
                "world_change": contract.world_change,
                "payoff_target": contract.payoff_target,
                "new_character_requests": contract.new_character_requests,
                "status": "legacy_reconstructed"
            }))?;
            let sealed = self.persist_execution_package(&persist_args).await?;
            let authority_fingerprint = sealed
                .get("authority_root_fingerprint")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            let mut current = self.read_manifest(project_dir).await?;
            let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
            let body = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
            let truth_issues = current
                .truth_validations
                .iter()
                .filter(|record| record.chapter_number == chapter.number)
                .max_by_key(|record| &record.created_at)
                .map(|record| record.issues.clone())
                .unwrap_or_default();
            let gate = chapter_quality_gate(&current, &chapter, &body, &truth_issues);
            let hard_findings = gate
                .findings
                .iter()
                .filter(|finding| finding.hard_blocking())
                .cloned()
                .collect::<Vec<_>>();
            if let Some(record) = current
                .chapters
                .iter_mut()
                .find(|record| record.number == chapter.number)
            {
                record.status = if hard_findings.is_empty() {
                    chapter_lifecycle::ChapterLifecycleStatus::Draft
                } else {
                    chapter_lifecycle::ChapterLifecycleStatus::NeedsRevision
                }
                .as_str()
                .to_string();
                record.updated_at = now_iso();
            }
            current.updated_at = now_iso();
            self.write_manifest(project_dir, &current).await?;

            let settlement_rebuilt = if hard_findings.is_empty() {
                let settlement_args = serde_json::from_value::<NovelStudioArgs>(json!({
                    "action": "settle_chapter_state",
                    "project_path": project_path,
                    "chapter_number": chapter.number
                }))?;
                self.settle_chapter_state(&settlement_args)
                    .await?
                    .get("validation")
                    .and_then(|validation| validation.get("passed"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            } else {
                false
            };
            let body_fingerprint = chapter_quality::chapter_body_fingerprint(&body);
            let draft = runner::DraftOutput {
                title: chapter.title.clone(),
                content: body,
                summary: chapter.summary.clone(),
                key_facts: chapter.key_facts.clone(),
                continuity_updates: chapter.continuity_updates.clone(),
                degraded: false,
                degraded_reason: String::new(),
            };
            let metadata_fingerprint = governance::authority_fingerprint(&json!({
                "title": &draft.title,
                "summary": &draft.summary,
                "key_facts": &draft.key_facts,
                "continuity_updates": &draft.continuity_updates
            }));
            let candidate_id = governance::authority_fingerprint(&json!({
                "authority_fingerprint": &authority_fingerprint,
                "body_fingerprint": &body_fingerprint,
                "metadata_fingerprint": &metadata_fingerprint,
                "provenance": governance::CandidateProvenance::LegacyCandidate
            }));
            let authority_conflicts = hard_findings
                .iter()
                .filter(|finding| {
                    matches!(
                        finding.class,
                        chapter_quality::ChapterFindingClass::Contract
                            | chapter_quality::ChapterFindingClass::Continuity
                    )
                })
                .count();
            let state_conflicts = hard_findings
                .iter()
                .filter(|finding| finding.class == chapter_quality::ChapterFindingClass::State)
                .count();
            let candidate = governance::DraftCandidateRecord {
                candidate_id,
                parent_candidate_id: None,
                authority_fingerprint: authority_fingerprint.clone(),
                body_fingerprint: body_fingerprint.clone(),
                metadata_fingerprint,
                draft,
                findings: hard_findings.clone(),
                quality_vector: governance::RevisionQualityVector {
                    hard_blockers: hard_findings.len(),
                    authority_conflicts,
                    state_conflicts,
                    incomplete_body: hard_findings.iter().any(|finding| {
                        finding.code == "body_truncated" || finding.code == "body_missing"
                    }),
                    contaminated_body: hard_findings
                        .iter()
                        .any(|finding| finding.code == "body_surface_contamination"),
                    ..Default::default()
                },
                provenance: governance::CandidateProvenance::LegacyCandidate,
                accepted_as_best: true,
            };
            let candidate_dir = project_dir.join("reviews/candidates");
            tokio::fs::create_dir_all(&candidate_dir).await?;
            let candidate_path = format!(
                "reviews/candidates/chapter-{:04}.candidate-0000.{}.json",
                chapter.number,
                &body_fingerprint[..body_fingerprint.len().min(12)]
            );
            atomic_write_file(
                project_dir.join(&candidate_path),
                serde_json::to_string_pretty(&candidate)?,
            )
            .await?;
            let best_path = format!("reviews/candidates/chapter-{:04}.best.json", chapter.number);
            atomic_write_file(
                project_dir.join(&best_path),
                serde_json::to_string_pretty(&candidate)?,
            )
            .await?;
            migrated.push(json!({
                "chapter_number": chapter.number,
                "candidate_path": candidate_path,
                "best_candidate_path": best_path,
                "authority_root_fingerprint": authority_fingerprint,
                "hard_blocked": !hard_findings.is_empty()
                ,"settlement_rebuilt": settlement_rebuilt
            }));
        }
        Ok(migrated)
    }

    pub(in crate::tool::writing::novel_studio) async fn compose_context(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        ensure_structured_contract_v2(&mut manifest);
        let number = match args.chapter_number {
            Some(number) => number,
            None => {
                durable_chapter_progress(&project_dir, &manifest)
                    .await
                    .next_chapter
            }
        };
        tokio::fs::create_dir_all(project_dir.join("runtime")).await?;

        if !args.minimal_context && !args.overwrite {
            if let Some(record) = manifest
                .context_packages
                .iter()
                .find(|record| record.number == number)
                .cloned()
            {
                let current_contract_fingerprint = governance::authority_fingerprint(
                    &canonical_project_contract_projection(&manifest),
                );
                let current_truth_fingerprint = governance::authority_fingerprint(
                    &approved_truth_snapshot(&project_dir, &manifest, number).await?,
                );
                let sealed_dependencies_are_current = !record.sealed
                    || (record.canonical_contract_fingerprint == current_contract_fingerprint
                        && record.truth_fingerprint == current_truth_fingerprint);
                if record.sealed && !sealed_dependencies_are_current {
                    anyhow::bail!(
                        "authority_stale: chapter {number} canonical contract or approved truth changed after authority sealing"
                    );
                }
                let context_file = project_dir.join(&record.path);
                let rules_file = project_dir.join(&record.rules_path);
                let trace_file = project_dir.join(&record.trace_path);
                if context_file.exists() && rules_file.exists() && trace_file.exists() {
                    if let Ok(raw) = tokio::fs::read_to_string(&context_file).await {
                        if record.sealed && sealed_dependencies_are_current {
                            let authority =
                                serde_json::from_str::<governance::SealedChapterAuthority>(&raw)
                                    .map_err(|error| {
                                        anyhow::anyhow!(
                                            "sealed chapter authority cannot be decoded: {error}"
                                        )
                                    })?;
                            if authority.schema_version == governance::sealed_authority_version() {
                                if authority.chapter_number != number
                                    || !authority.protected_coverage.complete
                                {
                                    anyhow::bail!(
                                    "sealed chapter authority failed chapter or coverage validation"
                                );
                                }
                                let root_payload = authority
                                    .projection(governance::AuthorityRole::Writer)
                                    .and_then(|projection| projection.payload.get("authority"))
                                    .ok_or_else(|| {
                                        anyhow::anyhow!(
                                            "sealed chapter authority has no writer projection"
                                        )
                                    })?;
                                if governance::authority_fingerprint(root_payload)
                                    != authority.authority_root_fingerprint
                                {
                                    anyhow::bail!(
                                        "sealed chapter authority root fingerprint mismatch"
                                    );
                                }
                                let prompt_context = governance::model_authority_projection_payload(
                                    governance::AuthorityRole::Writer,
                                    root_payload,
                                    &authority.authority_root_fingerprint,
                                );
                                return Ok(json!({
                                "success": true,
                                "project_path": project_dir.to_string_lossy(),
                                "chapter_number": number,
                                "context_path": context_file.to_string_lossy(),
                                "full_context_path": context_file.to_string_lossy(),
                                "rules_path": rules_file.to_string_lossy(),
                                "trace_path": trace_file.to_string_lossy(),
                                "context": root_payload,
                                "prompt_context": prompt_context,
                                "context_package": authority.context_package,
                                "context_budget": record.context_budget,
                                "rule_stack": authority.rule_stack,
                                "trace": authority.trace,
                                "sealed_authority": authority,
                                "authority_root_fingerprint": record.authority_root_fingerprint,
                                "role_projection_fingerprints": record.role_projection_fingerprints,
                                "protected_coverage": record.protected_coverage,
                                "context_package_record": record,
                                "sealed": true,
                                "reused": true,
                                "stage": pipeline::NovelPhase::ContextPackage,
                                "next_action": "write_draft"
                                }));
                            }
                        }
                        if !record.sealed {
                            if let Ok(saved) = serde_json::from_str::<serde_json::Value>(&raw) {
                                let context = saved
                                    .get("project_context")
                                    .cloned()
                                    .or_else(|| saved.get("context").cloned())
                                    .unwrap_or_else(|| saved.clone());
                                let prompt_context = build_prompt_context_payload(&context);
                                let context_budget =
                                    saved.get("context_budget").cloned().unwrap_or_else(|| {
                                        build_context_budget_telemetry(
                                            &context,
                                            &prompt_context,
                                            &manifest.language,
                                        )
                                    });
                                let context_package = saved
                                    .get("context_package")
                                    .cloned()
                                    .unwrap_or_else(|| json!({ "selected_context": [] }));
                                let rule_stack = saved
                                    .get("rule_stack")
                                    .cloned()
                                    .unwrap_or_else(|| json!({}));
                                let trace =
                                    saved.get("trace").cloned().unwrap_or_else(|| json!({}));
                                return Ok(json!({
                                    "success": true,
                                    "project_path": project_dir.to_string_lossy(),
                                    "chapter_number": number,
                                    "context_path": context_file.to_string_lossy(),
                                    "full_context_path": context_file.to_string_lossy(),
                                    "rules_path": rules_file.to_string_lossy(),
                                    "trace_path": trace_file.to_string_lossy(),
                                    "context": context,
                                    "prompt_context": prompt_context,
                                    "context_package": context_package,
                                    "context_budget": context_budget,
                                    "rule_stack": rule_stack,
                                    "trace": trace,
                                    "context_package_record": record,
                                    "sealed": record.sealed,
                                    "reused": true,
                                    "stage": pipeline::NovelPhase::ContextPackage,
                                    "next_action": "generate_chapter_execution_package"
                                }));
                            }
                        }
                    }
                }
            }
        }

        let (context, context_package, rule_stack, mut trace) = if args.minimal_context {
            let context = build_minimal_context_payload(&manifest, number);
            let context_package = governance::build_context_package(number, Vec::new());
            let rule_stack = governance::build_rule_stack(
                number,
                manifest.contract.is_some(),
                manifest
                    .chapter_contracts
                    .iter()
                    .any(|record| record.number == number),
                manifest.truth_files.len(),
                0,
                manifest
                    .chapter_architectures
                    .iter()
                    .any(|record| record.number == number),
            );
            let mut trace = governance::build_trace(
                number,
                vec!["typed_manifest_minimal_authority".to_string()],
                vec!["typed_manifest_minimal_authority".to_string()],
                Vec::new(),
            );
            trace.notes.push(format!(
                "minimal_authoritative_context: {}",
                args.notes.trim()
            ));
            (context, context_package, rule_stack, trace)
        } else {
            let context = build_context_payload(&project_dir, &manifest, number).await?;
            let (context_package, rule_stack, trace) =
                build_context_governance(&project_dir, &manifest, number).await?;
            (context, context_package, rule_stack, trace)
        };
        let prompt_context = build_prompt_context_payload(&context);
        let canonical_contract = canonical_project_contract_projection(&manifest);
        let truth_as_of_chapter = approved_truth_snapshot(&project_dir, &manifest, number).await?;
        let execution_authority_context = json!({
            "schema_version": "benshu.presealed_execution_authority.v1",
            "chapter_number": number,
            "canonical_contract": canonical_contract,
            "truth_as_of_chapter": truth_as_of_chapter,
            "current_chapter_goal": context_packaging::chapter_boundary_seed_view(
                &manifest,
                number,
            ).into_iter().collect::<Vec<_>>(),
            "current_plan": context.get("plan").cloned().unwrap_or(Value::Null),
            "next_chapter_boundary": context
                .get("next_chapter_boundary")
                .cloned()
                .unwrap_or(Value::Null),
            "rolling_outline_window": context_packaging::rolling_outline_window_view(
                &manifest,
                number,
                governance::ROLLING_OUTLINE_LOOKAHEAD_CHAPTERS,
            )
        });
        let protected_chars = serde_json::to_string(&execution_authority_context)?
            .chars()
            .count();
        if !args.minimal_context && protected_chars > protected_prompt_context_char_limit() {
            anyhow::bail!(
                "protected authority exceeds the prompt budget ({protected_chars} > {}); compact optional contract enrichment before generating prose",
                protected_prompt_context_char_limit()
            );
        }
        let context_budget =
            build_context_budget_telemetry(&context, &prompt_context, &manifest.language);
        trace.prompt_context_fingerprint = prompt_context_fingerprint(&prompt_context);
        trace.context_budget = context_budget.clone();
        trace.notes.push(if args.minimal_context {
            "mode=minimal_authoritative_context".to_string()
        } else {
            "mode=governed_context".to_string()
        });
        let artifact_variant = if args.minimal_context { ".minimal" } else { "" };
        let context_path = format!("runtime/chapter-{number:04}{artifact_variant}.context.json");
        let rules_path = format!("runtime/chapter-{number:04}{artifact_variant}.rules.yaml");
        let trace_path = format!("runtime/chapter-{number:04}{artifact_variant}.trace.json");
        atomic_write_file(
            project_dir.join(&context_path),
            serde_json::to_string_pretty(&json!({
                    "project_context": context,
                    "context_package": context_package,
                    "context_budget": context_budget,
                    "rule_stack": rule_stack,
                "trace": trace
                ,"sealed": false
            }))?,
        )
        .await?;
        atomic_write_file(
            project_dir.join(&rules_path),
            governance::render_rule_stack_yaml(&rule_stack),
        )
        .await?;
        atomic_write_file(
            project_dir.join(&trace_path),
            serde_json::to_string_pretty(&trace)?,
        )
        .await?;
        let record = ContextPackageRecord {
            number,
            path: context_path.clone(),
            rules_path: rules_path.clone(),
            trace_path: trace_path.clone(),
            selected_sources: context_package.selected_context.len(),
            context_budget: context_budget.clone(),
            authority_root_fingerprint: String::new(),
            sealed: false,
            sealed_at: String::new(),
            chapter_contract_fingerprint: String::new(),
            canonical_contract_fingerprint: String::new(),
            truth_fingerprint: String::new(),
            truth_cutoff_chapter: number.saturating_sub(1),
            role_projection_fingerprints: BTreeMap::new(),
            protected_coverage: json!({}),
            excluded_future_paths: Vec::new(),
            created_at: now_iso(),
        };
        if !args.minimal_context {
            upsert_context_package_record(&mut manifest, record.clone());
            manifest.updated_at = now_iso();
            self.write_manifest(&project_dir, &manifest).await?;
        }
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "chapter_number": number,
            "context_path": project_dir.join(&context_path).to_string_lossy(),
            "full_context_path": project_dir.join(&context_path).to_string_lossy(),
            "rules_path": project_dir.join(&rules_path).to_string_lossy(),
            "trace_path": project_dir.join(&trace_path).to_string_lossy(),
            "context": context,
            "prompt_context": prompt_context,
            "execution_authority_context": execution_authority_context,
            "context_package": context_package,
            "context_budget": context_budget,
            "rule_stack": rule_stack,
            "trace": trace,
            "context_package_record": record,
            "sealed": false,
            "stage": pipeline::NovelPhase::ContextPackage,
            "next_action": "generate_chapter_execution_package"
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn add_chapter_plan(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        self.ensure_project_scaffold(&project_dir).await?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let number = args
            .chapter_number
            .unwrap_or_else(|| next_planned_chapter_number(&manifest));
        let mut plan = first_non_empty(&[
            args.plan.as_str(),
            args.content.as_str(),
            args.outline.as_str(),
            args.summary.as_str(),
            args.notes.as_str(),
            args.brief.as_str(),
        ])
        .trim()
        .to_string();
        let plan_generated_from_contract = if plan.is_empty() || plan == "untitled" {
            if let Some(fallback) = fallback_chapter_plan_from_manifest(&manifest, number) {
                plan = fallback;
                true
            } else {
                false
            }
        } else {
            false
        };
        if plan.trim().is_empty() || plan == "untitled" {
            anyhow::bail!(
                "plan, content, outline, summary, notes, or brief is required for add_chapter_plan"
            );
        }
        let default_title = default_chapter_title(&manifest.language, number);
        let title = first_non_empty(&[
            args.chapter_title.as_str(),
            args.title.as_str(),
            default_title.as_str(),
        ])
        .to_string();
        let path = format!("plans/{number:04}_{}.md", slugify(&title));
        let now = now_iso();
        let record = ChapterPlanRecord {
            number,
            title,
            path: path.clone(),
            plan: plan.trim().to_string(),
            status: first_non_empty(&[args.status.as_str(), "planned"]).to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        atomic_write_file(
            project_dir.join(&path),
            render_plan_file(&record, args.notes.trim()),
        )
        .await?;
        let execution_contract = chapter_execution_contract_v2_from_args(args);
        let contract_record = write_chapter_control_contract(
            &project_dir,
            &manifest,
            number,
            &record.title,
            plan.trim(),
            args.notes.trim(),
            &args.key_facts,
            execution_contract,
        )
        .await?;
        manifest.chapter_plans.retain(|plan| plan.number != number);
        manifest.chapter_plans.push(record.clone());
        manifest.chapter_plans.sort_by_key(|plan| plan.number);
        upsert_chapter_contract_record(&mut manifest, contract_record.clone());
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;

        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "chapter_plan": record,
            "chapter_contract": contract_record,
            "plan_generated_from_contract": plan_generated_from_contract,
            "state": project_state_summary_light(&manifest),
            "audit": light_status_audit_manifest(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn plan_chapter(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let result = self.add_chapter_plan(args).await?;
        Ok(with_stage(
            result,
            pipeline::NovelPhase::ChapterExecutionPackage.as_str(),
            "compose_chapter",
        ))
    }

    pub(in crate::tool::writing::novel_studio) async fn architect_chapter(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        self.ensure_project_scaffold(&project_dir).await?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let mut number = args
            .chapter_number
            .unwrap_or_else(|| next_unarchitected_planned_chapter_number(&manifest));
        let planned = manifest
            .chapter_plans
            .iter()
            .find(|plan| plan.number == number)
            .or_else(|| {
                if args.chapter_number.is_some() {
                    return None;
                }
                manifest
                    .chapter_plans
                    .iter()
                    .filter(|plan| {
                        !manifest
                            .chapter_architectures
                            .iter()
                            .any(|item| item.number == plan.number)
                    })
                    .min_by_key(|plan| plan.number)
            });
        if let Some(plan) = planned {
            number = plan.number;
        }
        let planned_text = planned.map(|plan| plan.plan.as_str()).unwrap_or("");
        let architecture = first_non_empty(&[
            args.content.as_str(),
            args.outline.as_str(),
            args.plan.as_str(),
            args.notes.as_str(),
            planned_text,
        ]);
        if architecture.trim().is_empty() || architecture == "untitled" {
            anyhow::bail!(
                "content, outline, plan, notes, or an existing chapter plan is required for architect_chapter"
            );
        }
        let default_title = default_chapter_title(&manifest.language, number);
        let title = first_non_empty(&[
            args.chapter_title.as_str(),
            args.title.as_str(),
            planned.map(|plan| plan.title.as_str()).unwrap_or(""),
            default_title.as_str(),
        ])
        .to_string();
        let path = format!("plans/{number:04}_{}_architecture.md", slugify(&title));
        let now = now_iso();
        let record = ChapterArchitectureRecord {
            number,
            title,
            path: path.clone(),
            architecture: architecture.trim().to_string(),
            status: first_non_empty(&[args.status.as_str(), "architected"]).to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        atomic_write_file(
            project_dir.join(&path),
            render_architecture_file(&record, args.notes.trim()),
        )
        .await?;
        manifest
            .chapter_architectures
            .retain(|item| item.number != number);
        manifest.chapter_architectures.push(record.clone());
        manifest
            .chapter_architectures
            .sort_by_key(|item| item.number);
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;

        Ok(json!({
            "success": true,
            "stage": pipeline::NovelPhase::ChapterExecutionPackage,
            "next_action": "write_draft",
            "project_path": project_dir.to_string_lossy(),
            "chapter_number": number,
            "chapter_title": record.title,
            "chapter_architecture": record,
            "writing_phase": runner::writing_phase_contract(
                pipeline::NovelPhase::Draft,
                "write_draft",
                &project_dir,
                number,
                &record.title,
                "Generate complete bounded chapter prose from the saved story contract, context package, and chapter architecture, then submit it through the content submission contract.",
                manifest.chapter_unit_target,
            ),
            "progress_report_contract": crate::tool::writing::session_surface::longform_progress_report_contract(),
            "writing_policy": policy::fiction_stage_policy(
                pipeline::NovelPhase::ChapterExecutionPackage.as_str(),
                "write_draft",
            ),
            "next_step_hint": "Execute the returned writing_phase. The chat reply should report progress/path only; the chapter body belongs in novel_studio write_draft content.",
            "state": project_state_summary_light(&manifest),
            "audit": light_status_audit_manifest(&manifest)
        }))
    }

    pub(in crate::tool::writing::novel_studio) async fn persist_execution_package(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        self.ensure_project_scaffold(&project_dir).await?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let number = args
            .chapter_number
            .unwrap_or_else(|| next_planned_chapter_number(&manifest));
        let mut plan = first_non_empty(&[
            args.plan.as_str(),
            args.content.as_str(),
            args.outline.as_str(),
            args.summary.as_str(),
            args.notes.as_str(),
            args.brief.as_str(),
        ])
        .trim()
        .to_string();
        let plan_generated_from_contract = if plan.is_empty() || plan == "untitled" {
            if let Some(fallback) = fallback_chapter_plan_from_manifest(&manifest, number) {
                plan = fallback;
                true
            } else {
                false
            }
        } else {
            false
        };
        if plan.trim().is_empty() || plan == "untitled" {
            anyhow::bail!("execution package plan is empty");
        }
        let architecture = first_non_empty(&[
            args.content.as_str(),
            args.outline.as_str(),
            args.notes.as_str(),
            args.plan.as_str(),
            args.summary.as_str(),
        ])
        .trim()
        .to_string();
        if architecture.is_empty() || architecture == "untitled" {
            anyhow::bail!("execution package architecture is empty");
        }
        let default_title = default_chapter_title(&manifest.language, number);
        let title = first_non_empty(&[
            args.chapter_title.as_str(),
            args.title.as_str(),
            manifest
                .chapter_plans
                .iter()
                .find(|plan| plan.number == number)
                .map(|plan| plan.title.as_str())
                .unwrap_or(""),
            default_title.as_str(),
        ])
        .to_string();
        let now = now_iso();
        let mut plan_record = ChapterPlanRecord {
            number,
            title: title.clone(),
            path: format!("plans/{number:04}_{}.md", slugify(&title)),
            plan: plan.trim().to_string(),
            status: first_non_empty(&[args.status.as_str(), "planned"]).to_string(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let character_registrations = register_chapter_character_requests(
            &mut manifest,
            number,
            &args.new_character_requests,
        );
        let unresolved = governance::unresolved_character_request_ids(
            &args.new_character_requests,
            &character_registrations,
        );
        if !unresolved.is_empty() {
            anyhow::bail!(
                "chapter execution package contains unresolved character requests: {}",
                unresolved.join(", ")
            );
        }
        let mut execution_contract = chapter_execution_contract_v2_from_args(args);
        execution_contract.character_registrations = character_registrations.clone();
        plan_record = apply_character_registrations(plan_record, &character_registrations)?;
        execution_contract =
            apply_character_registrations(execution_contract, &character_registrations)?;
        let mut future_chapters =
            apply_character_registrations(args.future_chapters.clone(), &character_registrations)?;
        let expected_chapters = manifest
            .target_units
            .zip(manifest.chapter_unit_target)
            .and_then(|(target, per_chapter)| {
                longform_policy::expected_chapter_count(target, per_chapter)
            });
        let last_rolling_chapter = number
            .saturating_add(governance::ROLLING_OUTLINE_LOOKAHEAD_CHAPTERS)
            .min(expected_chapters.unwrap_or(usize::MAX));
        future_chapters.retain(|seed| {
            seed.number.is_some_and(|future_number| {
                future_number > number && future_number <= last_rolling_chapter
            }) && !seed.goal.trim().is_empty()
                && !seed.expected_turn.trim().is_empty()
        });
        future_chapters.sort_by_key(|seed| seed.number);
        future_chapters.dedup_by_key(|seed| seed.number);
        atomic_write_file(
            project_dir.join(&plan_record.path),
            render_plan_file(&plan_record, args.notes.trim()),
        )
        .await?;
        let contract_record = write_chapter_control_contract(
            &project_dir,
            &manifest,
            number,
            &plan_record.title,
            plan.trim(),
            args.notes.trim(),
            &args.key_facts,
            execution_contract,
        )
        .await?;
        let architecture_record = apply_character_registrations(
            ChapterArchitectureRecord {
                number,
                title,
                path: format!(
                    "plans/{number:04}_{}_architecture.md",
                    slugify(&plan_record.title)
                ),
                architecture: architecture.trim().to_string(),
                status: first_non_empty(&[args.status.as_str(), "architected"]).to_string(),
                created_at: now.clone(),
                updated_at: now,
            },
            &character_registrations,
        )?;
        atomic_write_file(
            project_dir.join(&architecture_record.path),
            render_architecture_file(&architecture_record, args.notes.trim()),
        )
        .await?;
        manifest.chapter_plans.retain(|plan| plan.number != number);
        manifest.chapter_plans.push(plan_record.clone());
        manifest.chapter_plans.sort_by_key(|plan| plan.number);
        upsert_chapter_contract_record(&mut manifest, contract_record.clone());
        manifest
            .chapter_architectures
            .retain(|item| item.number != number);
        manifest
            .chapter_architectures
            .push(architecture_record.clone());
        manifest
            .chapter_architectures
            .sort_by_key(|item| item.number);
        if let Some(bible) = manifest.story_bible.as_mut() {
            novel_bible::upsert_planned_chapter_goal(
                bible,
                number,
                args.summary.trim(),
                args.reveal.trim(),
                args.payoff_target.trim(),
            );
            for seed in &future_chapters {
                let Some(future_number) = seed.number else {
                    continue;
                };
                novel_bible::upsert_planned_chapter_goal(
                    bible,
                    future_number,
                    seed.goal.trim(),
                    seed.expected_turn.trim(),
                    "",
                );
            }
        }

        ensure_structured_contract_v2(&mut manifest);
        if !manifest.structured_contract_v2.has_authored_content() {
            anyhow::bail!("cannot seal chapter authority without an authored canonical contract");
        }
        let mut context_record = manifest
            .context_packages
            .iter()
            .find(|record| record.number == number)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot seal chapter authority before the base context package is persisted"
                )
            })?;
        if context_record.sealed {
            anyhow::bail!(
                "chapter authority is already sealed; invalidate it before replacing the execution package"
            );
        }
        let transient_context_path = context_record.path.clone();
        let transient_rules_path = context_record.rules_path.clone();
        let transient_trace_path = context_record.trace_path.clone();
        let saved_context =
            tokio::fs::read_to_string(project_dir.join(&transient_context_path)).await?;
        let mut saved_context: Value = serde_json::from_str(&saved_context)?;
        if saved_context
            .get("sealed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            anyhow::bail!(
                "chapter context artifact is already sealed but its manifest record is stale"
            );
        }
        governance::replace_character_request_ids_in_value(
            &mut saved_context,
            &character_registrations,
        );
        let project_context = saved_context
            .get("project_context")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("base context artifact has no project_context"))?;
        let context_package = serde_json::from_value::<governance::ContextPackage>(
            saved_context
                .get("context_package")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("base context artifact has no context_package"))?,
        )?;
        let rule_stack = serde_json::from_value::<governance::RuleStack>(
            saved_context
                .get("rule_stack")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("base context artifact has no rule_stack"))?,
        )?;
        let mut trace = serde_json::from_value::<governance::ChapterTrace>(
            saved_context
                .get("trace")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("base context artifact has no trace"))?,
        )?;
        trace.notes.push("sealed_authority=true".to_string());

        let canonical_contract = canonical_project_contract_projection(&manifest);
        let truth_as_of_chapter = approved_truth_snapshot(&project_dir, &manifest, number).await?;
        let truth_cutoff_chapter = number.saturating_sub(1);
        let working_context = working_context_without_contract_mirrors(project_context);
        let protected_coverage = governance::build_authority_coverage(
            number,
            &canonical_contract,
            &truth_as_of_chapter,
            &context_package,
            &rule_stack,
            &trace,
            &contract_record,
            &architecture_record,
        );
        if !protected_coverage.complete {
            anyhow::bail!(
                "chapter authority protected coverage is incomplete: {}",
                protected_coverage.missing_paths.join(", ")
            );
        }
        let protected_payload = json!({
            "chapter_number": number,
            "canonical_contract": canonical_contract,
            "truth_as_of_chapter": truth_as_of_chapter,
            "truth_cutoff_chapter": truth_cutoff_chapter,
            "working_context": working_context,
            "context_package": context_package,
            "rule_stack": rule_stack,
            "trace": trace,
            "chapter_plan": plan_record,
            "chapter_contract": contract_record,
            "chapter_architecture": architecture_record,
            "character_registrations": character_registrations
        });
        let authority_root_fingerprint = governance::authority_fingerprint(&protected_payload);
        let excluded_future_paths = vec![
            "manifest.truth_files".to_string(),
            "story_bible.structured_contract_v2".to_string(),
            "story_bible.narrative_graph.future_chapters".to_string(),
            format!("chapters.number>={number}"),
        ];
        let role_projections = governance::AuthorityRole::ALL
            .into_iter()
            .map(|role| {
                (
                    role,
                    governance::build_authority_projection(
                        role,
                        &protected_payload,
                        &excluded_future_paths,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let sealed_at = now_iso();
        let sealed_authority = governance::SealedChapterAuthority {
            schema_version: governance::sealed_authority_version().to_string(),
            chapter_number: number,
            canonical_contract,
            truth_as_of_chapter,
            truth_cutoff_chapter,
            context_package,
            rule_stack,
            trace,
            chapter_contract: contract_record.clone(),
            chapter_architecture: architecture_record.clone(),
            character_registrations: character_registrations.clone(),
            role_projections,
            authority_root_fingerprint: authority_root_fingerprint.clone(),
            protected_coverage: protected_coverage.clone(),
            sealed_at: sealed_at.clone(),
        };
        let authority_dir = project_dir.join("plans").join("authorities");
        tokio::fs::create_dir_all(&authority_dir).await?;
        let durable_context_path = format!("plans/authorities/chapter-{number:04}.authority.json");
        let durable_rules_path = format!("plans/authorities/chapter-{number:04}.rules.yaml");
        let durable_trace_path = format!("plans/authorities/chapter-{number:04}.trace.json");
        atomic_write_file(
            project_dir.join(&durable_context_path),
            serde_json::to_string_pretty(&sealed_authority)?,
        )
        .await?;
        atomic_write_file(
            project_dir.join(&durable_rules_path),
            governance::render_rule_stack_yaml(&sealed_authority.rule_stack),
        )
        .await?;
        atomic_write_file(
            project_dir.join(&durable_trace_path),
            serde_json::to_string_pretty(&sealed_authority.trace)?,
        )
        .await?;

        context_record.path = durable_context_path;
        context_record.rules_path = durable_rules_path;
        context_record.trace_path = durable_trace_path;
        context_record.authority_root_fingerprint = authority_root_fingerprint.clone();
        context_record.sealed = true;
        context_record.sealed_at = sealed_at;
        context_record.chapter_contract_fingerprint =
            governance::authority_fingerprint(&contract_record);
        context_record.canonical_contract_fingerprint =
            governance::authority_fingerprint(&sealed_authority.canonical_contract);
        context_record.truth_fingerprint =
            governance::authority_fingerprint(&sealed_authority.truth_as_of_chapter);
        context_record.truth_cutoff_chapter = truth_cutoff_chapter;
        context_record.role_projection_fingerprints = sealed_authority
            .role_projections
            .iter()
            .map(|(role, projection)| (role.as_str().to_string(), projection.fingerprint.clone()))
            .collect();
        context_record.protected_coverage = serde_json::to_value(&protected_coverage)?;
        context_record.excluded_future_paths = excluded_future_paths;
        upsert_context_package_record(&mut manifest, context_record.clone());
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        for transient_path in [
            transient_context_path,
            transient_rules_path,
            transient_trace_path,
        ] {
            if transient_path.starts_with("runtime/") {
                let _ = tokio::fs::remove_file(project_dir.join(transient_path)).await;
            }
        }

        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.checkpointed",
            "stage": pipeline::NovelPhase::ChapterExecutionPackage,
            "next_action": "write_draft",
            "project_path": project_dir.to_string_lossy(),
            "chapter_number": number,
            "chapter_plan": plan_record,
            "chapter_contract": contract_record,
            "chapter_architecture": architecture_record,
            "character_registrations": character_registrations,
            "future_chapters": future_chapters,
            "sealed_authority": sealed_authority,
            "authority_root_fingerprint": authority_root_fingerprint,
            "role_projection_fingerprints": context_record.role_projection_fingerprints,
            "protected_coverage": protected_coverage,
            "sealed": true,
            "plan_generated_from_contract": plan_generated_from_contract,
            "state": project_state_summary_light(&manifest),
            "audit": light_status_audit_manifest(&manifest)
        }))
    }
}
