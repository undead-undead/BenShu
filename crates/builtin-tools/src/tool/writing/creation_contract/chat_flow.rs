use super::*;

#[async_trait]
pub trait CreationDraftRuntime {
    async fn load_draft(
        &mut self,
        session_id: &str,
    ) -> anyhow::Result<Option<SessionCreationDraftState>>;
    async fn save_draft(&mut self, draft: &SessionCreationDraftState) -> anyhow::Result<()>;
    async fn clear_draft(&mut self, session_id: &str) -> anyhow::Result<()>;
    async fn create_draft(&mut self, draft: &mut SessionCreationDraftState) -> anyhow::Result<()>;
    async fn update_draft(&mut self, draft: &SessionCreationDraftState) -> anyhow::Result<()>;
    async fn approve_draft(&mut self, draft: &SessionCreationDraftState) -> anyhow::Result<Value>;
    async fn approved_draft_for_existing_project(
        &mut self,
        session_id: &str,
        draft: &mut SessionCreationDraftState,
    ) -> anyhow::Result<Value>;
    async fn discard_draft(&mut self, draft: &SessionCreationDraftState) -> anyhow::Result<()>;
    async fn existing_project_path(
        &mut self,
        session_id: &str,
        draft: &SessionCreationDraftState,
    ) -> anyhow::Result<Option<String>>;
    async fn existing_project_path_for_continuation_message(
        &mut self,
        session_id: &str,
        message: &str,
    ) -> anyhow::Result<Option<String>>;
    async fn existing_project_artifact_kind(
        &mut self,
        project_path: &str,
    ) -> anyhow::Result<String>;
}

fn sync_and_validate_approved_contract(
    draft: &mut SessionCreationDraftState,
    approved: &Value,
) -> Result<(), ContractValidationReport> {
    if approved.get("draft").is_none() {
        return Err(ContractValidationReport {
            artifact_kind: draft.artifact_kind.clone(),
            issues: vec!["项目没有返回可恢复的权威写作合同，系统不能安全地继续写作".to_string()],
        });
    }

    let mut synchronized = draft.clone();
    let execution_scope_note = synchronized
        .planning_notes
        .iter()
        .find(|note| note.starts_with(CREATION_EXECUTION_SCOPE_NOTE_PREFIX))
        .cloned();
    synchronized.title.clear();
    synchronized.language.clear();
    synchronized.genre.clear();
    synchronized.brief.clear();
    synchronized.target_units = None;
    synchronized.chapter_unit_target = None;
    synchronized.max_chapters_per_turn = None;
    clear_fiction_contract_fields(&mut synchronized);
    if let Some(project_path) = project_path_from_approved_creation_draft(approved) {
        synchronized.project_path = project_path;
    }
    sync_creation_draft_from_approval(&mut synchronized, approved);
    if let Some(execution_scope_note) = execution_scope_note {
        synchronized
            .planning_notes
            .retain(|note| !note.starts_with(CREATION_EXECUTION_SCOPE_NOTE_PREFIX));
        synchronized.planning_notes.push(execution_scope_note);
    }
    let report = ContractValidationReport::for_draft_scope(
        &synchronized,
        ContractReadinessScope::LockedAuthorityContract,
    );
    *draft = synchronized;
    if report.is_ready() {
        Ok(())
    } else {
        Err(report)
    }
}

pub fn creation_draft_metadata_key(session_id: &str) -> String {
    format!("writing.creation_draft.{session_id}")
}

pub async fn infer_project_artifact_kind(project_path: &str) -> anyhow::Result<String> {
    let normalized = project_path.replace('\\', "/");
    if normalized.contains("/novels/") {
        return Ok("fiction".to_string());
    }

    let manifest_path = std::path::Path::new(project_path).join("project.json");
    let Ok(raw) = tokio::fs::read_to_string(manifest_path).await else {
        return Ok("fiction".to_string());
    };
    let Ok(manifest) = serde_json::from_str::<Value>(&raw) else {
        return Ok("fiction".to_string());
    };
    if manifest.get("story_bible").is_some()
        || manifest
            .get("chapters")
            .and_then(Value::as_array)
            .is_some_and(|chapters| !chapters.is_empty())
    {
        return Ok("fiction".to_string());
    }
    Ok(manifest
        .get("document_type")
        .and_then(Value::as_str)
        .filter(|kind| matches!(*kind, "paper" | "report"))
        .unwrap_or("fiction")
        .to_string())
}

pub fn intent_requests_existing_work_continuation(intent: &str) -> bool {
    let lowered = intent.to_ascii_lowercase();
    if creation_draft_message_requests_continuation_generation(intent, &lowered) {
        return true;
    }
    if creation_draft_content_operation(intent, "fiction")
        .is_some_and(|operation| !matches!(operation, NovelContentOperation::Read))
    {
        return true;
    }
    if intent_requests_existing_work_read_only_status(intent, &lowered) {
        return false;
    }
    let continuation_surface = [
        "继续",
        "续写",
        "接着",
        "沿用",
        "承接",
        "下一章",
        "上一章",
        "前一章",
        "刚才",
        "刚刚",
        "上次",
        "上一轮",
        "之前",
        "前面",
        "已生成",
        "生成的",
        "这个项目",
        "这个文档",
        "这个文件",
        "这个小说",
        "current",
        "previous",
        "last",
        "existing",
        "continue",
        "append",
        "same project",
        "same document",
        "next chapter",
    ];
    continuation_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term))
}

pub(crate) fn intent_requests_existing_work_generation(intent: &str) -> bool {
    creation_draft_message_requests_continuation_generation(intent, &intent.to_ascii_lowercase())
}

pub(crate) fn intent_requests_existing_work_read_only_status(intent: &str, lowered: &str) -> bool {
    let read_or_status_surface = [
        "检查",
        "查看",
        "看一下",
        "读取",
        "总结",
        "概括",
        "说明",
        "告诉我",
        "是否完成",
        "完成了吗",
        "完成了没",
        "完成没",
        "写好了吗",
        "写好了没",
        "写好没",
        "已完成",
        "完成到",
        "第几章",
        "进度",
        "状态",
        "总字数",
        "章节数",
        "最后一章",
        "导出路径",
        "路径",
        "inspect",
        "status",
        "progress",
        "done",
        "complete",
        "summary",
        "path",
    ];
    let existing_surface = [
        "当前",
        "这本",
        "这个",
        "刚才",
        "刚刚",
        "上次",
        "上一轮",
        "之前",
        "前面",
        "已经",
        "已完成",
        "完成到",
        "已生成",
        "生成的",
        "保存的",
        "导出的",
        "current",
        "previous",
        "last",
        "already",
        "existing",
        "generated",
        "saved",
        "exported",
    ];
    let asks_read_or_status = read_or_status_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term));
    let references_existing = existing_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term));
    let references_existing_segment = !referenced_artifact_segment_numbers(intent).is_empty();
    asks_read_or_status && (references_existing || references_existing_segment)
}

pub fn creation_intake_response(message: &str) -> Option<CreationDraftUserResponse> {
    let lowered = message.to_ascii_lowercase();
    if intent_requests_existing_work_read_only_status(message, &lowered)
        || intent_requests_read_only_existing_artifact_answer(message)
    {
        return None;
    }
    let decision = evaluate_creation_intake(message);
    if !decision.should_clarify() {
        return None;
    }
    Some(CreationDraftUserResponse::new(
        decision.prompt?,
        decision
            .artifact_kind
            .unwrap_or_else(|| "artifact".to_string()),
    ))
}

pub async fn handle_creation_draft_chat<R>(
    runtime: &mut R,
    session_id: &str,
    message: &str,
) -> anyhow::Result<Option<CreationDraftTurnOutcome>>
where
    R: CreationDraftRuntime + Send,
{
    if let Some(mut draft) = runtime.load_draft(session_id).await? {
        let existing_work_continuation_for_status =
            intent_requests_existing_work_continuation(message);
        let turn_intent = classify_creation_draft_turn_intent_with_context(
            message,
            true,
            Some(draft.lifecycle_status()),
            (!draft.project_path.trim().is_empty()).then_some(draft.project_path.as_str()),
            None,
        );
        if matches!(turn_intent, CreationDraftTurnIntent::Discard) {
            runtime.discard_draft(&draft).await?;
            runtime.clear_draft(session_id).await?;
            return Ok(Some(CreationDraftTurnOutcome::Respond(
                CreationDraftUserResponse::new(
                    "已取消当前创作草案。你可以重新描述新的写作需求。",
                    draft.artifact_kind,
                ),
            )));
        }
        if matches!(turn_intent, CreationDraftTurnIntent::ReadStatus)
            && !existing_work_continuation_for_status
        {
            return Ok(Some(CreationDraftTurnOutcome::Respond(
                creation_draft_planning_response(&draft, message),
            )));
        }
        if benshu_runtime_policy_core::creation_request_needs_adult_age_confirmation(message) {
            return Ok(Some(CreationDraftTurnOutcome::Respond(
                creation_intake_response(message).unwrap_or_else(|| {
                    CreationDraftUserResponse::new(
                        "这类创作可能包含成人向、强烈暴力或血腥内容。开始生成合同或正文前，请先确认你已年满十八周岁。",
                        draft.artifact_kind.clone(),
                    )
                }),
            )));
        }

        let approval = matches!(turn_intent, CreationDraftTurnIntent::ApproveAndStart);
        let execution = creation_draft_execution_requested_for_intent(
            message,
            &draft.artifact_kind,
            turn_intent,
        );
        let modification = creation_draft_modification_requested(message);
        let generated_title_revision = creation_draft_requests_generated_title_revision(message);
        let framework_request = creation_draft_framework_requested(message, &draft.artifact_kind);
        let lowered = message.to_ascii_lowercase();
        let continuation_generation =
            creation_draft_message_requests_continuation_generation(message, &lowered);
        let existing_work_continuation = existing_work_continuation_for_status;
        let content_operation = if existing_work_continuation
            && !message_has_explicit_content_operation_target(message, &lowered)
        {
            None
        } else if let Some(operation) =
            creation_draft_content_operation(message, &draft.artifact_kind)
        {
            if draft.is_approved() {
                Some(operation)
            } else if let Some(project_path) =
                runtime.existing_project_path(session_id, &draft).await?
            {
                draft.project_path = project_path;
                Some(operation)
            } else {
                None
            }
        } else {
            None
        };
        let view_only = !approval && !execution && creation_draft_view_only_requested(message);
        if view_only {
            return Ok(Some(CreationDraftTurnOutcome::Respond(
                creation_draft_planning_response(&draft, message),
            )));
        }
        if draft.is_approved()
            && !approval
            && !execution
            && !modification
            && content_operation.is_none()
            && !continuation_generation
        {
            return Ok(None);
        }
        if !view_only && content_operation.is_none() && (modification || (!approval && !execution))
        {
            apply_message_to_creation_draft(&mut draft, message);
            runtime.update_draft(&draft).await?;
            draft.updated_at = chrono::Utc::now().to_rfc3339();
        }

        if generated_title_revision
            && !existing_work_continuation
            && (approval || execution)
            && draft.artifact_kind == "fiction"
        {
            runtime.save_draft(&draft).await?;
            let prompt = final_prompt_from_creation_framework_request(&draft, message);
            return Ok(Some(CreationDraftTurnOutcome::ContinueWithMessage(prompt)));
        }

        if let Some(operation) = content_operation {
            let approved = runtime
                .approved_draft_for_existing_project(session_id, &mut draft)
                .await?;
            if !creation_draft_approval_succeeded(&approved)
                || project_path_from_approved_creation_draft(&approved).is_none()
            {
                runtime.save_draft(&draft).await?;
                return Ok(Some(CreationDraftTurnOutcome::Respond(
                    CreationDraftUserResponse::new(
                        "当前小说项目尚未完成创建，所以我没有执行正文增删查改。请先确认并完成创作合同。",
                        draft.artifact_kind,
                    ),
                )));
            }
            if let Err(report) = sync_and_validate_approved_contract(&mut draft, &approved) {
                draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
                runtime.save_draft(&draft).await?;
                return Ok(Some(CreationDraftTurnOutcome::Respond(
                    report.user_response(&draft, message),
                )));
            }
            draft.set_lifecycle_status(CreationDraftLifecycleStatus::Approved);
            runtime.save_draft(&draft).await?;
            let prompt =
                final_prompt_from_novel_content_operation(&draft, &approved, message, operation);
            return Ok(Some(CreationDraftTurnOutcome::ContinueWithMessage(prompt)));
        }

        if (continuation_generation || existing_work_continuation)
            && runtime
                .existing_project_path(session_id, &draft)
                .await?
                .is_some()
        {
            let approved = runtime
                .approved_draft_for_existing_project(session_id, &mut draft)
                .await?;
            if creation_draft_approval_succeeded(&approved)
                && project_path_from_approved_creation_draft(&approved).is_some()
            {
                if let Err(report) = sync_and_validate_approved_contract(&mut draft, &approved) {
                    draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
                    runtime.save_draft(&draft).await?;
                    return Ok(Some(CreationDraftTurnOutcome::Respond(
                        report.user_response(&draft, message),
                    )));
                }
                sanitize_creation_draft_control_noise(&mut draft);
                draft.set_lifecycle_status(CreationDraftLifecycleStatus::Approved);
                runtime.save_draft(&draft).await?;
                let prompt = final_prompt_from_approved_creation_draft(&draft, &approved, message);
                return Ok(Some(CreationDraftTurnOutcome::ContinueWithMessage(prompt)));
            }
        }

        if approval || execution {
            if !draft.is_approved() {
                sanitize_creation_draft_control_noise(&mut draft);
                let report = ContractValidationReport::for_draft_scope(
                    &draft,
                    ContractReadinessScope::LockedAuthorityContract,
                );
                if !report.is_ready() {
                    draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
                    draft.diagnostics = merge_list(
                        &draft.diagnostics,
                        &[format!(
                            "用户已确认开始，但合同仍缺少系统必需字段，系统已阻止正文写作：{}",
                            report.issues.join("；")
                        )],
                    );
                    runtime.save_draft(&draft).await?;
                    return Ok(Some(CreationDraftTurnOutcome::Respond(
                        report.user_response(&draft, message),
                    )));
                }
                draft.set_lifecycle_status(CreationDraftLifecycleStatus::ContractReady);
            }
            let approved = if draft.is_approved() {
                runtime
                    .approved_draft_for_existing_project(session_id, &mut draft)
                    .await?
            } else {
                runtime.approve_draft(&draft).await?
            };
            if creation_draft_approval_title_conflicted(&approved)
                && draft.artifact_kind == "fiction"
                && !draft.is_approved()
            {
                if let Some(title) = approved
                    .get("title")
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    draft.diagnostics = merge_list(
                        &draft.diagnostics,
                        &[format!(
                            "标题《{title}》已存在，请根据当前合同重新生成不同书名。"
                        )],
                    );
                }
                draft.title.clear();
                runtime.save_draft(&draft).await?;
                let prompt = final_prompt_from_creation_framework_request(&draft, message);
                return Ok(Some(CreationDraftTurnOutcome::ContinueWithMessage(prompt)));
            }
            if !creation_draft_approval_succeeded(&approved)
                || project_path_from_approved_creation_draft(&approved).is_none()
            {
                runtime.save_draft(&draft).await?;
                return Ok(Some(CreationDraftTurnOutcome::Respond(
                    CreationDraftUserResponse::new(
                        creation_draft_approval_failure_response(&approved),
                        draft.artifact_kind,
                    ),
                )));
            }
            if let Err(report) = sync_and_validate_approved_contract(&mut draft, &approved) {
                draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
                draft.diagnostics = merge_list(
                    &draft.diagnostics,
                    &[format!(
                        "项目初始化返回的合同没有通过权威合同校验，系统已阻止正文写作：{}",
                        report.issues.join("；")
                    )],
                );
                runtime.save_draft(&draft).await?;
                return Ok(Some(CreationDraftTurnOutcome::Respond(
                    report.user_response(&draft, message),
                )));
            }
            draft.set_lifecycle_status(CreationDraftLifecycleStatus::Approved);
            runtime.save_draft(&draft).await?;
            let prompt = final_prompt_from_approved_creation_draft(&draft, &approved, message);
            return Ok(Some(CreationDraftTurnOutcome::ContinueWithMessage(prompt)));
        }

        runtime.save_draft(&draft).await?;
        if framework_request {
            let repair_issues = creation_draft_pending_quality_repair_issues(&draft);
            let prompt = if repair_issues.is_empty() {
                final_prompt_from_creation_framework_request(&draft, message)
            } else {
                final_prompt_from_contract_quality_repair(&draft, message, &repair_issues)
            };
            return Ok(Some(CreationDraftTurnOutcome::ContinueWithMessage(prompt)));
        }
        return Ok(Some(CreationDraftTurnOutcome::Respond(
            creation_draft_planning_response(&draft, message),
        )));
    }

    if intent_requests_existing_work_continuation(message) {
        if let Some(project_path) = runtime
            .existing_project_path_for_continuation_message(session_id, message)
            .await?
        {
            let artifact_kind = runtime
                .existing_project_artifact_kind(&project_path)
                .await
                .unwrap_or_else(|_| "fiction".to_string());
            if matches!(artifact_kind.as_str(), "fiction" | "paper" | "report") {
                if let Some(mut draft) =
                    build_initial_creation_draft(session_id, &artifact_kind, message)
                {
                    draft.project_path = project_path.clone();
                    let approved = runtime
                        .approved_draft_for_existing_project(session_id, &mut draft)
                        .await?;
                    if !creation_draft_approval_succeeded(&approved)
                        || project_path_from_approved_creation_draft(&approved).is_none()
                        || approved.get("draft").is_none()
                    {
                        return Ok(Some(CreationDraftTurnOutcome::Respond(
                            CreationDraftUserResponse::new(
                                "找到了已有写作项目，但没有恢复出可验证的权威合同；为了避免角色、书名和结局漂移，本轮没有直接续写。请先检查项目状态或重新加载该项目。",
                                artifact_kind,
                            ),
                        )));
                    }
                    if let Err(report) = sync_and_validate_approved_contract(&mut draft, &approved)
                    {
                        runtime.save_draft(&draft).await?;
                        return Ok(Some(CreationDraftTurnOutcome::Respond(
                            report.user_response(&draft, message),
                        )));
                    }
                    draft.set_lifecycle_status(CreationDraftLifecycleStatus::Approved);
                    runtime.save_draft(&draft).await?;
                    let prompt =
                        final_prompt_from_approved_creation_draft(&draft, &approved, message);
                    return Ok(Some(CreationDraftTurnOutcome::ContinueWithMessage(prompt)));
                }
            }
        }
    }

    let decision = evaluate_creation_intake(message);
    if decision.should_clarify() {
        return Ok(creation_intake_response(message).map(CreationDraftTurnOutcome::Respond));
    }
    let kind = decision
        .artifact_kind
        .clone()
        .or_else(|| detect_creation_artifact_kind(message));
    if kind.is_none() && !decision.should_clarify() {
        return Ok(None);
    }

    let Some(kind) = kind else {
        return Ok(None);
    };
    let Some(mut draft) = build_initial_creation_draft(session_id, &kind, message) else {
        return Ok(creation_intake_response(message).map(CreationDraftTurnOutcome::Respond));
    };
    runtime.create_draft(&mut draft).await?;
    runtime.save_draft(&draft).await?;

    if kind == "fiction" {
        // Fresh fiction requests always enter the visible contract flow first.
        // Even "write directly" wording means "auto-complete the contract",
        // not "skip confirmation and start prose".
        let prompt = final_prompt_from_creation_framework_request(&draft, message);
        return Ok(Some(CreationDraftTurnOutcome::ContinueWithMessage(prompt)));
    }

    Ok(Some(CreationDraftTurnOutcome::Respond(
        creation_draft_planning_response(&draft, message),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_project_authority_cannot_borrow_missing_fields_from_previous_draft() {
        let mut draft = build_initial_creation_draft(
            "authority-reset",
            "fiction",
            "写都市玄幻小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        draft.title = "旧书名".to_string();
        draft.fiction_premise = "旧故事前提".to_string();
        draft.current_contract = Some(serde_json::json!({
            "title": {"canonical_title": "旧书名"},
            "premise": "旧故事前提"
        }));

        let approved = serde_json::json!({
            "success": true,
            "project_path": "data/generated/novels/project",
            "draft": {}
        });
        let report = sync_and_validate_approved_contract(&mut draft, &approved)
            .expect_err("incomplete project authority must remain blocked");

        assert!(draft.current_contract.is_none());
        assert!(draft.title.is_empty());
        assert!(draft.fiction_premise.is_empty());
        assert!(!report.is_ready());
    }

    #[test]
    fn approved_project_sync_preserves_initial_execution_scope_authority() {
        let mut draft = build_initial_creation_draft(
            "authority-scope",
            "fiction",
            "写一本10万字小说，每章2500字，每次只写一章，确认后自动连续写完整本。",
        )
        .expect("draft");
        let approved = serde_json::json!({
            "success": true,
            "project_path": "data/generated/novels/project",
            "draft": {}
        });

        let _ = sync_and_validate_approved_contract(&mut draft, &approved);

        assert_eq!(
            persisted_creation_execution_scope(&draft.planning_notes),
            Some(CreationDraftTurnScope::AllRemaining)
        );
    }
}
