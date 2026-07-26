use crate::agent::history::QueryHistory;
use crate::agent::message::Message;
use crate::agent::protocol::{AgentEventData, AgentLiaison, ChatOutcome, TokenUsage, ToolCallData};
use crate::agent::provider::Provider;
use crate::error::Result;
use tracing::info;

use super::{reasoner_constants, Reasoner};

pub(super) enum StepDisposition {
    NotApplicable,
    ContinueLoop,
    Finalized(ChatOutcome),
}

impl<P: Provider> Reasoner<P> {
    pub(super) async fn try_pre_llm_local_file_continuation(
        &self,
        bridge: &dyn AgentLiaison,
        messages: &mut Vec<Message>,
        steps: usize,
        history: &mut QueryHistory,
        tool_trace: &mut Vec<ToolCallData>,
    ) -> Result<StepDisposition> {
        let Some(query) = Self::latest_user_query(messages) else {
            return Ok(StepDisposition::NotApplicable);
        };
        if !Self::query_requests_local_file_continuation(&query)
            || Self::has_system_marker_after_latest_user(
                messages,
                "BENSHU_ORCHESTRATION_LOCAL_FILE_CONTINUATION",
            )
        {
            return Ok(StepDisposition::NotApplicable);
        }
        if !self.tool_is_enabled("delegate") {
            return Ok(StepDisposition::NotApplicable);
        }

        bridge.emit(AgentEventData::Thought {
            content: Self::user_facing_progress_message("file_artifact", &query),
        });
        bridge.emit(AgentEventData::Thought {
            content: "ORCHESTRATION CHAIN: local text continuation requested; executing bounded artifact delegate directly instead of letting the coordinator search the web.".to_string(),
        });
        messages.push(Message::system(
            "BENSHU_ORCHESTRATION_LOCAL_FILE_CONTINUATION".to_string(),
        ));
        let continuation_context = Self::latest_artifact_continuation_context(messages)
            .map(|context| {
                format!(
                    "\n\nExisting artifact/work-in-progress context from verified runtime receipts:\n{context}\n\
                     Continue from these existing paths/projects and preserve already-created artifact identity. \
                     Do not create a new project/document unless the existing artifact is unusable; if it is unusable, state the concrete blocker."
                )
            })
            .unwrap_or_default();

        bridge
            .executor()
            .coordinate(
                messages,
                String::new(),
                vec![(
                    format!("orchestrated-local-continuation-{}", steps),
                    "delegate".to_string(),
                    serde_json::json!({
                        "role": "writer",
                        "task": format!(
                            "Continue or append the requested local written artifact. Reuse existing project/artifact identifiers when provided, preserve title, entities, continuity rules, and main arc/argument, then write the next bounded update with the available writing/file tool. External acquisition is not required unless the original user request explicitly asks for it. Original user request: {}{}",
                            query,
                            continuation_context
                        ),
                        "full_user_request": query
                    }),
                )],
                steps,
                history,
                tool_trace,
            )
            .await?;

        if let Some((failed_tool_name, error)) = Self::latest_tool_error_result(messages) {
            bridge.emit(AgentEventData::Thought {
                content: format!(
                    "ORCHESTRATION FINALIZE: local file continuation failed inside `{}`; returning the blocker directly.",
                    failed_tool_name
                ),
            });
            let final_text = Self::tool_failure_delivery_text(&query, &failed_tool_name, &error);
            let usage = TokenUsage::default();
            return bridge
                .finalize_outcome(
                    messages,
                    final_text,
                    Some(usage),
                    Vec::new(),
                    tool_trace.clone(),
                    steps,
                )
                .await
                .map(StepDisposition::Finalized);
        }

        let Some(file_result) = Self::latest_successful_tool_result_text(messages, "delegate")
        else {
            return Ok(StepDisposition::ContinueLoop);
        };
        if Self::tool_result_is_blocked(&file_result) {
            bridge.emit(AgentEventData::Thought {
                content: "ORCHESTRATION RECOVERY: local artifact continuation returned a blocker; continuing through the normal recovery loop instead of reporting a false completion.".to_string(),
            });
            return Ok(StepDisposition::ContinueLoop);
        }
        let final_text = if Self::query_prefers_chinese(&query) {
            let prefix = if file_result.contains("operation: status") {
                "已完成本地产物状态检查。"
            } else if file_result.contains("operation: read_chapter") {
                "已读取本地长文产物。"
            } else {
                "已完成本地长文续写并保存。"
            };
            format!("{prefix}\n\n{}", file_result)
        } else {
            let prefix = if file_result.contains("operation: status") {
                "The local artifact status check is complete."
            } else if file_result.contains("operation: read_chapter") {
                "The local long-form artifact has been read."
            } else {
                "The local long-form continuation has been written and saved."
            };
            format!("{prefix}\n\n{}", file_result)
        };
        let usage = TokenUsage::default();
        bridge
            .finalize_outcome(
                messages,
                final_text,
                Some(usage),
                Vec::new(),
                tool_trace.clone(),
                steps,
            )
            .await
            .map(StepDisposition::Finalized)
    }

    pub(super) async fn try_file_artifact_followup_after_import(
        bridge: &dyn AgentLiaison,
        messages: &mut Vec<Message>,
        query: &str,
        steps: usize,
        history: &mut QueryHistory,
        tool_trace: &mut Vec<ToolCallData>,
    ) -> Result<Option<String>> {
        if !Self::query_requests_file_artifact(query)
            || Self::has_system_marker_after_latest_user(
                messages,
                "BENSHU_ORCHESTRATION_CHAIN_FILE_ARTIFACT",
            )
        {
            return Ok(None);
        }

        let import_receipt = Self::latest_successful_tool_result_text(messages, "delegate")
            .filter(|result| Self::tool_result_has_knowledge_persistence_effect(result));
        if Self::query_requests_knowledge_persistence(query) && import_receipt.is_none() {
            bridge.emit(AgentEventData::Thought {
                content: "ORCHESTRATION RECOVERY: file artifact follow-up is waiting for a durable knowledge.imported receipt; continuing instead of writing from an unconfirmed or blocked import.".to_string(),
            });
            return Ok(None);
        }

        bridge.emit(AgentEventData::Thought {
            content: Self::user_facing_progress_message("file_artifact", query),
        });
        bridge.emit(AgentEventData::Thought {
            content: "ORCHESTRATION CHAIN: knowledge import completed and the user requested a file artifact; executing bounded artifact delegate before final delivery.".to_string(),
        });
        messages.push(Message::system(
            "BENSHU_ORCHESTRATION_CHAIN_FILE_ARTIFACT".to_string(),
        ));

        let mut artifact_task = query.to_string();
        let has_imported_source = import_receipt.as_ref().is_some_and(|result| {
            Self::lookup_result_satisfies_requested_knowledge_depth(query, result)
        });
        if let Some(evidence) = Self::latest_lookup_result_for_followup_execution(messages) {
            if !has_imported_source {
                if let Some(gap) = Self::collection_evidence_gap_for_query(query, &evidence) {
                    return Ok(Some(Self::collection_evidence_gap_blocker(
                        query, gap, &evidence,
                    )));
                }
                if !Self::lookup_result_satisfies_requested_knowledge_depth(query, &evidence) {
                    return Ok(Some(Self::metadata_surrogate_depth_blocker(
                        query, &evidence,
                    )));
                }
            }
            let evidence_preview = Self::compact_lookup_evidence_for_file_artifact(&evidence);
            artifact_task.push_str("\n\nVerified researcher evidence:\n");
            artifact_task.push_str(&evidence_preview);
        }
        if let Some(import_receipt) = import_receipt {
            let receipt_preview: String = import_receipt.chars().take(2_000).collect();
            artifact_task.push_str("\n\nKnowledge import receipt:\n");
            artifact_task.push_str(&receipt_preview);
        }

        bridge
            .executor()
            .coordinate(
                messages,
                String::new(),
                vec![Self::file_artifact_delegate_call(
                    steps,
                    query,
                    &artifact_task,
                )],
                steps,
                history,
                tool_trace,
            )
            .await?;

        if let Some((failed_tool_name, error)) = Self::latest_tool_error_result(messages) {
            bridge.emit(AgentEventData::Thought {
                content: format!(
                    "ORCHESTRATION FINALIZE: bounded file-artifact follow-up failed inside `{}`; returning the blocker directly.",
                    failed_tool_name
                ),
            });
            return Ok(Some(Self::tool_failure_delivery_text(
                query,
                &failed_tool_name,
                &error,
            )));
        }

        let Some(file_result) = Self::latest_successful_tool_result_text(messages, "delegate")
        else {
            return Ok(None);
        };
        if !Self::tool_result_satisfies_artifact_request(query, &file_result) {
            if Self::requested_text_target_chars(query).is_some()
                && (Self::tool_result_has_artifact_written_effect(&file_result)
                    || Self::tool_result_has_governed_artifact_checkpoint(&file_result))
            {
                bridge.emit(AgentEventData::Thought {
                    content: "ORCHESTRATION RECOVERY: the writer produced a governed artifact checkpoint, but the requested text scale is not complete; continuing the same artifact task instead of finalizing a setup or partial draft.".to_string(),
                });
                messages.push(Message::system(format!(
                    "{}\n\nBENSHU_ARTIFACT_SCALE_CONTINUATION_REQUIRED\n\
                     The latest writer result is only an intermediate checkpoint for the requested artifact scale.\n\
                     Continue the same writing task with the equipped writer tools. Preserve any existing project path, title, contract, sources, truth/continuity ledger, and target size. Do not restart from a new project unless the previous project is unreadable. Plan or write the next bounded section/chapter, audit and revise when needed, and export/save the requested final file only after the reported total units satisfy the user's explicit target.",
                    reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED
                )));
                return Ok(None);
            }
            let compact = Self::strip_spurious_completion_claims(
                &Self::compact_tool_result_for_recovery(&file_result),
            );
            return Ok(Some(if Self::query_prefers_chinese(query) {
                format!(
                    "知识库写入已经完成，但本地文件产物还没有完成：缺少 `artifact.written` 回执。\n\n当前具体卡点：{}",
                    compact.trim()
                )
            } else {
                format!(
                    "The knowledge import completed, but the local file artifact is not complete yet: the `artifact.written` receipt is missing.\n\nCurrent blocker: {}",
                    compact.trim()
                )
            }));
        }
        let final_text = if Self::query_prefers_chinese(query) {
            format!(
                "已完成知识库写入，并已执行本地文件保存步骤。\n\n{}",
                file_result
            )
        } else {
            format!(
                "The knowledge import is complete, and the local file artifact step has run.\n\n{}",
                file_result
            )
        };
        Ok(Some(final_text))
    }

    pub(super) fn inject_efficiency_warning_if_needed(
        &self,
        messages: &mut Vec<Message>,
        tool_trace: &[ToolCallData],
        recent_tool_call_count: usize,
    ) {
        let efficiency_threshold =
            self.config.efficiency_trigger_secs * reasoner_constants::SEC_TO_MS;
        if efficiency_threshold == 0 {
            return;
        }

        let slow_tools: Vec<String> = tool_trace
            .iter()
            .rev()
            .take(recent_tool_call_count)
            .filter(|trace| trace.duration_ms > efficiency_threshold)
            .map(|trace| format!("'{}' ({}ms)", trace.name, trace.duration_ms))
            .collect();

        if slow_tools.is_empty() {
            return;
        }

        let warning = format!(
            "{}\n\
             The following tools were executed with high latency (>{}s): {}.\n\n\
             IMPERATIVE: If you expect to perform similar computations frequently, consider using 'forge_skill' to create a high-performance script to reduce future execution time.",
            reasoner_constants::MARKER_EFFICIENCY_WARNING,
            self.config.efficiency_trigger_secs,
            slow_tools.join(", ")
        );
        info!("Reasoner: Injecting efficiency warning due to slow tools.");
        messages.push(Message::system(warning));
    }

    pub(super) async fn try_pre_llm_knowledge_followup(
        &self,
        bridge: &dyn AgentLiaison,
        messages: &mut Vec<Message>,
        steps: usize,
        history: &mut QueryHistory,
        tool_trace: &mut Vec<ToolCallData>,
    ) -> Result<StepDisposition> {
        let latest_query = Self::latest_user_query(messages).unwrap_or_default();
        let persistence_query =
            Self::latest_knowledge_persistence_query(messages).unwrap_or(latest_query);
        if !Self::query_requests_knowledge_persistence(&persistence_query)
            || Self::has_system_marker_after_latest_user(
                messages,
                "BENSHU_ORCHESTRATION_CHAIN_KNOWLEDGE",
            )
            || Self::current_turn_has_completed_knowledge_import(messages)
        {
            return Ok(StepDisposition::NotApplicable);
        }

        let Some(result) = Self::latest_lookup_result_for_followup_execution(messages) else {
            return Ok(StepDisposition::NotApplicable);
        };
        if let Some(gap) = Self::collection_evidence_gap_for_query(&persistence_query, &result) {
            let marker = "BENSHU_ORCHESTRATION_COLLECTION_EVIDENCE_GAP";
            if Self::has_system_marker_after_latest_user(messages, marker) {
                return Ok(StepDisposition::NotApplicable);
            }
            bridge.emit(AgentEventData::Thought {
                content: Self::user_facing_progress_message("knowledge_import", &persistence_query),
            });
            bridge.emit(AgentEventData::Thought {
                content: format!(
                    "ORCHESTRATION CHAIN: collection evidence gate paused automatic knowledge import; observed {} of {} requested item-level records.",
                    gap.observed, gap.requested
                ),
            });
            messages.push(Message::system(marker.to_string()));
            messages.push(Message::system(
                Self::collection_evidence_recovery_instruction(&persistence_query, gap, &result),
            ));
            return Ok(StepDisposition::ContinueLoop);
        }
        if !Self::lookup_result_satisfies_requested_knowledge_depth(&persistence_query, &result) {
            let source_depth_fetch_marker = "BENSHU_ORCHESTRATION_SOURCE_DEPTH_FETCH";
            let result_lowered = result.to_ascii_lowercase();
            let result_already_fetched = result_lowered.contains("executed_tool: web_fetch")
                || result_lowered.contains("fetched_result:")
                || result_lowered.contains("content_body")
                || result_lowered.contains("document body")
                || result_lowered.contains("article body");
            if !result_already_fetched
                && !Self::has_system_marker_after_latest_user(messages, source_depth_fetch_marker)
            {
                if let Some(url) = Self::followup_execution_source_url(&persistence_query, &result)
                {
                    if self.tool_is_enabled("web_fetch") {
                        bridge.emit(AgentEventData::Thought {
                            content: Self::user_facing_progress_message(
                                "source_fetch",
                                &persistence_query,
                            ),
                        });
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION CHAIN: lookup returned a concrete candidate URL but not source-body depth; executing bounded `web_fetch` before knowledge import.".to_string(),
                        });
                        messages.push(Message::system(source_depth_fetch_marker.to_string()));
                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![(
                                    format!("orchestrated-source-depth-fetch-{}", steps),
                                    "web_fetch".to_string(),
                                    serde_json::json!({
                                        "url": url,
                                        "structured": true
                                    }),
                                )],
                                steps,
                                history,
                                tool_trace,
                            )
                            .await?;
                        return Ok(StepDisposition::ContinueLoop);
                    }

                    if self.tool_is_enabled("delegate") {
                        bridge.emit(AgentEventData::Thought {
                            content: Self::user_facing_progress_message(
                                "source_fetch",
                                &persistence_query,
                            ),
                        });
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION CHAIN: lookup returned a concrete candidate URL but this agent lacks direct fetch; delegating a bounded source-body fetch before knowledge import.".to_string(),
                        });
                        messages.push(Message::system(source_depth_fetch_marker.to_string()));
                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![(
                                    format!("orchestrated-source-depth-delegate-fetch-{}", steps),
                                    "delegate".to_string(),
                                    serde_json::json!({
                                        "role": "researcher",
                                        "full_user_request": persistence_query,
                                        "task": format!(
                                            "Fetch this concrete candidate source URL for source-body depth before knowledge import. Do not run another open-ended search unless the URL is inaccessible or clearly not a readable source. Return a compact receipt with status, worker: researcher, executed_tool, source_url, and fetched_result/body evidence. URL: {}\n\nOriginal user request:\n{}",
                                            url,
                                            persistence_query
                                        )
                                    }),
                                )],
                                steps,
                                history,
                                tool_trace,
                            )
                            .await?;
                        return Ok(StepDisposition::ContinueLoop);
                    }
                }
            }
            let marker = "BENSHU_ORCHESTRATION_SOURCE_DEPTH_GAP";
            if Self::has_system_marker_after_latest_user(messages, marker) {
                return Ok(StepDisposition::NotApplicable);
            }
            bridge.emit(AgentEventData::Thought {
                content: Self::user_facing_progress_message("knowledge_import", &persistence_query),
            });
            bridge.emit(AgentEventData::Thought {
                content: "ORCHESTRATION CHAIN: source-depth gate paused automatic knowledge import because the latest lookup looked like an index/metadata page, not requested source content.".to_string(),
            });
            messages.push(Message::system(marker.to_string()));
            messages.push(Message::system(format!(
                "BENSHU_SOURCE_DEPTH_RECOVERY_REQUIRED\n\
                 The latest lookup is not sufficient for the user's requested knowledge import depth. It appears to be public metadata, a list, search results, a category/index page, or another locator surface rather than the source body/detail content requested. Do not import this lookup as the requested source content and do not write a source-grounded artifact from it. Continue the same task by obtaining a concrete source item, detail page, readable body, file/document content, or a clear runtime blocker explaining what is missing.\n\n\
                 User request:\n{}\n\nLatest lookup preview:\n{}",
                persistence_query,
                Self::compact_lookup_evidence_for_file_artifact(&result)
            )));
            return Ok(StepDisposition::ContinueLoop);
        }
        if !Self::lookup_result_satisfies_requested_material_alignment(&persistence_query, &result)
        {
            let marker = "BENSHU_ORCHESTRATION_SOURCE_ALIGNMENT_GAP";
            if Self::has_system_marker_after_latest_user(messages, marker) {
                return Ok(StepDisposition::NotApplicable);
            }
            bridge.emit(AgentEventData::Thought {
                content: Self::user_facing_progress_message("source_fetch", &persistence_query),
            });
            bridge.emit(AgentEventData::Thought {
                content: "ORCHESTRATION CHAIN: source alignment gate paused automatic knowledge import because the fetched body did not preserve the user's requested source-material intent.".to_string(),
            });
            messages.push(Message::system(marker.to_string()));
            messages.push(Message::system(format!(
                "BENSHU_SOURCE_ALIGNMENT_RECOVERY_REQUIRED\n\
                 The latest fetched body is readable, but it does not satisfy the user's original source-material intent. Do not import it into the knowledge base and do not use it as grounding for the requested artifact. Continue the same task by obtaining a source body/detail content that matches the user's explicit material type, or return a clear runtime blocker if no aligned source can be verified.\n\n\
                 User request:\n{}\n\nLatest lookup preview:\n{}",
                persistence_query,
                Self::compact_lookup_evidence_for_file_artifact(&result)
            )));
            return Ok(StepDisposition::ContinueLoop);
        }
        let Some(url) = Self::explicit_source_url_in_result(&result)
            .or_else(|| Self::best_lookup_source_url_for_query(&persistence_query, &result))
        else {
            return Ok(StepDisposition::NotApplicable);
        };

        if !self.tool_is_enabled("delegate") {
            let marker = "BENSHU_ORCHESTRATION_CHAIN_KNOWLEDGE_REQUIRES_COORDINATOR";
            if Self::has_system_marker_after_latest_user(messages, marker) {
                return Ok(StepDisposition::NotApplicable);
            }
            bridge.emit(AgentEventData::Thought {
                content: "ORCHESTRATION CHAIN: current worker found importable source evidence but is not equipped with cross-worker delegation; returning the evidence to the coordinator for the next stage.".to_string(),
            });
            messages.push(Message::system(marker.to_string()));
            messages.push(Message::system(format!(
                "BENSHU_KNOWLEDGE_IMPORT_COORDINATOR_HANDOFF_REQUIRED\n\
                 The current agent found a concrete source URL for a task that also asks for knowledge persistence, but this agent is not equipped with `delegate` and must not call unavailable orchestration tools.\n\
                 Finish this worker subtask by returning the source URL, title if known, and a compact evidence summary. The coordinator/main agent will route the actual knowledge import to an equipped worker.\n\n\
                 Source URL: {url}\n\n\
                 User request:\n{persistence_query}\n\n\
                 Evidence preview:\n{}",
                Self::compact_lookup_evidence_for_file_artifact(&result)
            )));
            return Ok(StepDisposition::ContinueLoop);
        }

        bridge.emit(AgentEventData::Thought {
            content: Self::user_facing_progress_message("knowledge_import", &persistence_query),
        });
        bridge.emit(AgentEventData::Thought {
            content: "ORCHESTRATION CHAIN (pre-llm): researcher already returned a concrete source URL for a knowledge-persistence task; executing bounded `delegate(knowledge, ...)` before another model round.".to_string(),
        });

        messages.push(Message::system(
            "BENSHU_ORCHESTRATION_CHAIN_KNOWLEDGE".to_string(),
        ));

        bridge
            .executor()
            .coordinate(
                messages,
                String::new(),
                vec![(
                    format!("orchestrated-knowledge-pre-llm-{}", steps),
                    "delegate".to_string(),
                    serde_json::json!({
                        "role": "knowledge",
                        "full_user_request": persistence_query,
                        "task": format!(
                            "Import this concrete source URL into the knowledge base exactly once. Do not run another lookup. URL: {}\n\nfetched_result:\n{}",
                            url,
                            Self::compact_lookup_evidence_for_knowledge_import(&result)
                        )
                    }),
                )],
                steps,
                history,
                tool_trace,
            )
            .await?;

        if let Some((failed_tool_name, error)) = Self::latest_tool_error_result(messages) {
            let final_text =
                Self::tool_failure_delivery_text(&persistence_query, &failed_tool_name, &error);
            bridge.emit(AgentEventData::Thought {
                content: format!(
                    "ORCHESTRATION FINALIZE: pre-llm bounded knowledge-import follow-up failed inside `{}`; returning the blocker directly.",
                    failed_tool_name
                ),
            });
            let outcome = bridge
                .finalize_outcome(
                    messages,
                    final_text,
                    None::<TokenUsage>,
                    Vec::new(),
                    tool_trace.clone(),
                    steps,
                )
                .await?;
            return Ok(StepDisposition::Finalized(outcome));
        }

        let Some(knowledge_result) = Self::latest_successful_tool_result_text(messages, "delegate")
        else {
            return Ok(StepDisposition::NotApplicable);
        };
        if !knowledge_result
            .to_ascii_lowercase()
            .contains("worker: knowledge")
        {
            return Ok(StepDisposition::NotApplicable);
        }

        if Self::query_requests_post_import_delivery(&persistence_query)
            && !Self::has_system_marker_after_latest_user(
                messages,
                "BENSHU_ORCHESTRATION_CHAIN_FINAL_DELIVERY",
            )
        {
            if let Some(gap) = Self::collection_evidence_gap_for_query(&persistence_query, &result)
            {
                let final_text =
                    Self::collection_evidence_gap_blocker(&persistence_query, gap, &result);
                bridge.emit(AgentEventData::Thought {
                    content: "ORCHESTRATION FINALIZE: collection evidence gate blocked downstream delivery after knowledge import because the source set is incomplete.".to_string(),
                });
                let outcome = bridge
                    .finalize_outcome(
                        messages,
                        final_text,
                        None::<TokenUsage>,
                        Vec::new(),
                        tool_trace.clone(),
                        steps,
                    )
                    .await?;
                return Ok(StepDisposition::Finalized(outcome));
            }
            if Self::query_requests_file_artifact(&persistence_query)
                && !Self::has_system_marker_after_latest_user(
                    messages,
                    "BENSHU_ORCHESTRATION_CHAIN_FILE_ARTIFACT",
                )
            {
                bridge.emit(AgentEventData::Thought {
                    content: Self::user_facing_progress_message(
                        "file_artifact",
                        &persistence_query,
                    ),
                });
                bridge.emit(AgentEventData::Thought {
                    content: "ORCHESTRATION CHAIN: knowledge import completed and the user requested a file artifact; executing bounded artifact delegate before final delivery.".to_string(),
                });
                messages.push(Message::system(
                    "BENSHU_ORCHESTRATION_CHAIN_FILE_ARTIFACT".to_string(),
                ));

                let mut artifact_task = persistence_query.to_string();
                let evidence_preview = Self::compact_lookup_evidence_for_file_artifact(&result);
                artifact_task.push_str("\n\nVerified researcher evidence:\n");
                artifact_task.push_str(&evidence_preview);
                let receipt_preview: String = knowledge_result.chars().take(2_000).collect();
                artifact_task.push_str("\n\nKnowledge import receipt:\n");
                artifact_task.push_str(&receipt_preview);

                bridge
                    .executor()
                    .coordinate(
                        messages,
                        String::new(),
                        vec![Self::file_artifact_delegate_call(
                            steps,
                            &persistence_query,
                            &artifact_task,
                        )],
                        steps,
                        history,
                        tool_trace,
                    )
                    .await?;

                if let Some((failed_tool_name, error)) = Self::latest_tool_error_result(messages) {
                    let final_text = Self::tool_failure_delivery_text(
                        &persistence_query,
                        &failed_tool_name,
                        &error,
                    );
                    bridge.emit(AgentEventData::Thought {
                        content: format!(
                            "ORCHESTRATION FINALIZE: bounded file-artifact follow-up failed inside `{}`; returning the blocker directly.",
                            failed_tool_name
                        ),
                    });
                    let outcome = bridge
                        .finalize_outcome(
                            messages,
                            final_text,
                            None::<TokenUsage>,
                            Vec::new(),
                            tool_trace.clone(),
                            steps,
                        )
                        .await?;
                    return Ok(StepDisposition::Finalized(outcome));
                }

                if let Some(file_result) =
                    Self::latest_successful_tool_result_text(messages, "delegate")
                {
                    if !Self::tool_result_satisfies_artifact_request(
                        &persistence_query,
                        &file_result,
                    ) {
                        if Self::requested_text_target_chars(&persistence_query).is_some()
                            && (Self::tool_result_has_artifact_written_effect(&file_result)
                                || Self::tool_result_has_governed_artifact_checkpoint(&file_result))
                        {
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION RECOVERY: bounded artifact delegate produced a governed checkpoint, but the requested text scale is not complete; continuing instead of finalizing setup or a partial draft.".to_string(),
                            });
                            messages.push(Message::system(format!(
                                "{}\n\nBENSHU_ARTIFACT_SCALE_CONTINUATION_REQUIRED\n\
                                 The latest writer result is only an intermediate checkpoint for the requested artifact scale.\n\
                                 Continue the same writing task with the equipped writer tools. Preserve any existing project path, title, contract, sources, truth/continuity ledger, and target size. Do not restart from a new project unless the previous project is unreadable. Plan or write the next bounded section/chapter, audit and revise when needed, and export/save the requested final file only after the reported total units satisfy the user's explicit target.",
                                reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED
                            )));
                            return Ok(StepDisposition::ContinueLoop);
                        }
                        let compact = Self::strip_spurious_completion_claims(
                            &Self::compact_tool_result_for_recovery(&file_result),
                        );
                        let final_text = if Self::query_prefers_chinese(&persistence_query) {
                            format!(
                                "知识库写入已经完成，但本地文件产物还没有完成：缺少 `artifact.written` 回执。\n\n当前具体卡点：{}",
                                compact.trim()
                            )
                        } else {
                            format!(
                                "The knowledge-base import completed, but the local file artifact is not complete yet: the `artifact.written` receipt is missing.\n\nCurrent blocker: {}",
                                compact.trim()
                            )
                        };
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION FINALIZE: bounded file-artifact follow-up returned planning/read-only evidence without artifact.written; returning blocker instead of positive completion.".to_string(),
                        });
                        let outcome = bridge
                            .finalize_outcome(
                                messages,
                                final_text,
                                None::<TokenUsage>,
                                Vec::new(),
                                tool_trace.clone(),
                                steps,
                            )
                            .await?;
                        return Ok(StepDisposition::Finalized(outcome));
                    }
                    let final_text = if Self::query_prefers_chinese(&persistence_query) {
                        format!(
                            "已完成知识库写入，并已执行本地文件保存步骤。\n\n{}",
                            file_result
                        )
                    } else {
                        format!(
                            "The knowledge-base import is complete, and the local file artifact step has run.\n\n{}",
                            file_result
                        )
                    };
                    bridge.emit(AgentEventData::Thought {
                        content: "ORCHESTRATION FINALIZE: knowledge import and bounded file-artifact chain completed; returning the file result.".to_string(),
                    });
                    let outcome = bridge
                        .finalize_outcome(
                            messages,
                            final_text,
                            None::<TokenUsage>,
                            Vec::new(),
                            tool_trace.clone(),
                            steps,
                        )
                        .await?;
                    return Ok(StepDisposition::Finalized(outcome));
                }
            }

            if let Some(final_text) =
                Self::synthesize_post_import_delivery(&persistence_query, messages)
            {
                bridge.emit(AgentEventData::Thought {
                    content: "ORCHESTRATION FINALIZE: knowledge import completed and verified researcher data is already available; returning the final requested delivery immediately instead of spending another model round.".to_string(),
                });
                let outcome = bridge
                    .finalize_outcome(
                        messages,
                        final_text,
                        None::<TokenUsage>,
                        Vec::new(),
                        tool_trace.clone(),
                        steps,
                    )
                    .await?;
                return Ok(StepDisposition::Finalized(outcome));
            }
            Self::push_post_import_delivery_instruction(messages, &persistence_query);
            bridge.emit(AgentEventData::Thought {
                content: "ORCHESTRATION CHAIN: knowledge import completed, but the user also requested final analysis/delivery; continuing to one bounded final synthesis round instead of returning only the import receipt.".to_string(),
            });
            return Ok(StepDisposition::ContinueLoop);
        }

        let final_text = Self::summarize_delegate_delivery(
            &persistence_query,
            &knowledge_result,
            Self::query_prefers_chinese(&persistence_query),
        );
        bridge.emit(AgentEventData::Thought {
            content: "ORCHESTRATION FINALIZE: pre-llm lookup -> knowledge chain completed in-code; returning the import receipt directly.".to_string(),
        });
        let outcome = bridge
            .finalize_outcome(
                messages,
                final_text,
                None::<TokenUsage>,
                Vec::new(),
                tool_trace.clone(),
                steps,
            )
            .await?;
        Ok(StepDisposition::Finalized(outcome))
    }

    pub(crate) fn current_turn_has_completed_knowledge_import(messages: &[Message]) -> bool {
        Self::current_turn_messages(messages).iter().any(|message| {
            let text = message.text().to_ascii_lowercase();
            (text.contains("worker: knowledge")
                || text.contains("\"worker\":\"knowledge\"")
                || text.contains("\"worker\": \"knowledge\""))
                && (text.contains("runtime_effect: knowledge.imported")
                    || text.contains("\"runtime_effect\":\"knowledge.imported\"")
                    || text.contains("\"runtime_effect\": \"knowledge.imported\"")
                    || text.contains("imported web knowledge"))
        })
    }
}
