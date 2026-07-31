use super::chapter_runtime::StreamProgressThrottleState;
use super::*;
use std::time::Instant;

pub(super) struct NovelChapterRunner {
    pub(super) agent: Arc<dyn MultiAgent>,
    pub(super) tool: NovelStudioTool,
    pub(super) project_path: String,
    pub(super) language: String,
    pub(super) chapter_unit_target: Option<usize>,
    pub(super) worker_label: String,
    pub(super) runtime: NovelWorkflowRuntimeState,
    pub(super) force_generation_after_target: bool,
    pub(super) completion_gate: Option<ProjectCompletionGateDecision>,
    pub(super) progress_throttle: Arc<Mutex<BTreeMap<String, StreamProgressThrottleState>>>,
    pub(super) chapter_context_cache: Arc<Mutex<BTreeMap<usize, Arc<SealedChapterAuthority>>>>,
}

enum MetadataRepairFlow {
    NotNeeded,
    Retry,
    Repaired,
    FallbackApplied,
}

async fn record_runner_parse_provenance(
    runtime: &NovelWorkflowRuntimeState,
    chapter_number: usize,
    stage: &str,
    provenance: novel_runner::ParseProvenance,
) {
    if matches!(
        provenance,
        novel_runner::ParseProvenance::ExactJson | novel_runner::ParseProvenance::StreamProtocol
    ) {
        return;
    }
    let checkpoint = format!("novel-chapter:{stage}:parse-recovery");
    record_workflow_checkpoint(
        runtime,
        chapter_number as u32,
        &checkpoint,
        format!(
            "第 {chapter_number} 章 {stage} 输出通过 {} 路径解析；结果将继续接受正文与元数据质量门检查。",
            provenance.as_str()
        ),
    )
    .await;
}

fn inject_sealed_future_boundary_finding(
    authority: &SealedChapterAuthority,
    body: &str,
    observation: &novel_runner::FinalChapterObservation,
    write_result: &mut Value,
    language: &str,
) -> bool {
    let Some((current_seed, next_seed, next_path)) =
        governance::sealed_current_and_next_chapter_seeds(authority)
    else {
        return false;
    };
    let cjk = language_looks_cjk(language);
    let required_character_anchors = governance::distinct_future_boundary_character_anchors(
        authority,
        &current_seed,
        &next_seed,
    );
    let Some((excerpt, source)) = sealed_future_boundary_evidence(
        body,
        &observation.future_boundary_evidence,
        &current_seed,
        &next_seed,
        cjk,
        &required_character_anchors,
    ) else {
        return false;
    };
    let next_number = authority.chapter_number.saturating_add(1);
    let Some(finding) = chapter_quality::future_chapter_consumed_finding(
        authority.chapter_number,
        next_number,
        next_path,
        next_seed,
        excerpt,
        source,
        &authority.authority_root_fingerprint,
        body,
    ) else {
        return false;
    };
    let Some(gate_value) = write_result.get_mut("quality_gate") else {
        return false;
    };
    let Ok(mut gate) =
        serde_json::from_value::<chapter_quality::ChapterQualityGate>(gate_value.clone())
    else {
        return false;
    };
    gate.extend_findings(vec![finding]);
    let Ok(serialized) = serde_json::to_value(gate) else {
        return false;
    };
    *gate_value = serialized;
    true
}

fn sealed_future_boundary_evidence(
    body: &str,
    observer_evidence: &str,
    current_seed: &str,
    next_seed: &str,
    cjk: bool,
    required_character_anchors: &[String],
) -> Option<(String, &'static str)> {
    if let Some(excerpt) = governance::validated_future_boundary_observer_evidence(
        body,
        observer_evidence,
        current_seed,
        next_seed,
        cjk,
        required_character_anchors,
    ) {
        return Some((excerpt, "final_body_observer+sealed_next_chapter_boundary"));
    }
    governance::final_body_future_consumption_evidence(
        body,
        current_seed,
        next_seed,
        cjk,
        required_character_anchors,
    )
    .map(|excerpt| {
        (
            excerpt,
            "final_body_completed_event+sealed_next_chapter_boundary",
        )
    })
}

fn apply_character_registrations_to_package(
    package: &mut novel_runner::ChapterExecutionPackage,
    registrations: &[ChapterCharacterRegistration],
) {
    if registrations.is_empty() {
        return;
    }
    for registration in registrations {
        let request_id = registration.request_id.trim();
        let name = registration.canonical_name.trim();
        if request_id.is_empty() || name.is_empty() {
            continue;
        }
        package.memo.goal = package.memo.goal.replace(request_id, name);
        package.memo.body = package.memo.body.replace(request_id, name);
        for section in &mut package.memo.sections {
            section.body = section.body.replace(request_id, name);
        }
        package.architecture = package.architecture.replace(request_id, name);
        package.scene_goal = package.scene_goal.replace(request_id, name);
        package.conflict = package.conflict.replace(request_id, name);
        package.choice = package.choice.replace(request_id, name);
        package.cost = package.cost.replace(request_id, name);
        package.reveal = package.reveal.replace(request_id, name);
        package.emotional_beat = package.emotional_beat.replace(request_id, name);
        package.chapter_function = package.chapter_function.replace(request_id, name);
        package.irreversible_event = package.irreversible_event.replace(request_id, name);
        package.new_state_after_chapter = package.new_state_after_chapter.replace(request_id, name);
        package.world_change = package.world_change.replace(request_id, name);
        package.character_change = package.character_change.replace(request_id, name);
        package.relationship_change = package.relationship_change.replace(request_id, name);
        package.power_delta = package.power_delta.replace(request_id, name);
        package.resource_delta = package.resource_delta.replace(request_id, name);
        for hook in &mut package.hook_opened {
            *hook = hook.replace(request_id, name);
        }
        for hook in &mut package.hook_paid_off {
            *hook = hook.replace(request_id, name);
        }
        package.title_basis = package.title_basis.replace(request_id, name);
        for seed in &mut package.future_chapters {
            seed.goal = seed.goal.replace(request_id, name);
            seed.expected_turn = seed.expected_turn.replace(request_id, name);
        }
    }
    let cast = registrations
        .iter()
        .filter(|registration| !registration.canonical_name.trim().is_empty())
        .map(|registration| {
            format!(
                "- {}：{}；用途：{}；范围：{}；关系：{}；欲望：{}；恐惧：{}；底线：{}；弧线：{} -> {}；声音：{}",
                registration.canonical_name.trim(),
                registration.role.trim(),
                registration.narrative_purpose.trim(),
                registration.importance.trim(),
                registration.relationship_to_existing.trim(),
                registration.desire.trim(),
                registration.fear.trim(),
                registration.bottom_line.trim(),
                registration.arc_start.trim(),
                registration.arc_end.trim(),
                registration.voice_style.trim()
            )
        })
        .collect::<Vec<_>>();
    if !cast.is_empty() {
        package.architecture.push_str("\n\n## 本章已登记新人物\n");
        package.architecture.push_str(&cast.join("\n"));
        package
            .architecture
            .push_str("\n正文只能使用这里已分配的姓名，不得把 request_id 写进正文。");
    }
}

fn execution_package_from_sealed_authority(
    authority: &SealedChapterAuthority,
    language: &str,
    project_title: &str,
    completion_gate: Option<&ProjectCompletionGateDecision>,
) -> anyhow::Result<novel_runner::ChapterExecutionPackage> {
    let writer_payload = authority
        .projection(AuthorityRole::Writer)
        .and_then(|projection| projection.payload.get("authority"))
        .ok_or_else(|| anyhow::anyhow!("sealed authority has no writer payload"))?;
    let writer_json = serde_json::to_string(writer_payload)?;
    let mut package = fallback_chapter_execution_package(
        language,
        project_title,
        authority.chapter_number,
        &writer_json,
        completion_gate.is_some(),
        completion_gate,
    );
    if let Some(plan) = writer_payload
        .pointer("/chapter_plan/plan")
        .and_then(Value::as_str)
        .filter(|plan| !plan.trim().is_empty())
    {
        package.memo = novel_runner::parse_memo(plan, language).unwrap_or_else(|_| {
            novel_runner::ChapterMemo {
                goal: authority.chapter_contract.goal.clone(),
                body: plan.to_string(),
                sections: Vec::new(),
            }
        });
    } else {
        package.memo.goal = authority.chapter_contract.goal.clone();
        package.memo.body = authority.chapter_contract.goal.clone();
    }
    package.architecture = authority.chapter_architecture.architecture.clone();
    package.scene_goal = authority.chapter_contract.scene_goal.clone();
    package.conflict = authority.chapter_contract.conflict.clone();
    package.choice = authority.chapter_contract.choice.clone();
    package.cost = authority.chapter_contract.cost.clone();
    package.reveal = authority.chapter_contract.reveal.clone();
    package.emotional_beat = authority.chapter_contract.emotional_beat.clone();
    package.chapter_function = authority.chapter_contract.payoff_target.clone();
    package.irreversible_event = authority.chapter_contract.reveal.clone();
    package.new_state_after_chapter = authority.chapter_contract.new_state_after_chapter.clone();
    package.world_change = authority.chapter_contract.world_change.clone();
    package.character_change = authority.chapter_contract.character_change.clone();
    package.relationship_change = authority.chapter_contract.relationship_delta.clone();
    package.power_delta = authority.chapter_contract.power_delta.clone();
    package.resource_delta = authority.chapter_contract.resource_delta.clone();
    package.hook_opened = authority.chapter_contract.hook_opened.clone();
    package.hook_paid_off = authority.chapter_contract.hook_paid_off.clone();
    package.new_character_requests = authority.chapter_contract.new_character_requests.clone();
    package.future_chapters.clear();
    package.degraded = false;
    package.degraded_reason.clear();
    Ok(package)
}

impl NovelChapterRunner {
    pub(super) fn tool(&self) -> &NovelStudioTool {
        &self.tool
    }

    fn cache_sealed_authority(
        &self,
        authority: SealedChapterAuthority,
    ) -> Arc<SealedChapterAuthority> {
        let chapter_number = authority.chapter_number;
        let authority = Arc::new(authority);
        if let Ok(mut cache) = self.chapter_context_cache.lock() {
            cache.insert(chapter_number, authority.clone());
        }
        authority
    }

    pub(super) async fn sealed_authority(
        &self,
        chapter_number: usize,
    ) -> anyhow::Result<Arc<SealedChapterAuthority>> {
        if let Ok(cache) = self.chapter_context_cache.lock() {
            if let Some(authority) = cache.get(&chapter_number) {
                return Ok(authority.clone());
            }
        }
        let packet = call_novel_studio_json_with_timeout(
            &self.tool,
            json!({
                "action": "compose_context",
                "project_path": self.project_path,
                "chapter_number": chapter_number
            }),
            local_tool_stage_timeout_secs(),
            "compose_context_authority_reuse",
        )
        .await?;
        let authority = serde_json::from_value::<SealedChapterAuthority>(
            packet
                .get("sealed_authority")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("chapter authority is not sealed"))?,
        )?;
        self.validate_sealed_authority(&authority)?;
        Ok(self.cache_sealed_authority(authority))
    }

    fn validate_sealed_authority(&self, authority: &SealedChapterAuthority) -> anyhow::Result<()> {
        let unresolved = governance::unresolved_character_request_ids(
            &authority.chapter_contract.new_character_requests,
            &authority.character_registrations,
        );
        if !unresolved.is_empty() {
            anyhow::bail!(
                "sealed authority contains unresolved character requests: {}",
                unresolved.join(", ")
            );
        }
        if !authority.protected_coverage.complete {
            anyhow::bail!(
                "sealed authority coverage is incomplete: {}",
                authority.protected_coverage.missing_paths.join(", ")
            );
        }
        for role in AuthorityRole::ALL {
            let projection = authority.projection(role).ok_or_else(|| {
                anyhow::anyhow!("sealed authority lacks {} projection", role.as_str())
            })?;
            if super::super::novel_governance::authority_fingerprint(&projection.payload)
                != projection.fingerprint
            {
                anyhow::bail!(
                    "sealed authority {} projection fingerprint mismatch",
                    role.as_str()
                );
            }
            let protected = projection
                .payload
                .get("authority")
                .ok_or_else(|| anyhow::anyhow!("sealed authority projection has no payload"))?;
            if super::super::novel_governance::authority_fingerprint(protected)
                != authority.authority_root_fingerprint
            {
                anyhow::bail!(
                    "sealed authority {} root fingerprint mismatch",
                    role.as_str()
                );
            }
            if projection.protected_core_fingerprint != authority.authority_root_fingerprint
                || !projection.truncated_paths.is_empty()
                || projection.included_paths.is_empty()
            {
                anyhow::bail!(
                    "sealed authority {} projection coverage trace is invalid",
                    role.as_str()
                );
            }
        }
        Ok(())
    }

    pub(super) async fn authority_projection_json(
        &self,
        chapter_number: usize,
        role: AuthorityRole,
    ) -> anyhow::Result<String> {
        let authority = self.sealed_authority(chapter_number).await?;
        let projection = authority.projection(role).ok_or_else(|| {
            anyhow::anyhow!("sealed authority lacks {} projection", role.as_str())
        })?;
        let protected_payload = projection
            .payload
            .get("authority")
            .ok_or_else(|| anyhow::anyhow!("sealed authority projection has no payload"))?;
        Ok(serde_json::to_string(
            &governance::model_authority_projection_payload(
                role,
                protected_payload,
                &authority.authority_root_fingerprint,
            ),
        )?)
    }

    pub(super) async fn authoritative_chapter_context_json(&self, chapter_number: usize) -> String {
        self.authority_projection_json(chapter_number, AuthorityRole::Reviser)
            .await
            .unwrap_or_default()
    }

    pub(super) async fn project_approved_target_reached(&self) -> anyhow::Result<bool> {
        project_approved_target_reached(&self.tool, &self.project_path).await
    }

    async fn apply_local_body_cleanup(
        &self,
        chapter_number: usize,
        draft: &mut novel_runner::DraftOutput,
        write_result: &mut Value,
        audit: &mut Value,
        phase: &'static str,
    ) -> anyhow::Result<bool> {
        let before_fingerprint = text_fingerprint(&draft.content);
        let issues = revision_issues(write_result, audit);
        let sanitized = sanitize_chapter_body_report(&draft.content, &draft.title, &self.language);
        draft.content = sanitized.text;
        let after_sanitize_fingerprint = text_fingerprint(&draft.content);
        let locally_repaired = apply_local_revision_suggestions(&draft.content, &issues);
        if text_fingerprint(&locally_repaired) != after_sanitize_fingerprint {
            draft.content = sanitize_chapter_body(&locally_repaired, &draft.title, &self.language);
        }
        let changed = text_fingerprint(&draft.content) != before_fingerprint;
        if !changed
            && !deterministic_cleanup_issues_are_stale_after_local_repair(&draft.content, &issues)
        {
            return Ok(false);
        }
        repair_draft_summary_after_body_cleanup(draft, &self.language);
        let label = format!("novel-chapter:{phase}:local-cleanup");
        record_workflow_checkpoint(
            &self.runtime,
            chapter_number as u32,
            &label,
            if changed {
                format!("第 {chapter_number} 章已先执行本地文字表面清理，再重新进入质量门。")
            } else {
                format!("第 {chapter_number} 章本地文字表面问题疑似为过期审核项，正文不变并重新进入质量门。")
            },
        )
        .await;
        *write_result = call_novel_studio_json(
            &self.tool,
            json!({
                "action": "revise_draft",
                "candidate_only": true,
                "project_path": self.project_path,
                "chapter_number": chapter_number,
                "chapter_title": draft.title.clone(),
                "content": draft.content.clone(),
                "summary": draft.summary.clone(),
                "key_facts": draft.key_facts.clone(),
                "continuity_updates": draft.continuity_updates.clone()
            }),
        )
        .await?;
        *audit = self
            .rule_first_audit_or_full_audit(chapter_number, write_result)
            .await?;
        Ok(true)
    }

    pub(super) async fn run_chapter(
        &self,
        request: &ContinuousStepRequest,
    ) -> anyhow::Result<String> {
        self.ensure_not_cancelled().await?;
        let chapter_number = request.step.index;
        let chapter_started_at = Instant::now();
        if self.chapter_already_approved(chapter_number).await? {
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:skip-approved",
                format!("第 {chapter_number} 章已经通过审查，跳过重写。"),
            )
            .await;
            return Ok(format!(
                "chapter {chapter_number} skipped; already approved; no rewrite"
            ));
        }
        let reusable_existing_draft = self
            .read_reusable_unapproved_chapter(chapter_number)
            .await?;
        let existing_chapter_title = reusable_existing_draft
            .as_ref()
            .map(|draft| draft.title.clone())
            .filter(|title| !title.trim().is_empty());
        if request.attempt > 0 && reusable_existing_draft.is_some() {
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:retry:reuse-draft",
                format!(
                    "第 {chapter_number} 章进入第 {} 次 step retry；保留未批准候选并由统一有限修订控制器重新评估。",
                    request.attempt + 1
                ),
            )
            .await;
        }
        record_workflow_checkpoint(
            &self.runtime,
            chapter_number as u32,
            "novel-chapter:context:start",
            format!("正在为第 {chapter_number} 章组装轻量上下文包。"),
        )
        .await;
        let context_started_at = Instant::now();
        let mut context_packet = call_novel_studio_json_with_timeout(
            &self.tool,
            json!({
                "action": "compose_context",
                "project_path": self.project_path,
                "chapter_number": chapter_number
            }),
            local_tool_stage_timeout_secs(),
            "compose_context",
        )
        .await?;
        if reusable_existing_draft.is_some() && context_packet.get("sealed_authority").is_none() {
            let migration = call_novel_studio_json_with_timeout(
                &self.tool,
                json!({
                    "action": "repair_project_state",
                    "project_path": self.project_path,
                    "chapter_number": chapter_number
                }),
                local_tool_stage_timeout_secs(),
                "reconstruct_legacy_unapproved_authority",
            )
            .await?;
            let migrated = migration
                .get("migrated_legacy_candidates")
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item.get("chapter_number").and_then(Value::as_u64)
                            == Some(chapter_number as u64)
                    })
                });
            if migrated {
                context_packet = call_novel_studio_json_with_timeout(
                    &self.tool,
                    json!({
                        "action": "compose_context",
                        "project_path": self.project_path,
                        "chapter_number": chapter_number
                    }),
                    local_tool_stage_timeout_secs(),
                    "compose_context_after_legacy_migration",
                )
                .await?;
                record_workflow_checkpoint(
                    &self.runtime,
                    chapter_number as u32,
                    "novel-chapter:legacy-authority:reconstructed",
                    format!(
                        "第 {chapter_number} 章既有未批准正文已从原计划、合同和架构重建密封权威，并登记为统一修订候选。"
                    ),
                )
                .await;
            }
        }
        let base_context_json = compact_context_json(&context_packet)?;
        let context_char_count = base_context_json.chars().count();
        let title =
            context_project_title(&context_packet).unwrap_or_else(|| "Untitled".to_string());
        record_workflow_checkpoint(
            &self.runtime,
            chapter_number as u32,
            "novel-chapter:context:ready",
            format!(
                "第 {chapter_number} 章上下文包已就绪，准备生成章节执行包；compact_context_chars={context_char_count}; context_ms={}。",
                context_started_at.elapsed().as_millis()
            ),
        )
        .await;
        let package_started_at = Instant::now();
        let existing_authority = context_packet
            .get("sealed_authority")
            .cloned()
            .map(serde_json::from_value::<SealedChapterAuthority>)
            .transpose()?;
        let (package, authority) = if let Some(authority) = existing_authority {
            self.validate_sealed_authority(&authority)?;
            let package = execution_package_from_sealed_authority(
                &authority,
                &self.language,
                &title,
                self.completion_gate.as_ref(),
            )?;
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:execution-package:sealed-reuse",
                format!("第 {chapter_number} 章复用已封存执行包，未重新生成或覆盖章节权威。"),
            )
            .await;
            (package, authority)
        } else {
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:execution-package:start",
                format!("正在生成第 {chapter_number} 章执行包。"),
            )
            .await;
            let mut package = self
                .generate_chapter_execution_package(
                    chapter_number,
                    &title,
                    &base_context_json,
                    request.previous_error.as_deref(),
                )
                .await?;
            if package.degraded {
                anyhow::bail!(
                    "chapter execution package is degraded and cannot be sealed: {}",
                    preview_text(&package.degraded_reason, 220)
                );
            }
            self.ensure_not_cancelled().await?;
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:execution-package:persist:start",
                format!("正在持久化并封存第 {chapter_number} 章执行包。"),
            )
            .await;
            let value = call_novel_studio_json_with_timeout(
                &self.tool,
                json!({
                    "action": "persist_execution_package",
                    "project_path": self.project_path,
                    "chapter_number": chapter_number,
                    "plan": package.memo.body.clone(),
                    "summary": package.scene_goal.clone(),
                    "content": package.architecture.clone(),
                    "notes": package.title_basis.clone(),
                    "scene_goal": package.scene_goal.clone(),
                    "conflict": package.conflict.clone(),
                    "choice": package.choice.clone(),
                    "cost": package.cost.clone(),
                    "reveal": package.reveal.clone(),
                    "emotional_beat": package.emotional_beat.clone(),
                    "power_delta": package.power_delta.clone(),
                    "resource_delta": package.resource_delta.clone(),
                    "hook_opened": package.hook_opened.clone(),
                    "hook_paid_off": package.hook_paid_off.clone(),
                    "new_state_after_chapter": package.new_state_after_chapter.clone(),
                    "world_change": package.world_change.clone(),
                    "character_change": package.character_change.clone(),
                    "relationship_delta": package.relationship_change.clone(),
                    "payoff_target": package.chapter_function.clone(),
                    "future_chapters": package.future_chapters.clone(),
                    "new_character_requests": package.new_character_requests.clone()
                }),
                local_tool_stage_timeout_secs(),
                "persist_execution_package",
            )
            .await?;
            let registrations = serde_json::from_value::<Vec<ChapterCharacterRegistration>>(
                value
                    .get("character_registrations")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            )?;
            let unresolved = governance::unresolved_character_request_ids(
                &package.new_character_requests,
                &registrations,
            );
            if !unresolved.is_empty() {
                anyhow::bail!(
                    "chapter execution package contains unresolved character requests: {}",
                    unresolved.join(", ")
                );
            }
            apply_character_registrations_to_package(&mut package, &registrations);
            let authority = serde_json::from_value::<SealedChapterAuthority>(
                value.get("sealed_authority").cloned().ok_or_else(|| {
                    anyhow::anyhow!("persisted execution package did not return sealed authority")
                })?,
            )?;
            self.validate_sealed_authority(&authority)?;
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:execution-package:persist:completed",
                format!(
                    "第 {chapter_number} 章执行包已持久化并封存；新增人物登记 {} 项；package_ms={}。",
                    registrations.len(),
                    package_started_at.elapsed().as_millis()
                ),
            )
            .await;
            (package, authority)
        };
        let authority = self.cache_sealed_authority(authority);
        let writer_projection = authority
            .projection(AuthorityRole::Writer)
            .ok_or_else(|| anyhow::anyhow!("sealed authority has no writer projection"))?;
        let writer_payload = writer_projection
            .payload
            .get("authority")
            .ok_or_else(|| anyhow::anyhow!("sealed authority writer projection has no payload"))?;
        let context_json = serde_json::to_string(&governance::model_authority_projection_payload(
            AuthorityRole::Writer,
            writer_payload,
            &authority.authority_root_fingerprint,
        ))?;
        let character_authority =
            novel_runner::CharacterAuthority::from_context(&writer_projection.payload);

        let memo = package.memo.clone();
        let draft_started_at = Instant::now();
        let recovered_candidate = self
            .recover_last_accepted_candidate(chapter_number, &authority.authority_root_fingerprint);
        let recovered_best = recovered_candidate
            .as_ref()
            .map(|(_, candidate)| candidate.clone());
        let mut draft = if let Some((path, candidate)) = recovered_candidate {
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:candidate:recovered-best",
                format!(
                    "第 {chapter_number} 章从持久化候选记录恢复最后一个 accepted_as_best 版本；path={path}。"
                ),
            )
            .await;
            candidate.draft
        } else if let Some(existing) = reusable_existing_draft.as_ref() {
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:candidate:reuse-existing",
                format!("第 {chapter_number} 章将既有未批准正文作为统一修订控制器的初始候选。"),
            )
            .await;
            existing.clone()
        } else {
            let mut writer_prompt = novel_runner::writer_prompt(
                &self.language,
                &title,
                chapter_number,
                self.chapter_unit_target,
                &memo,
                &package.architecture,
                &context_json,
                &character_authority,
            );
            if let Some(gate) = self.completion_gate.as_ref() {
                writer_prompt.push_str("\n\n");
                writer_prompt.push_str(&finale_execution_directive(gate, &self.language));
            }
            if let Some(previous_error) = request.previous_error.as_deref() {
                if !previous_error.trim().is_empty() {
                    if language_looks_cjk(&self.language) {
                        writer_prompt.push_str("\n\n上一候选被拒绝的问题：\n");
                        writer_prompt.push_str(previous_error);
                        writer_prompt
                            .push_str("\n请生成一个新的语义候选，且全部创作字段继续使用中文。");
                    } else {
                        writer_prompt.push_str("\n\nPrevious rejected candidate issue:\n");
                        writer_prompt.push_str(previous_error);
                        writer_prompt.push_str("\nGenerate one new semantic candidate.");
                    }
                }
            }
            let writer_prompt = clean_provider_prompt(&writer_prompt);
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:draft:start",
                format!("第 {chapter_number} 章开始调用 worker 模型生成初始正文候选。"),
            )
            .await;
            let draft_output = novel_runner::generate_draft(
                &self.agent,
                &writer_prompt,
                initial_chapter_generation_limits(self.chapter_unit_target, &self.language),
                self.progress_sink(chapter_number, "draft"),
                chapter_number,
                &self.language,
            )
            .await?;
            record_runner_parse_provenance(
                &self.runtime,
                chapter_number,
                "draft",
                draft_output.provenance,
            )
            .await;
            self.ensure_not_cancelled().await?;
            let generated = draft_output.value;
            if generated.degraded {
                record_workflow_checkpoint(
                    &self.runtime,
                    chapter_number as u32,
                    "novel-chapter:draft:truncated-candidate",
                    format!(
                        "第 {chapter_number} 章初始输出未满足结构化合同，以 truncated candidate 进入一次有界补尾：{}",
                        preview_text(&generated.degraded_reason, 220)
                    ),
                )
                .await;
            }
            generated
        };
        if let Some(title) = existing_chapter_title.as_ref() {
            draft.title = title.clone();
        }
        draft.content = sanitize_chapter_body(&draft.content, &draft.title, &self.language);
        repair_draft_summary_after_body_cleanup(&mut draft, &self.language);
        let mut current_draft = draft.clone();
        record_workflow_checkpoint(
            &self.runtime,
            chapter_number as u32,
            "novel-chapter:draft:ready",
            format!(
                "第 {chapter_number} 章正文草稿已生成；draft_ms={}; draft_units={}。",
                draft_started_at.elapsed().as_millis(),
                count_chapter_units(&current_draft.content, &self.language)
            ),
        )
        .await;
        let draft_action = if reusable_existing_draft.is_some() {
            "revise_draft"
        } else {
            "write_draft"
        };
        let mut write_result = call_novel_studio_json(
            &self.tool,
            json!({
                "action": draft_action,
                "project_path": self.project_path,
                "chapter_number": chapter_number,
                "chapter_title": draft.title,
                "content": draft.content,
                "summary": draft.summary,
                "key_facts": draft.key_facts,
                "continuity_updates": draft.continuity_updates
            }),
        )
        .await?;
        let persisted_chapter = call_novel_studio_json(
            &self.tool,
            json!({
                "action": "read_chapter",
                "project_path": self.project_path,
                "chapter_number": chapter_number
            }),
        )
        .await?;
        align_draft_with_studio_result(&mut current_draft, &persisted_chapter);

        let audit_started_at = Instant::now();
        let mut audit = self
            .rule_first_audit_or_full_audit(chapter_number, &write_result)
            .await?;
        record_workflow_checkpoint(
            &self.runtime,
            chapter_number as u32,
            "novel-chapter:audit:ready",
            format!(
                "第 {chapter_number} 章初审已完成；audit_ms={}。",
                audit_started_at.elapsed().as_millis()
            ),
        )
        .await;
        let initial_findings = findings_from_results(&write_result, &audit);
        let persisted_state =
            self.recover_revision_state(chapter_number, &authority.authority_root_fingerprint);
        let mut revision_cycle = self
            .run_bounded_revision_cycle(
                &authority,
                current_draft.clone(),
                &initial_findings,
                persisted_state,
                recovered_best,
            )
            .await?;
        if request.attempt == 0
            && (body_revision_required_after_audit(&write_result, &audit)
                || chapter_body_has_degenerate_repetition(&current_draft.content, &self.language))
        {
            let issues = revision_issue_summary(&write_result, &audit);
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:draft:route-to-revision",
                format!(
                    "第 {chapter_number} 章初稿存在正文退化，保留草稿并进入同章修订/恢复链路；issues={}",
                    preview_text(&issues, 320)
                ),
            )
            .await;
        }
        let mut attempted_length_topup = revision_cycle.state.budget.length_topup_attempted;
        let mut tail_completion_attempts =
            usize::from(revision_cycle.state.budget.tail_completion_attempted);
        let mut last_tail_completion_fingerprint = None;
        let mut last_deterministic_cleanup_fingerprint = None;
        let mut metadata_repair_attempts = revision_cycle.state.budget.metadata_repair_attempts;
        let mut rejected_metadata_titles = Vec::new();
        let mut accepted_observation = None;
        let persisted_revision_attempts = revision_cycle
            .state
            .budget
            .semantic_attempts
            .min(MAX_LLM_REVISION_ATTEMPTS);
        let mut revision_index = persisted_revision_attempts;
        loop {
            self.ensure_not_cancelled().await?;
            let cleanup_fingerprint = text_fingerprint(&current_draft.content);
            if !body_revision_required_after_audit(&write_result, &audit)
                && accepted_observation
                    .as_ref()
                    .is_none_or(|(fingerprint, _)| *fingerprint != cleanup_fingerprint)
            {
                if let Ok(observation) = self
                    .observe_final_chapter_state(chapter_number, &write_result, None)
                    .await
                {
                    if inject_sealed_future_boundary_finding(
                        &authority,
                        &current_draft.content,
                        &observation,
                        &mut write_result,
                        &self.language,
                    ) {
                        accepted_observation = None;
                        let findings = findings_from_results(&write_result, &audit);
                        revision_cycle.best_candidate.quality_vector = revision_quality_vector(
                            &authority,
                            &current_draft,
                            &findings,
                            None,
                            &[],
                            self.chapter_unit_target,
                            &self.language,
                        );
                        revision_cycle.best_candidate.findings = findings;
                        continue;
                    }
                    accepted_observation = Some((cleanup_fingerprint, observation));
                }
            }
            let durable_body_fingerprint =
                chapter_quality::chapter_body_fingerprint(&current_draft.content);
            let cleanup_already_attempted = !revision_cycle
                .state
                .budget
                .can_cleanup(&durable_body_fingerprint);
            let effective_last_cleanup_fingerprint = if cleanup_already_attempted {
                Some(cleanup_fingerprint)
            } else {
                last_deterministic_cleanup_fingerprint
            };
            let tail_completion_attempted_for_current_body = tail_completion_attempts
                >= MAX_TAIL_COMPLETION_RECOVERIES
                || last_tail_completion_fingerprint == Some(cleanup_fingerprint);
            match decide_chapter_loop_step(ChapterLoopDecisionInput {
                write_result: &write_result,
                audit: &audit,
                body_fingerprint: cleanup_fingerprint,
                last_cleanup_fingerprint: effective_last_cleanup_fingerprint,
                attempted_tail_completion: tail_completion_attempted_for_current_body,
                attempted_length_topup,
                chapter_unit_target: self.chapter_unit_target,
                language: &self.language,
            }) {
                ChapterLoopDecision::BlockRevision => {
                    return Ok(format_revision_blocker_result(
                        &self.project_path,
                        chapter_number,
                        &write_result,
                        &audit,
                    ));
                }
                ChapterLoopDecision::MetadataRepair => {
                    match self
                        .repair_metadata_gate_once(
                            chapter_number,
                            &mut current_draft,
                            &mut write_result,
                            &mut audit,
                            &mut metadata_repair_attempts,
                            &mut rejected_metadata_titles,
                            MAX_METADATA_REPAIR_ATTEMPTS,
                        )
                        .await?
                    {
                        MetadataRepairFlow::FallbackApplied => break,
                        MetadataRepairFlow::Retry => {
                            revision_cycle.state.budget.metadata_repair_attempts =
                                metadata_repair_attempts;
                            continue;
                        }
                        MetadataRepairFlow::Repaired => {
                            revision_cycle.state.budget.metadata_repair_attempts =
                                metadata_repair_attempts;
                            let (draft, write, reviewed, _, _) = self
                                .reconcile_submitted_candidate(
                                    &authority,
                                    chapter_number,
                                    current_draft,
                                    write_result,
                                    audit,
                                    CandidateProvenance::MetadataRepair,
                                    &mut revision_cycle,
                                )
                                .await?;
                            current_draft = draft;
                            write_result = write;
                            audit = reviewed;
                            continue;
                        }
                        MetadataRepairFlow::NotNeeded => {}
                    }
                }
                ChapterLoopDecision::Accept => break,
                ChapterLoopDecision::TailCompletion => {
                    last_tail_completion_fingerprint = Some(cleanup_fingerprint);
                    tail_completion_attempts += 1;
                    revision_cycle.state.budget.tail_completion_attempted = true;
                    if self
                        .apply_tail_completion_recovery(
                            chapter_number,
                            &mut current_draft,
                            &mut write_result,
                            &mut audit,
                            "tail-completion",
                        )
                        .await?
                    {
                        let (draft, write, reviewed, _, _) = self
                            .reconcile_submitted_candidate(
                                &authority,
                                chapter_number,
                                current_draft,
                                write_result,
                                audit,
                                CandidateProvenance::TailCompletion,
                                &mut revision_cycle,
                            )
                            .await?;
                        current_draft = draft;
                        write_result = write;
                        audit = reviewed;
                    }
                    continue;
                }
                ChapterLoopDecision::LocalCleanup => {
                    last_deterministic_cleanup_fingerprint = Some(cleanup_fingerprint);
                    revision_cycle
                        .state
                        .budget
                        .local_cleanup_fingerprints
                        .insert(chapter_quality::chapter_body_fingerprint(
                            &current_draft.content,
                        ));
                    if self
                        .apply_local_body_cleanup(
                            chapter_number,
                            &mut current_draft,
                            &mut write_result,
                            &mut audit,
                            "draft",
                        )
                        .await?
                    {
                        let (draft, write, reviewed, _, _) = self
                            .reconcile_submitted_candidate(
                                &authority,
                                chapter_number,
                                current_draft,
                                write_result,
                                audit,
                                CandidateProvenance::LocalCleanup,
                                &mut revision_cycle,
                            )
                            .await?;
                        current_draft = draft;
                        write_result = write;
                        audit = reviewed;
                        if !needs_revision(&write_result) && audit_passed(&audit) {
                            break;
                        }
                        continue;
                    }
                    if only_local_cleanup_issues(&write_result, &audit) {
                        break;
                    }
                }
                ChapterLoopDecision::LengthTopup => {
                    attempted_length_topup = true;
                    revision_cycle.state.budget.length_topup_attempted = true;
                    let before_units = count_chapter_units(&current_draft.content, &self.language);
                    let topped = self
                        .expand_short_chapter_if_needed(
                            chapter_number,
                            current_draft.clone(),
                            "length-topup",
                        )
                        .await?;
                    let after_units = count_chapter_units(&topped.content, &self.language);
                    if after_units <= before_units {
                        break;
                    }
                    current_draft = topped;
                    repair_draft_summary_after_body_cleanup(&mut current_draft, &self.language);
                    write_result = call_novel_studio_json(
                        &self.tool,
                        json!({
                            "action": "revise_draft",
                            "candidate_only": true,
                            "project_path": self.project_path,
                            "chapter_number": chapter_number,
                            "chapter_title": current_draft.title,
                            "content": current_draft.content,
                            "summary": current_draft.summary,
                            "key_facts": current_draft.key_facts,
                            "continuity_updates": current_draft.continuity_updates
                        }),
                    )
                    .await?;
                    audit = self
                        .rule_first_audit_or_full_audit(chapter_number, &write_result)
                        .await?;
                    let (draft, write, reviewed, _, _) = self
                        .reconcile_submitted_candidate(
                            &authority,
                            chapter_number,
                            current_draft,
                            write_result,
                            audit,
                            CandidateProvenance::LengthTopup,
                            &mut revision_cycle,
                        )
                        .await?;
                    current_draft = draft;
                    write_result = write;
                    audit = reviewed;
                    continue;
                }
                ChapterLoopDecision::StopForFinalCleanup => {
                    break;
                }
                ChapterLoopDecision::LlmRevision => {}
            }
            // Metadata and local-cleanup routes may discover that their
            // deterministic repair is not applicable while body blockers still
            // require a semantic rewrite. Those routes fall through to the same
            // reviser below, so the shared persisted budget must guard this
            // common entry point rather than only the explicit LlmRevision arm.
            if !revision_cycle.state.budget.can_attempt_semantic_revision() {
                return Ok(format_revision_blocker_result(
                    &self.project_path,
                    chapter_number,
                    &write_result,
                    &audit,
                ));
            }
            let revision_issues = revision_issues(&write_result, &audit);
            let revision_mode = revision_mode_for_results(&write_result, &audit);
            let mut reviser_prompt = novel_runner::reviser_prompt(
                &self.language,
                &title,
                chapter_number,
                self.chapter_unit_target,
                &memo,
                &package.architecture,
                &context_json,
                &current_draft.content,
                &revision_issues,
                revision_mode,
                &character_authority,
            );
            if let Some(gate) = self.completion_gate.as_ref() {
                reviser_prompt.push_str("\n\n");
                reviser_prompt.push_str(&finale_execution_directive(gate, &self.language));
            }
            reviser_prompt.push_str(&revision_guidance(
                revision_index + 1,
                &write_result,
                &audit,
                &self.language,
                revision_mode,
            ));
            let reviser_prompt = clean_provider_prompt(&reviser_prompt);
            revision_cycle.state.budget.semantic_attempts += 1;
            revision_index += 1;
            let revised_output = novel_runner::generate_draft(
                &self.agent,
                &reviser_prompt,
                chapter_generation_limits(self.chapter_unit_target, &self.language),
                self.progress_sink(chapter_number, "revise"),
                chapter_number,
                &self.language,
            )
            .await?;
            record_runner_parse_provenance(
                &self.runtime,
                chapter_number,
                "revise",
                revised_output.provenance,
            )
            .await;
            self.ensure_not_cancelled().await?;
            let mut revised = revised_output.value;
            let mut candidate_provenance = CandidateProvenance::SemanticRevision;
            if revised.degraded {
                candidate_provenance = CandidateProvenance::TruncatedRecovery;
                if !revision_cycle.state.budget.tail_completion_attempted
                    && !revised.content.trim().is_empty()
                    && !chapter_body_has_tool_or_json_residue(&revised.content)
                {
                    revision_cycle.state.budget.tail_completion_attempted = true;
                    tail_completion_attempts = 1;
                    revised = self
                        .complete_unfinished_chapter_tail_if_needed(
                            chapter_number,
                            revised,
                            "truncated-revision-tail",
                        )
                        .await?;
                }
                if !draft_output_fallback_body_is_usable(
                    &revised,
                    self.chapter_unit_target,
                    &self.language,
                ) {
                    record_workflow_checkpoint(
                        &self.runtime,
                        chapter_number as u32,
                        "novel-chapter:revision-rejected:truncated",
                        format!(
                            "第 {chapter_number} 章 truncated candidate 在一次有界补尾后仍不可用，保留当前 best。"
                        ),
                    )
                    .await;
                    return Ok(format_revision_blocker_result(
                        &self.project_path,
                        chapter_number,
                        &write_result,
                        &audit,
                    ));
                }
                revised.degraded = false;
            }
            if !current_draft.title.trim().is_empty() {
                revised.title = current_draft.title.clone();
            }
            revised.content =
                sanitize_chapter_body(&revised.content, &revised.title, &self.language);
            repair_draft_summary_after_body_cleanup(&mut revised, &self.language);
            current_draft = revised.clone();
            write_result = call_novel_studio_json(
                &self.tool,
                json!({
                    "action": "revise_draft",
                    "candidate_only": true,
                    "project_path": self.project_path,
                    "chapter_number": chapter_number,
                    "chapter_title": revised.title,
                    "content": revised.content,
                    "summary": revised.summary,
                    "key_facts": revised.key_facts,
                    "continuity_updates": revised.continuity_updates
                }),
            )
            .await?;
            audit = self
                .rule_first_audit_or_full_audit(chapter_number, &write_result)
                .await?;
            let (candidate_draft, candidate_write, candidate_audit, accepted_as_best, _) = self
                .reconcile_submitted_candidate(
                    &authority,
                    chapter_number,
                    current_draft,
                    write_result,
                    audit,
                    candidate_provenance,
                    &mut revision_cycle,
                )
                .await?;
            current_draft = candidate_draft;
            write_result = candidate_write;
            audit = candidate_audit;
            if !accepted_as_best {
                record_workflow_checkpoint(
                    &self.runtime,
                    chapter_number as u32,
                    "novel-chapter:revision-rollback:non-improving",
                    format!(
                        "第 {chapter_number} 章候选没有满足确定性净提升约束，已回滚到 accepted_as_best 候选；candidate={}。",
                        revision_cycle.best_candidate.candidate_id
                    ),
                )
                .await;
                continue;
            }
            let current_tail_fingerprint = text_fingerprint(&current_draft.content);
            if tail_completion_attempts < MAX_TAIL_COMPLETION_RECOVERIES
                && last_tail_completion_fingerprint != Some(current_tail_fingerprint)
                && revision_issues_include_tail_completion(&write_result, &audit)
            {
                last_tail_completion_fingerprint = Some(current_tail_fingerprint);
                tail_completion_attempts += 1;
                revision_cycle.state.budget.tail_completion_attempted = true;
                if self
                    .apply_tail_completion_recovery(
                        chapter_number,
                        &mut current_draft,
                        &mut write_result,
                        &mut audit,
                        "revise-tail-recovery",
                    )
                    .await?
                {
                    let (draft, write, reviewed, _, _) = self
                        .reconcile_submitted_candidate(
                            &authority,
                            chapter_number,
                            current_draft,
                            write_result,
                            audit,
                            CandidateProvenance::TailCompletion,
                            &mut revision_cycle,
                        )
                        .await?;
                    current_draft = draft;
                    write_result = write;
                    audit = reviewed;
                }
                continue;
            }
            if audit_next_action_blocked(&audit)
                && !only_local_cleanup_issues(&write_result, &audit)
                && !body_revision_required_after_audit(&write_result, &audit)
                && !revision_cycle.state.budget.can_attempt_semantic_revision()
            {
                return Ok(format_revision_blocker_result(
                    &self.project_path,
                    chapter_number,
                    &write_result,
                    &audit,
                ));
            }
        }

        if body_revision_required_after_audit(&write_result, &audit) {
            return Ok(format_revision_blocker_result(
                &self.project_path,
                chapter_number,
                &write_result,
                &audit,
            ));
        }
        let settlement = self
            .settle_observed_final_chapter_state(
                chapter_number,
                &write_result,
                accepted_observation.and_then(|(fingerprint, observation)| {
                    (fingerprint == text_fingerprint(&current_draft.content)).then_some(observation)
                }),
            )
            .await?;
        if !settlement
            .pointer("/validation/passed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(format_state_repair_blocker_result(
                &self.project_path,
                chapter_number,
                &settlement,
            ));
        }
        let validation = call_novel_studio_json(
            &self.tool,
            json!({
                "action": "validate_chapter_state",
                "project_path": self.project_path,
                "chapter_number": chapter_number
            }),
        )
        .await?;
        let validation_passed = validation
            .pointer("/validation/passed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if validation_passed {
            let approval = self
                .approve_chapter_after_validation(
                    chapter_number,
                    &mut current_draft,
                    &mut write_result,
                    &mut audit,
                )
                .await?;
            if !approval_result_is_approved(&approval) {
                return Ok(format_revision_blocker_result(
                    &self.project_path,
                    chapter_number,
                    &write_result,
                    &audit,
                ));
            }
        } else {
            return Ok(format_state_repair_blocker_result(
                &self.project_path,
                chapter_number,
                &validation,
            ));
        }

        let readback = call_novel_studio_json(
            &self.tool,
            json!({
                "action": "read_chapter",
                "project_path": self.project_path,
                "chapter_number": chapter_number
            }),
        )
        .await
        .ok();
        let artifact_path = readback
            .as_ref()
            .and_then(|value| value.get("artifact_path").and_then(Value::as_str))
            .or_else(|| {
                readback
                    .as_ref()
                    .and_then(|value| value.pointer("/chapter/path").and_then(Value::as_str))
            })
            .or_else(|| {
                write_result
                    .get("txt_artifact_path")
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                write_result
                    .get("preferred_artifact_path")
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                write_result
                    .pointer("/readable_export/current_path")
                    .and_then(Value::as_str)
            })
            .or_else(|| write_result.get("artifact_path").and_then(Value::as_str))
            .unwrap_or("");
        let unit_count = readback
            .as_ref()
            .and_then(|value| value.get("unit_count").and_then(Value::as_u64))
            .or_else(|| {
                readback
                    .as_ref()
                    .and_then(|value| value.pointer("/chapter/unit_count").and_then(Value::as_u64))
            })
            .or_else(|| write_result.get("unit_count").and_then(Value::as_u64))
            .unwrap_or(0);
        let total_units = readback
            .as_ref()
            .and_then(|value| {
                value
                    .pointer("/state/approved_units")
                    .and_then(Value::as_u64)
            })
            .or_else(|| {
                readback
                    .as_ref()
                    .and_then(|value| value.get("total_units").and_then(Value::as_u64))
            })
            .or_else(|| write_result.get("total_units").and_then(Value::as_u64))
            .unwrap_or(0);
        let audit_status = audit
            .pointer("/review/verdict")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let chapter_ms = chapter_started_at.elapsed().as_millis();
        record_workflow_checkpoint(
            &self.runtime,
            chapter_number as u32,
            "novel-chapter:completed",
            format!(
                "第 {chapter_number} 章已批准并写入产物；chapter_ms={chapter_ms}; unit_count={unit_count}; total_units={total_units}。"
            ),
        )
        .await;
        Ok(format!(
            "chapter {chapter_number} saved; path={artifact_path}; unit_count={unit_count}; total_units={total_units}; audit={audit_status}; chapter_ms={chapter_ms}"
        ))
    }

    async fn apply_tail_completion_recovery(
        &self,
        chapter_number: usize,
        draft: &mut novel_runner::DraftOutput,
        write_result: &mut Value,
        audit: &mut Value,
        phase: &'static str,
    ) -> anyhow::Result<bool> {
        let before_fingerprint = text_fingerprint(&draft.content);
        let completed = self
            .complete_unfinished_chapter_tail_if_needed(chapter_number, draft.clone(), phase)
            .await?;
        if text_fingerprint(&completed.content) == before_fingerprint {
            return Ok(false);
        }
        *draft = completed;
        repair_draft_summary_after_body_cleanup(draft, &self.language);
        *write_result = call_novel_studio_json(
            &self.tool,
            json!({
                "action": "revise_draft",
                "candidate_only": true,
                "project_path": self.project_path,
                "chapter_number": chapter_number,
                "chapter_title": draft.title.clone(),
                "content": draft.content.clone(),
                "summary": draft.summary.clone(),
                "key_facts": draft.key_facts.clone(),
                "continuity_updates": draft.continuity_updates.clone()
            }),
        )
        .await?;
        *audit = self
            .rule_first_audit_or_full_audit(chapter_number, write_result)
            .await?;
        Ok(true)
    }

    async fn read_reusable_unapproved_chapter(
        &self,
        chapter_number: usize,
    ) -> anyhow::Result<Option<novel_runner::DraftOutput>> {
        let Some(chapter) = project_chapter_record(&self.project_path, chapter_number)? else {
            return Ok(None);
        };
        if chapter_record_value_is_approved(&chapter) {
            return Ok(None);
        }
        let Some(path) = chapter.get("path").and_then(Value::as_str) else {
            return Ok(None);
        };
        let Ok(raw) = fs::read_to_string(Path::new(&self.project_path).join(path)) else {
            return Ok(None);
        };
        let body = strip_frontmatter(&raw);
        let Some(content) = Some(body.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        let title = chapter
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                if language_looks_cjk(&self.language) {
                    "未命名章节"
                } else {
                    "Untitled Chapter"
                }
            });
        let mut draft = novel_runner::DraftOutput {
            title: title.to_string(),
            content: sanitize_chapter_body(content, title, &self.language),
            summary: chapter
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            key_facts: json_string_array(chapter.get("key_facts")),
            continuity_updates: json_string_array(chapter.get("continuity_updates")),
            degraded: false,
            degraded_reason: String::new(),
        };
        repair_draft_summary_after_body_cleanup(&mut draft, &self.language);
        if !existing_unapproved_chapter_is_reusable(
            &draft,
            self.chapter_unit_target,
            &self.language,
        ) {
            return Ok(None);
        }
        Ok(Some(draft))
    }

    async fn repair_chapter_metadata_with_llm(
        &self,
        chapter_number: usize,
        draft: novel_runner::DraftOutput,
        write_result: &Value,
        rejected_titles: &[String],
    ) -> anyhow::Result<Option<(novel_runner::DraftOutput, Value)>> {
        record_workflow_checkpoint(
            &self.runtime,
            chapter_number as u32,
            "novel-chapter:metadata-repair:start",
            format!("第 {chapter_number} 章正文可用，正在只修复标题、摘要和连续性元数据。"),
        )
        .await;
        let prompt = metadata_repair_prompt(
            &self.language,
            chapter_number,
            &draft,
            &metadata_issue_summary(write_result),
            rejected_titles,
        );
        let raw = self
            .agent
            .generate_text_only_with_limits(
                &clean_provider_prompt(&prompt),
                metadata_repair_generation_limits(&self.language),
                self.progress_sink(chapter_number, "metadata"),
            )
            .await?;
        self.ensure_not_cancelled().await?;
        let cleaned_raw = clean_model_output(&raw);
        let repaired_metadata =
            parse_metadata_repair_output(&cleaned_raw, chapter_number, &self.language, &draft);
        let title_candidates = metadata_repair_title_candidates(
            &cleaned_raw,
            &repaired_metadata.title,
            rejected_titles,
        );
        let mut selected = None::<(usize, novel_runner::DraftOutput, Value)>;
        for title in title_candidates {
            let mut candidate_metadata = repaired_metadata.clone();
            candidate_metadata.title = title;
            let candidate_result = call_novel_studio_json(
                &self.tool,
                json!({
                    "action": "repair_chapter_metadata",
                    "candidate_only": true,
                    "project_path": self.project_path,
                    "chapter_number": chapter_number,
                    "chapter_title": candidate_metadata.title,
                    "summary": candidate_metadata.summary,
                    "key_facts": candidate_metadata.key_facts,
                    "continuity_updates": candidate_metadata.continuity_updates
                }),
            )
            .await?;
            let title_issue_count = metadata_title_issue_count(&candidate_result);
            let replace = selected
                .as_ref()
                .is_none_or(|(best_count, _, _)| title_issue_count < *best_count);
            if replace {
                selected = Some((title_issue_count, candidate_metadata, candidate_result));
            }
            if title_issue_count == 0 {
                break;
            }
        }
        let Some((_, repaired_metadata, repaired_write_result)) = selected else {
            return Ok(None);
        };
        let mut repaired_draft = repaired_metadata;
        if let Some(chapter) = repaired_write_result.get("chapter").or_else(|| {
            repaired_write_result
                .get("repaired_chapters")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
        }) {
            if let Some(title) = chapter.get("title").and_then(Value::as_str) {
                repaired_draft.title = title.trim().to_string();
            }
            if let Some(summary) = chapter.get("summary").and_then(Value::as_str) {
                repaired_draft.summary = summary.trim().to_string();
            }
            let facts = json_string_array(chapter.get("key_facts"));
            if !facts.is_empty() {
                repaired_draft.key_facts = facts;
            }
            let updates = json_string_array(chapter.get("continuity_updates"));
            if !updates.is_empty() {
                repaired_draft.continuity_updates = updates;
            }
        }
        Ok(Some((repaired_draft, repaired_write_result)))
    }

    async fn repair_metadata_gate_once(
        &self,
        chapter_number: usize,
        draft: &mut novel_runner::DraftOutput,
        write_result: &mut Value,
        audit: &mut Value,
        attempts: &mut usize,
        rejected_titles: &mut Vec<String>,
        max_attempts: usize,
    ) -> anyhow::Result<MetadataRepairFlow> {
        if !metadata_gate_needs_repair(write_result) {
            return Ok(MetadataRepairFlow::NotNeeded);
        }
        if !metadata_repair_allowed_with_audit(write_result, audit) {
            return Ok(MetadataRepairFlow::NotNeeded);
        }
        if !quality_gate_body_passed(write_result) {
            return Ok(MetadataRepairFlow::NotNeeded);
        }
        if *attempts >= max_attempts {
            self.apply_local_metadata_fallback(chapter_number, draft, write_result)
                .await?;
            return Ok(MetadataRepairFlow::FallbackApplied);
        }
        *attempts += 1;
        let current_title = draft.title.trim();
        if !current_title.is_empty()
            && !rejected_titles
                .iter()
                .any(|title| title.trim() == current_title)
        {
            rejected_titles.push(current_title.to_string());
        }
        let Some((repaired_draft, repaired_write_result)) = self
            .repair_chapter_metadata_with_llm(
                chapter_number,
                draft.clone(),
                write_result,
                rejected_titles,
            )
            .await?
        else {
            if *attempts < max_attempts {
                return Ok(MetadataRepairFlow::Retry);
            }
            self.apply_local_metadata_fallback(chapter_number, draft, write_result)
                .await?;
            return Ok(MetadataRepairFlow::FallbackApplied);
        };
        *draft = repaired_draft;
        *write_result = repaired_write_result;
        Ok(MetadataRepairFlow::Repaired)
    }

    async fn apply_local_metadata_fallback(
        &self,
        chapter_number: usize,
        draft: &mut novel_runner::DraftOutput,
        write_result: &mut Value,
    ) -> anyhow::Result<()> {
        let mut repaired = call_novel_studio_json(
            &self.tool,
            json!({
                "action": "repair_chapter_metadata",
                "project_path": self.project_path,
                "chapter_number": chapter_number
            }),
        )
        .await?;
        align_draft_with_studio_result(draft, &repaired);
        repaired["metadata_fallback_applied"] = json!(true);
        *write_result = repaired;
        Ok(())
    }

    async fn approve_chapter_after_validation(
        &self,
        chapter_number: usize,
        _draft: &mut novel_runner::DraftOutput,
        _write_result: &mut Value,
        _audit: &mut Value,
    ) -> anyhow::Result<Value> {
        call_novel_studio_json_raw(
            &self.tool,
            json!({
                "action": "approve_chapter",
                "project_path": self.project_path,
                "chapter_number": chapter_number
            }),
        )
        .await
    }

    async fn chapter_already_approved(&self, chapter_number: usize) -> anyhow::Result<bool> {
        let Some(chapter) = project_chapter_record(&self.project_path, chapter_number)? else {
            return Ok(false);
        };
        if !chapter_record_value_is_approved(&chapter) {
            return Ok(false);
        }
        Ok(true)
    }

    async fn ensure_not_cancelled(&self) -> anyhow::Result<()> {
        let (Some(task_manager), Some(task_id)) =
            (self.runtime.task_manager.as_ref(), self.runtime.task_id)
        else {
            return Ok(());
        };
        loop {
            if let Some(task) = task_manager.load(&task_id.to_string()).await? {
                match task.status {
                    benshu_state::TaskStatus::Cancelled => {
                        anyhow::bail!("novel workflow task was cancelled");
                    }
                    benshu_state::TaskStatus::Paused(_) => {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }
                    _ => {}
                }
            }
            return Ok(());
        }
    }

    async fn expand_short_chapter_if_needed(
        &self,
        chapter_number: usize,
        draft: novel_runner::DraftOutput,
        phase: &'static str,
    ) -> anyhow::Result<novel_runner::DraftOutput> {
        let Some(target) = self.chapter_unit_target.filter(|value| *value > 0) else {
            return Ok(draft);
        };
        let current_units = count_chapter_units(&draft.content, &self.language);
        let completion_units = required_chapter_units(target);
        if current_units >= completion_units {
            return Ok(draft);
        }

        let mut expanded = draft.clone();
        let desired_rounds = chapter_expansion_round_budget(target, current_units);
        let mut accepted_rounds = 0usize;
        let mut rejected_attempts = 0usize;
        // Length is a deterministic contract. Permit a finite set of distinct
        // top-up attempts before the quality gate blocks the chapter.
        const MAX_EXPANSION_ATTEMPTS: usize = 5;
        let max_attempts = MAX_EXPANSION_ATTEMPTS;
        let mut attempts = 0usize;
        let mut previous_rejection: Option<String> = None;
        let authority_context = self
            .authoritative_chapter_context_json(chapter_number)
            .await;
        while accepted_rounds < desired_rounds && attempts < max_attempts {
            attempts += 1;
            self.ensure_not_cancelled().await?;
            let units = count_chapter_units(&expanded.content, &self.language);
            if units >= completion_units {
                break;
            }
            let remaining = target
                .saturating_sub(units)
                .max(completion_units.saturating_sub(units));
            let segment_target = chapter_expansion_segment_target(target, remaining);
            let round = accepted_rounds + 1;
            let expand_prompt = chapter_expansion_prompt(
                chapter_number,
                &expanded.title,
                &self.language,
                target,
                completion_units,
                units,
                segment_target,
                attempts,
                previous_rejection.as_deref(),
                &expanded.summary,
                &expanded.content,
                &authority_context,
            );
            let expand_prompt = clean_provider_prompt(&expand_prompt);
            let expanded_raw = self
                .agent
                .generate_text_only_with_limits(
                    &expand_prompt,
                    chapter_segment_generation_limits(segment_target, &self.language),
                    self.progress_sink(chapter_number, &format!("{phase}-{round}")),
                )
                .await?;
            self.ensure_not_cancelled().await?;
            let expanded_raw = clean_model_output(&expanded_raw);
            if let Some(reason) =
                raw_chapter_expansion_rejection_reason(&expanded_raw, &self.language)
            {
                record_workflow_checkpoint(
                    &self.runtime,
                    chapter_number as u32,
                    "novel-chapter:expansion-rejected",
                    format!(
                        "第 {chapter_number} 章扩写原始片段未追加，原因：{reason}；继续尝试新的续写片段。"
                    ),
                )
                .await;
                rejected_attempts += 1;
                previous_rejection = Some(reason);
                if rejected_attempts >= MAX_EXPANSION_ATTEMPTS {
                    self.record_expansion_blocked(chapter_number, rejected_attempts, "被拒绝")
                        .await;
                    break;
                }
                continue;
            }
            let before_units = count_chapter_units(&expanded.content, &self.language);
            let addition = parse_chapter_expansion_output(&expanded_raw, &self.language);
            let addition =
                trim_overlapping_chapter_expansion(&expanded.content, addition, &self.language);
            if let Some(reason) = chapter_expansion_rejection_reason(
                &expanded.content,
                &addition.addition,
                &self.language,
            ) {
                record_workflow_checkpoint(
                    &self.runtime,
                    chapter_number as u32,
                    "novel-chapter:expansion-rejected",
                    format!(
                        "第 {chapter_number} 章扩写片段未追加，原因：{reason}；继续尝试新的续写片段。"
                    ),
                )
                .await;
                rejected_attempts += 1;
                previous_rejection = Some(reason);
                if rejected_attempts >= MAX_EXPANSION_ATTEMPTS {
                    self.record_expansion_blocked(chapter_number, rejected_attempts, "被拒绝")
                        .await;
                    break;
                }
                continue;
            }
            let addition_units = count_chapter_units(&addition.addition, &self.language);
            if addition_units < chapter_minimum_addition_units(segment_target) {
                rejected_attempts += 1;
                previous_rejection = Some(format!(
                    "扩写片段过短：仅 {addition_units} 字，未达到本次最少追加量"
                ));
                if rejected_attempts >= MAX_EXPANSION_ATTEMPTS {
                    self.record_expansion_blocked(chapter_number, rejected_attempts, "过短")
                        .await;
                    break;
                }
                continue;
            }
            append_chapter_addition(&mut expanded, addition);
            let after_units = count_chapter_units(&expanded.content, &self.language);
            if after_units <= before_units {
                rejected_attempts += 1;
                previous_rejection = Some("扩写片段没有增加正文".to_string());
                if rejected_attempts >= MAX_EXPANSION_ATTEMPTS {
                    self.record_expansion_blocked(
                        chapter_number,
                        rejected_attempts,
                        "没有增加正文",
                    )
                    .await;
                    break;
                }
                continue;
            }
            rejected_attempts = 0;
            accepted_rounds += 1;
        }
        Ok(expanded)
    }

    async fn complete_unfinished_chapter_tail_if_needed(
        &self,
        chapter_number: usize,
        draft: novel_runner::DraftOutput,
        phase: &'static str,
    ) -> anyhow::Result<novel_runner::DraftOutput> {
        let mut issues = chapter_body_completion_issue_list(&draft.content);
        if issues.is_empty() {
            return Ok(draft);
        }

        let mut completed = draft;
        let authority_context = self
            .authoritative_chapter_context_json(chapter_number)
            .await;
        let segment_target = if language_looks_cjk(&self.language) {
            600
        } else {
            240
        };
        for attempt in 1..=1 {
            self.ensure_not_cancelled().await?;
            let prompt = chapter_tail_completion_prompt(
                chapter_number,
                &completed.title,
                &self.language,
                segment_target,
                &completed.summary,
                &completed.content,
                &issues,
                &authority_context,
            );
            let raw = self
                .agent
                .generate_text_only_with_limits(
                    &clean_provider_prompt(&prompt),
                    chapter_segment_generation_limits(segment_target, &self.language),
                    self.progress_sink(chapter_number, &format!("{phase}-{attempt}")),
                )
                .await?;
            self.ensure_not_cancelled().await?;
            let raw = clean_model_output(&raw);
            if let Some(reason) = raw_chapter_expansion_rejection_reason(&raw, &self.language) {
                record_workflow_checkpoint(
                    &self.runtime,
                    chapter_number as u32,
                    "novel-chapter:tail-completion-rejected",
                    format!("第 {chapter_number} 章补尾原始片段未追加，原因：{reason}；继续尝试。"),
                )
                .await;
                continue;
            }
            let addition = parse_chapter_expansion_output(&raw, &self.language);
            let addition = trim_overlapping_chapter_tail_completion(
                &completed.content,
                addition,
                &self.language,
            );
            if let Some(reason) = chapter_tail_completion_rejection_reason(
                &completed.content,
                &addition.addition,
                &self.language,
            ) {
                record_workflow_checkpoint(
                    &self.runtime,
                    chapter_number as u32,
                    "novel-chapter:tail-completion-rejected",
                    format!("第 {chapter_number} 章补尾片段未追加，原因：{reason}；继续尝试。"),
                )
                .await;
                continue;
            }
            let before = text_fingerprint(&completed.content);
            append_chapter_tail_completion(&mut completed, addition);
            completed.content =
                sanitize_chapter_body(&completed.content, &completed.title, &self.language);
            if text_fingerprint(&completed.content) == before {
                continue;
            }
            repair_draft_summary_after_body_cleanup(&mut completed, &self.language);
            issues = chapter_body_completion_issue_list(&completed.content);
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:tail-completion",
                if issues.is_empty() {
                    format!("第 {chapter_number} 章检测到尾句截断，已追加补尾并恢复完整结尾。")
                } else {
                    format!(
                        "第 {chapter_number} 章已追加补尾，但仍存在未完成尾句问题：{}",
                        issues.join("; ")
                    )
                },
            )
            .await;
            if issues.is_empty() {
                break;
            }
        }
        Ok(completed)
    }

    async fn record_expansion_blocked(
        &self,
        chapter_number: usize,
        rejected_attempts: usize,
        reason: &str,
    ) {
        record_workflow_checkpoint(
            &self.runtime,
            chapter_number as u32,
            "novel-chapter:expansion-blocked",
            format!(
                "第 {chapter_number} 章扩写连续 {rejected_attempts} 次{reason}，停止本轮扩写并交给质量门处理。"
            ),
        )
        .await;
    }

    async fn generate_chapter_execution_package(
        &self,
        chapter_number: usize,
        title: &str,
        context_json: &str,
        previous_error: Option<&str>,
    ) -> anyhow::Result<novel_runner::ChapterExecutionPackage> {
        if !chapter_execution_package_llm_enabled() {
            record_workflow_checkpoint(
                &self.runtime,
                chapter_number as u32,
                "novel-chapter:execution-package:fallback",
                format!(
                    "第 {chapter_number} 章使用确定性章节执行包，避免中间规划调用阻塞正文生成。"
                ),
            )
            .await;
            return Ok(fallback_chapter_execution_package(
                &self.language,
                title,
                chapter_number,
                context_json,
                self.force_generation_after_target,
                self.completion_gate.as_ref(),
            ));
        }
        let mut feedback = previous_error.map(ToString::to_string);
        let mut last_error = None;
        for attempt in 1..=2 {
            self.ensure_not_cancelled().await?;
            let prompt = novel_runner::chapter_execution_prompt(
                &self.language,
                title,
                chapter_number,
                context_json,
                feedback.as_deref(),
            );
            let prompt = if let Some(gate) = self.completion_gate.as_ref() {
                append_finale_instruction(&prompt, gate, &self.language)
            } else {
                prompt
            };
            let prompt = clean_provider_prompt(&prompt);
            let output = match novel_runner::generate_execution_package(
                &self.agent,
                &prompt,
                Some(3200),
                &self.language,
            )
            .await
            {
                Ok(output) => output,
                Err(error) => {
                    let error = error.to_string();
                    last_error = Some(error.clone());
                    if attempt < 2 {
                        feedback = Some(error);
                        continue;
                    }
                    break;
                }
            };
            self.ensure_not_cancelled().await?;
            record_runner_parse_provenance(
                &self.runtime,
                chapter_number,
                "execution-package",
                output.provenance,
            )
            .await;
            return Ok(govern_generated_execution_package(
                output.value,
                &self.language,
                title,
                chapter_number,
                context_json,
                self.force_generation_after_target,
                self.completion_gate.as_ref(),
            ));
        }
        record_workflow_checkpoint(
            &self.runtime,
            chapter_number as u32,
            "novel-chapter:execution-package:fallback",
            format!(
                "第 {chapter_number} 章执行包未能稳定生成，已使用合同和上下文生成通用 fallback。原因：{}",
                last_error.as_deref().unwrap_or("unknown error")
            ),
        )
        .await;
        let package = fallback_chapter_execution_package(
            &self.language,
            title,
            chapter_number,
            context_json,
            self.force_generation_after_target,
            self.completion_gate.as_ref(),
        );
        Ok(package)
    }
}

fn approval_result_is_approved(value: &Value) -> bool {
    value
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && value
            .pointer("/chapter/status")
            .and_then(Value::as_str)
            .is_some_and(chapter_lifecycle::status_is_approved)
}

fn draft_output_fallback_body_is_usable(
    draft: &novel_runner::DraftOutput,
    chapter_unit_target: Option<usize>,
    language: &str,
) -> bool {
    let content = draft.content.trim();
    if content.is_empty() {
        return false;
    }
    let units = count_chapter_units(content, language);
    if units == 0 {
        return false;
    }
    if !chapter_body_completion_issue_list(content).is_empty() {
        return false;
    }
    if chapter_body_has_tool_or_json_residue(content) {
        return false;
    }
    let Some(required) = chapter_unit_target.map(required_chapter_units) else {
        return units >= 400;
    };
    let minimum = minimum_chapter_units(required).max(400);
    units >= minimum
}

pub(super) fn chapter_body_has_tool_or_json_residue(content: &str) -> bool {
    let lowered = content.to_ascii_lowercase();
    lowered.contains("executed_tool:")
        || lowered.contains("runtime_effect")
        || lowered.contains("tool_call")
        || lowered.contains("\"content\"")
        || lowered.contains("\"title\"")
        || lowered.contains("```json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeform_draft_with_repairable_tail_is_not_fallback_usable() {
        let body = format!(
            "{}他推开会议室的门，所有人的视线都压了过来。方案摊开在桌面上，他没有急着解释，而是先把客户真正的痛点写在白板中央。唐总沉默，竞争者冷笑，只有他知道这一步必须先把局面拖进自己的节奏里。最后一页翻开时，他听见窗外雨声变轻，也听见对手的呼吸乱了一拍。他知道自己已经",
            "景岑棠".repeat(260)
        );
        let draft = novel_runner::DraftOutput {
            title: "旋转门前".to_string(),
            summary: String::new(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            content: body,
            degraded: true,
            degraded_reason:
                "model output was not valid DraftOutput JSON; parsed freeform fallback".to_string(),
        };

        assert!(
            !chapter_body_completion_issue_list(&draft.content).is_empty(),
            "fixture should still need the existing tail-completion stage"
        );
        assert!(!draft_output_fallback_body_is_usable(
            &draft,
            Some(2500),
            "zh-CN"
        ));
    }

    #[test]
    fn sealed_future_boundary_uses_completed_event_when_observer_omits_it() {
        let current = "陆昭岚完成第一笔交易";
        let next = "梁砚桥察觉陆家庄园异常繁荣，竞争压力开始显现";
        let body = "梁砚桥已经察觉陆家庄园异常繁荣，竞争压力开始显现。";

        let (excerpt, source) =
            sealed_future_boundary_evidence(body, "", current, next, true, &[]).expect("evidence");

        assert_eq!(excerpt, body);
        assert_eq!(
            source,
            "final_body_completed_event+sealed_next_chapter_boundary"
        );
    }
}
