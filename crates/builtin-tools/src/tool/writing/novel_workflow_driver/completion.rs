use super::*;

#[derive(Debug, Clone, Default)]
pub(super) struct ProjectCompletionGateDecision {
    pub(super) target_reached: bool,
    pub(super) narrative_closed: bool,
    pub(super) complete: bool,
    pub(super) needs_finale: bool,
    pub(super) reason: String,
    pub(super) finale_brief: Option<String>,
    pub(super) debt_ids: Vec<String>,
}

/// Project completion is a local decision over durable approved progress,
/// typed story debts, and the latest approval receipt. The model is not an
/// authority for the `complete` bit.
pub(super) async fn evaluate_project_completion_gate(
    _agent: Arc<dyn MultiAgent>,
    tool: &NovelStudioTool,
    project_path: &str,
    _task: &str,
    language: &str,
) -> anyhow::Result<ProjectCompletionGateDecision> {
    let status = call_novel_studio_json(
        tool,
        json!({
            "action": "status",
            "project_path": project_path
        }),
    )
    .await?;
    let state = status.get("state").cloned().unwrap_or_else(|| json!({}));
    let target_reached = state_target_reached_by_approved_units(&state)
        && state_usize(&state, "first_unapproved_chapter").is_none();
    if !target_reached {
        return Ok(ProjectCompletionGateDecision {
            reason: "target units or contiguous approval gate is not satisfied".to_string(),
            ..Default::default()
        });
    }

    let mut debts = state_completion_debts(&state);
    debts.sort();
    debts.dedup();
    if debts.is_empty() {
        return Ok(ProjectCompletionGateDecision {
            target_reached: true,
            narrative_closed: true,
            complete: true,
            needs_finale: false,
            reason: "typed completion obligations and latest approval receipt are satisfied"
                .to_string(),
            finale_brief: None,
            debt_ids: Vec::new(),
        });
    }

    let debt_ids = state_completion_debt_ids(&state);
    let debt_summary = debts.join("; ");
    Ok(ProjectCompletionGateDecision {
        target_reached: true,
        narrative_closed: false,
        complete: false,
        needs_finale: true,
        reason: format!("typed completion debts remain: {debt_summary}"),
        finale_brief: Some(if language_looks_cjk(language) {
            format!("用正文中的实际事件清理这些合同债务：{debt_summary}")
        } else {
            format!("Clear these contract debts through concrete final-body events: {debt_summary}")
        }),
        debt_ids,
    })
}

fn state_completion_debt_ids(state: &Value) -> Vec<String> {
    state
        .get("typed_completion_debts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|debt| debt.get("id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn state_completion_debts(state: &Value) -> Vec<String> {
    state
        .get("completion_blockers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| {
            !value.contains("Approved chapter bodies on disk have not reached target_units")
                && !value.contains("contiguous chapter sequence")
        })
        .map(ToString::to_string)
        .collect()
}

pub(super) fn append_finale_instruction(
    task: &str,
    gate: &ProjectCompletionGateDecision,
    language: &str,
) -> String {
    format!("{task}\n\n{}", finale_execution_directive(gate, language))
}

pub(super) fn finale_execution_directive(
    gate: &ProjectCompletionGateDecision,
    language: &str,
) -> String {
    let chinese = language_looks_cjk(language)
        || content_has_cjk(&gate.reason)
        || gate.finale_brief.as_deref().is_some_and(content_has_cjk);
    let brief = gate.finale_brief.as_deref().unwrap_or(if chinese {
        "完成合同中尚未结算的终局债务。"
    } else {
        "Complete the remaining typed ending obligations."
    });
    if chinese {
        return format!(
            "写作完成门补充：项目已达到最低字数，但以下类型化合同债务尚未完成：{}\n\
             收束重点：{brief}\n\
             下一章必须把债务转化为正文中的实际事件；允许超过目标字数，不开启新主线。\
             最终正文 observer 必须为每项人物、世界、关系与伏笔变化提供精确证据并完成结算。",
            gate.reason
        );
    }
    format!(
        "Completion-gate addendum: minimum units are reached, but typed contract debts remain: {}\n\
         Closure focus: {brief}\n\
         Convert the debts into concrete final-body events. Exceeding the target is allowed; do not open a new main line. \
         The final-body observer must provide exact evidence for every character, world, relationship, and hook change.",
        gate.reason
    )
}

pub(super) fn task_requests_complete_narrative(task: &str) -> bool {
    // The typed execution scope is the authoritative result of contract
    // stabilization.  Use it here as well as in planning so the completion
    // gate cannot silently fall back to a lexical one-chapter decision for an
    // all-remaining request.
    if let Some(scope) = super::planning::task_creation_execution_scope(task) {
        return matches!(
            scope,
            super::super::creation_contract::CreationDraftTurnScope::AllRemaining
        );
    }
    task_intent_surfaces(task)
        .iter()
        .any(|surface| surface_requests_complete_narrative(surface))
}

fn surface_requests_complete_narrative(surface: &str) -> bool {
    let lowered = surface.to_ascii_lowercase();
    [
        "真正结尾",
        "完整结尾",
        "完整结局",
        "写到结尾",
        "写到终局",
        "写到大结局",
        "大结局",
        "完结",
        "收尾",
        "the end",
        "complete ending",
        "finish the story",
        "until the ending",
    ]
    .iter()
    .any(|term| surface.contains(term) || lowered.contains(term))
        || surface_has_whole_story_write_done_intent(surface)
}

fn surface_has_whole_story_write_done_intent(surface: &str) -> bool {
    let Some(index) = surface.find("写完") else {
        return false;
    };
    let prefix = surface[..index].chars().rev().take(12).collect::<String>();
    let prefix = prefix.chars().rev().collect::<String>();
    let suffix = surface[index + "写完".len()..]
        .chars()
        .take(12)
        .collect::<String>();
    [
        "整本",
        "全书",
        "全文",
        "小说",
        "故事",
        "这本书",
        "本书",
        "全部",
        "所有章节",
        "主线",
    ]
    .iter()
    .any(|scope| prefix.contains(scope) || suffix.contains(scope))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_debt_ids_are_read_from_typed_status_not_prose_blockers() {
        let state = json!({
            "completion_blockers": [
                "Key hook ledger still has unresolved debts: 城门之约"
            ],
            "typed_completion_debts": [
                {"id": "ending-must-resolve-0001", "title": "城门之约"}
            ]
        });

        assert_eq!(
            state_completion_debt_ids(&state),
            vec!["ending-must-resolve-0001".to_string()]
        );
    }

    #[test]
    fn typed_all_remaining_scope_allows_elastic_completion() {
        let task = "__creation_execution_scope:all_remaining\n合同已确认，开始写作。";

        assert!(task_requests_complete_narrative(task));
    }

    #[test]
    fn typed_first_unit_scope_overrides_completion_words_in_prompt_prose() {
        let task = "__creation_execution_scope:first_unit\n请先写完完整结局前的第一章。";

        assert!(!task_requests_complete_narrative(task));
    }
}
