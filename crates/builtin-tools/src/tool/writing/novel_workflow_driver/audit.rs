use super::*;

const MAX_FINAL_STATE_OBSERVER_ATTEMPTS: usize = 5;

fn required_observer_change_count(authority: &SealedChapterAuthority) -> usize {
    [
        authority.chapter_contract.new_state_after_chapter.as_str(),
        authority.chapter_contract.character_change.as_str(),
        authority.chapter_contract.relationship_delta.as_str(),
        authority.chapter_contract.world_change.as_str(),
        authority.chapter_contract.power_delta.as_str(),
        authority.chapter_contract.resource_delta.as_str(),
    ]
    .into_iter()
    .filter(|value| !value.trim().is_empty())
    .count()
        + authority
            .chapter_contract
            .hook_opened
            .iter()
            .filter(|value| !value.trim().is_empty())
            .count()
        + authority
            .chapter_contract
            .hook_paid_off
            .iter()
            .filter(|value| !value.trim().is_empty())
            .count()
}

fn final_observer_token_budget(required_changes: usize) -> u64 {
    1_600u64
        .saturating_add(
            u64::try_from(required_changes)
                .unwrap_or(u64::MAX)
                .saturating_mul(220),
        )
        .min(3_600)
}

impl NovelChapterRunner {
    pub(super) async fn ensure_delivery_advisory_window(
        &self,
        range_end: usize,
    ) -> anyhow::Result<()> {
        if !chapter_completes_delivery_review_window(range_end) {
            return Ok(());
        }
        let preparation = match call_novel_studio_json_with_timeout(
            &self.tool,
            json!({
                "action": "prepare_delivery_advisory_window",
                "project_path": self.project_path,
                "chapter_number": range_end
            }),
            local_tool_stage_timeout_secs(),
            "prepare_delivery_advisory_window",
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                record_workflow_checkpoint(
                    &self.runtime,
                    range_end as u32,
                    "novel-delivery-review:prepare-degraded",
                    format!("第 {range_end} 章交付窗口准备失败，已按非阻断策略继续：{error}"),
                )
                .await;
                return Ok(());
            }
        };
        if !preparation
            .get("ready")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || preparation
                .get("already_recorded")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            return Ok(());
        }
        let aggregate_fingerprint = preparation
            .get("aggregate_fingerprint")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let window = preparation
            .get("window")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let prompt = delivery_advisory_window_prompt(&self.language, &window);
        let generated = tokio::time::timeout(
            std::time::Duration::from_secs(90),
            self.agent
                .generate_text_only_with_max_tokens(&clean_provider_prompt(&prompt), Some(900)),
        )
        .await;
        let (status, advisories, score, degraded_reason) = match generated {
            Ok(Ok(raw)) => match parse_delivery_advisory_window_output(&raw) {
                Some(output) => ("completed", output.advisories, output.score, String::new()),
                None => (
                    "terminal_degraded",
                    Vec::new(),
                    None,
                    "delivery advisory output did not satisfy the typed schema".to_string(),
                ),
            },
            Ok(Err(error)) => (
                "terminal_degraded",
                Vec::new(),
                None,
                format!("delivery advisory inference failed: {error}"),
            ),
            Err(_) => (
                "terminal_degraded",
                Vec::new(),
                None,
                "delivery advisory inference exceeded its bounded timeout".to_string(),
            ),
        };
        let delivery_advisories = advisories
            .into_iter()
            .map(|item| {
                json!({
                    "category": item.category,
                    "message": item.message
                })
            })
            .collect::<Vec<_>>();
        match call_novel_studio_json_with_timeout(
            &self.tool,
            json!({
                "action": "commit_delivery_advisory_window",
                "project_path": self.project_path,
                "chapter_number": range_end,
                "candidate_fingerprint": aggregate_fingerprint,
                "delivery_advisories": delivery_advisories,
                "score": score,
                "status": status,
                "feedback": degraded_reason
            }),
            local_tool_stage_timeout_secs(),
            "commit_delivery_advisory_window",
        )
        .await
        {
            Ok(_) => {
                record_workflow_checkpoint(
                    &self.runtime,
                    range_end as u32,
                    if status == "completed" {
                        "novel-delivery-review:completed"
                    } else {
                        "novel-delivery-review:terminal-degraded"
                    },
                    if status == "completed" {
                        format!(
                            "已完成截至第 {range_end} 章的五章交付复盘；建议仅影响后续表达方式。"
                        )
                    } else {
                        format!("截至第 {range_end} 章的五章交付复盘已降级终止，不阻断下一章。")
                    },
                )
                .await;
            }
            Err(error) => {
                record_workflow_checkpoint(
                    &self.runtime,
                    range_end as u32,
                    "novel-delivery-review:commit-degraded",
                    format!("五章交付复盘记录提交失败，已按非阻断策略继续：{error}"),
                )
                .await;
            }
        }
        Ok(())
    }

    pub(super) async fn observe_final_chapter_state(
        &self,
        chapter_number: usize,
        write_result: &Value,
        previous_error: Option<&str>,
    ) -> anyhow::Result<novel_runner::FinalChapterObservation> {
        let content = read_chapter_body_from_write_result(write_result)
            .await
            .ok_or_else(|| anyhow::anyhow!("final chapter body is unavailable for settlement"))?;
        let authority = self.sealed_authority(chapter_number).await?;
        let authority_context = self
            .authority_projection_json(chapter_number, AuthorityRole::Observer)
            .await?;
        let prompt = novel_runner::final_chapter_observer_prompt(
            &self.language,
            chapter_number,
            &authority_context,
            &authority.chapter_contract.new_state_after_chapter,
            &content,
            previous_error,
        );
        // The provider already enforces bounded first-chunk and idle-stream
        // timeouts, while the chapter executor owns the overall step deadline.
        // A second 90-second wall-clock timeout here used to cancel valid local
        // generations near the end of their JSON response.
        let raw = self
            .agent
            .generate_text_only_with_max_tokens(
                &clean_provider_prompt(&prompt),
                Some(final_observer_token_budget(required_observer_change_count(
                    &authority,
                ))),
            )
            .await?;
        novel_runner::parse_final_chapter_observation(&raw, &content)
    }

    pub(super) async fn settle_observed_final_chapter_state(
        &self,
        chapter_number: usize,
        write_result: &Value,
        mut initial_observation: Option<novel_runner::FinalChapterObservation>,
    ) -> anyhow::Result<Value> {
        let mut feedback = None;
        let mut last_settlement = None;
        for attempt in 1..=MAX_FINAL_STATE_OBSERVER_ATTEMPTS {
            let observation = if let Some(observation) = initial_observation.take() {
                Ok(observation)
            } else {
                self.observe_final_chapter_state(chapter_number, write_result, feedback.as_deref())
                    .await
            };
            let (content, observer_error) = match observation {
                Ok(observation) => (serde_json::to_string(&observation)?, None),
                Err(error) => ("{}".to_string(), Some(error.to_string())),
            };
            let mut settlement = call_novel_studio_json(
                &self.tool,
                json!({
                    "action": "settle_chapter_state",
                    "project_path": self.project_path,
                    "chapter_number": chapter_number,
                    "content": content,
                    "observer_attempts_exhausted":
                        attempt == MAX_FINAL_STATE_OBSERVER_ATTEMPTS
                }),
            )
            .await?;
            if let Some(error) = observer_error {
                settlement["observer_error"] = json!(error);
            }
            if settlement
                .pointer("/validation/passed")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Ok(settlement);
            }
            if attempt < MAX_FINAL_STATE_OBSERVER_ATTEMPTS {
                let mut errors =
                    collect_string_array_to_vec(settlement.pointer("/validation/warnings"));
                errors.extend(collect_string_array_to_vec(
                    settlement.pointer("/validation/advisories"),
                ));
                if let Some(error) = settlement.get("observer_error").and_then(Value::as_str) {
                    errors.push(error.to_string());
                }
                feedback = Some(if errors.is_empty() {
                    "状态结算未通过；重新逐字核对正文证据和密封权威字段。".to_string()
                } else {
                    errors.join("\n")
                });
                record_workflow_checkpoint(
                    &self.runtime,
                    chapter_number as u32,
                    "novel-chapter:state-observer:repair",
                    format!(
                        "第 {chapter_number} 章状态观察结果未通过，使用同一最终正文进行第 {attempt}/{MAX_FINAL_STATE_OBSERVER_ATTEMPTS} 次有界证据纠错。"
                    ),
                )
                .await;
            }
            last_settlement = Some(settlement);
        }
        Ok(last_settlement.unwrap_or_else(|| json!({
            "validation": {"passed": false, "warnings": ["state observer produced no settlement"]}
        })))
    }

    pub(super) async fn rule_first_audit_or_full_audit(
        &self,
        chapter_number: usize,
        write_result: &Value,
    ) -> anyhow::Result<Value> {
        let candidate_only = write_result
            .get("candidate_only")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if chapter_requires_llm_quality_audit(chapter_number) {
            if let Some(audit) = self
                .llm_quality_audit_chapter(chapter_number, write_result)
                .await?
            {
                return Ok(audit);
            }
        }
        if candidate_only {
            let passed = !value_has_hard_findings(write_result)
                && json_array_is_empty(write_result.pointer("/truth_validation/issues"));
            return Ok(json!({
                "success": true,
                "candidate_only": true,
                "runtime_effect": "artifact.candidate_audited",
                "review": {
                    "verdict": if passed { "passed" } else { "needs_revision" },
                    "findings": [],
                    "advisories": [],
                    "locally_validated": true
                },
                "read_only": true
            }));
        }
        if write_result_is_clean_for_rule_audit(write_result) {
            return call_novel_studio_json(
                &self.tool,
                json!({
                    "action": "review_chapter",
                    "project_path": self.project_path,
                    "chapter_number": chapter_number,
                    "verdict": "passed",
                    "feedback": "Rule-first audit accepted the draft because the persisted writer quality gate, mechanical warnings, and truth validation were all clean."
                }),
            )
            .await;
        }
        call_novel_studio_json(
            &self.tool,
            json!({
                "action": "audit_chapter",
                "project_path": self.project_path,
                "chapter_number": chapter_number
            }),
        )
        .await
    }

    async fn llm_quality_audit_chapter(
        &self,
        chapter_number: usize,
        write_result: &Value,
    ) -> anyhow::Result<Option<Value>> {
        let Some(content) = read_chapter_body_from_write_result(write_result).await else {
            return Ok(None);
        };
        let title = write_result
            .pointer("/chapter/title")
            .and_then(Value::as_str)
            .unwrap_or("");
        let quality_issues =
            collect_string_array_to_vec(write_result.pointer("/quality_gate/issues"));
        let quality_repairable =
            collect_string_array_to_vec(write_result.pointer("/quality_gate/repairable"));
        let truth_issues =
            collect_string_array_to_vec(write_result.pointer("/truth_validation/issues"));
        let deterministic = quality_issues
            .iter()
            .chain(quality_repairable.iter())
            .chain(truth_issues.iter())
            .cloned()
            .collect::<Vec<_>>();
        let authority_context = self
            .authority_projection_json(chapter_number, AuthorityRole::Auditor)
            .await?;
        let prompt = llm_quality_audit_prompt(
            &self.language,
            chapter_number,
            title,
            &deterministic,
            &authority_context,
            &content,
        );
        let raw = match tokio::time::timeout(
            std::time::Duration::from_secs(90),
            self.agent
                .generate_text_only_with_max_tokens(&clean_provider_prompt(&prompt), Some(700)),
        )
        .await
        {
            Ok(Ok(raw)) => raw,
            Ok(Err(_)) | Err(_) => return Ok(None),
        };
        let Some(audit) = parse_llm_quality_audit_output(&raw) else {
            return Ok(None);
        };
        let locally_confirmed_findings = local_hard_findings(write_result);
        let mut advisories = audit.advisories;
        let findings = audit
            .authority_conflicts
            .into_iter()
            .filter_map(|conflict| {
                let finding = validate_llm_authority_conflict(
                    &conflict,
                    &locally_confirmed_findings,
                    &authority_context,
                    &content,
                );
                if finding.is_none() {
                    advisories.push(conflict.message);
                }
                finding
            })
            .collect::<Vec<_>>();
        advisories.sort();
        advisories.dedup();
        if write_result
            .get("candidate_only")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let passed = findings.iter().all(|finding| !finding.hard_blocking())
                && !value_has_hard_findings(write_result);
            return Ok(Some(json!({
                "success": true,
                "candidate_only": true,
                "runtime_effect": "artifact.candidate_audited",
                "review": {
                    "verdict": if passed { "passed" } else { "needs_revision" },
                    "findings": findings,
                    "advisories": advisories,
                    "score": audit.score,
                    "locally_validated": true
                },
                "read_only": true
            })));
        }
        call_novel_studio_json(
            &self.tool,
            json!({
                "action": "review_chapter",
                "project_path": self.project_path,
                "chapter_number": chapter_number,
                "findings": findings,
                "advisories": advisories,
                "score": audit.score,
                "feedback": "LLM audit telemetry was locally validated against the sealed authority and current body."
            }),
        )
        .await
        .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::{final_observer_token_budget, MAX_FINAL_STATE_OBSERVER_ATTEMPTS};

    #[test]
    fn observer_budget_scales_with_required_typed_transitions_and_stays_bounded() {
        assert_eq!(final_observer_token_budget(0), 1_600);
        assert_eq!(final_observer_token_budget(7), 3_140);
        assert_eq!(final_observer_token_budget(100), 3_600);
    }

    #[test]
    fn deterministic_final_state_observer_has_five_bounded_attempts() {
        assert_eq!(MAX_FINAL_STATE_OBSERVER_ATTEMPTS, 5);
    }
}
