use super::*;

pub(super) const MAX_METADATA_REPAIR_ATTEMPTS: usize = 5;

pub(super) fn metadata_gate_needs_repair(write_result: &Value) -> bool {
    metadata_gate_blocks(write_result)
        || !json_array_is_empty(write_result.pointer("/metadata_gate/repairable"))
        || !json_array_is_empty(write_result.pointer("/truth_validation/issues"))
}

pub(super) fn metadata_gate_blocks(write_result: &Value) -> bool {
    !json_array_is_empty(write_result.pointer("/metadata_gate/blocking"))
}

pub(super) fn metadata_gate_has_repairable(write_result: &Value) -> bool {
    !json_array_is_empty(write_result.pointer("/metadata_gate/repairable"))
}

pub(super) fn metadata_issue_summary(write_result: &Value) -> String {
    let mut issues = Vec::new();
    collect_string_array(write_result.pointer("/metadata_gate/blocking"), &mut issues);
    collect_string_array(
        write_result.pointer("/metadata_gate/repairable"),
        &mut issues,
    );
    collect_string_array(
        write_result.pointer("/truth_validation/issues"),
        &mut issues,
    );
    if issues.is_empty() {
        "metadata gate did not provide issues".to_string()
    } else {
        issues
            .into_iter()
            .map(|issue| format!("- {}", issue.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(super) fn metadata_repair_allowed_with_audit(write_result: &Value, audit: &Value) -> bool {
    quality_gate_body_passed(write_result)
        && audit_passed(audit)
        && !value_has_hard_findings(write_result)
        && !value_has_hard_findings(audit)
}

pub(super) fn format_metadata_blocker_result(
    project_path: &str,
    chapter_number: usize,
    write_result: &Value,
) -> String {
    format!(
        "status: blocked\nworker: writer\nexecuted_tool: novel_studio\noperation: repair_chapter_metadata\nproject_path: {project_path}\nchapter_number: {chapter_number}\nruntime_effect: artifact.metadata_needs_repair\ndraft_status: preserved_needs_revision\nblockers: chapter body is preserved, but metadata repair did not converge\nmetadata_issues:\n{}",
        metadata_issue_summary(write_result)
    )
}

pub(super) fn metadata_repair_generation_limits(language: &str) -> TextGenerationLimits {
    TextGenerationLimits {
        max_tokens: Some(if language_looks_cjk(language) {
            900
        } else {
            1100
        }),
        target_chars: Some(if language_looks_cjk(language) {
            700
        } else {
            950
        }),
        hard_max_chars: Some(if language_looks_cjk(language) {
            1800
        } else {
            2400
        }),
    }
}

pub(super) fn metadata_repair_prompt(
    language: &str,
    chapter_number: usize,
    draft: &novel_runner::DraftOutput,
    issues: &str,
) -> String {
    let body_preview = preview_text(&draft.content, 6500);
    if language_looks_cjk(language) {
        return format!(
            "只修复第 {chapter_number} 章的元数据，不要重写正文。\n\n\
             当前标题：{}\n\
             当前摘要：{}\n\
             当前 key_facts：{}\n\
             当前 continuity_updates：{}\n\n\
             元数据问题：\n{issues}\n\n\
             正文：\n{body_preview}\n\n\
             输出 JSON，字段必须是：title, summary, key_facts, continuity_updates。\n\
             要求：只输出一个 JSON 对象，不要 Markdown 或正文协议。title 只写标题核心，不得包含“第N章”、Chapter、书名号、卷名或序号，不得原样复用当前被拒标题；必须根据正文中已经完成的独特事件、关键物件、地点、选择或不可逆变化重新命名，不能直接截取正文长句的一小段。summary、key_facts、continuity_updates 必须被正文支撑；不要输出 content；不要改写正文；所有创作字段使用中文。",
            draft.title,
            draft.summary,
            draft.key_facts.join("；"),
            draft.continuity_updates.join("；")
        );
    }
    format!(
        "Repair only chapter {chapter_number} metadata. Do not rewrite prose.\n\n\
         Current title: {}\n\
         Current summary: {}\n\
         Current key_facts: {}\n\
         Current continuity_updates: {}\n\n\
         Metadata issues:\n{issues}\n\n\
         Body:\n{body_preview}\n\n\
         Return exactly one JSON object with fields: title, summary, key_facts, continuity_updates; no Markdown or body protocol. \
         The title must be the title core only, without a chapter number, book/volume label, or structural prefix, and must not repeat the rejected current title. Derive it from a completed unique event, object, place, choice, or irreversible change instead of clipping a prose sentence. Metadata must be supported by the body. Do not return content.",
        draft.title,
        draft.summary,
        draft.key_facts.join("; "),
        draft.continuity_updates.join("; ")
    )
}

pub(super) fn parse_metadata_repair_output(
    raw: &str,
    chapter_number: usize,
    language: &str,
    fallback: &novel_runner::DraftOutput,
) -> novel_runner::DraftOutput {
    let value = novel_runner::extract_json(raw)
        .and_then(|json| serde_json::from_str::<Value>(&json).ok())
        .filter(Value::is_object);
    let title = value
        .as_ref()
        .and_then(|value| value.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            novel_runner::jsonish_string_field(
                raw,
                "title",
                &["summary", "key_facts", "continuity_updates"],
            )
        })
        .unwrap_or_else(|| fallback.title.clone());
    let summary = value
        .as_ref()
        .and_then(|value| value.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            novel_runner::jsonish_string_field(raw, "summary", &["key_facts", "continuity_updates"])
        })
        .unwrap_or_else(|| fallback.summary.clone());
    let key_facts = value
        .as_ref()
        .map(|value| json_string_array(value.get("key_facts")))
        .filter(|items| !items.is_empty())
        .or_else(|| {
            let items = novel_runner::jsonish_string_array_field(raw, "key_facts");
            (!items.is_empty()).then_some(items)
        })
        .unwrap_or_else(|| fallback.key_facts.clone());
    let continuity_updates = value
        .as_ref()
        .map(|value| json_string_array(value.get("continuity_updates")))
        .filter(|items| !items.is_empty())
        .or_else(|| {
            let items = novel_runner::jsonish_string_array_field(raw, "continuity_updates");
            (!items.is_empty()).then_some(items)
        })
        .unwrap_or_else(|| fallback.continuity_updates.clone());
    let title = if title.trim().is_empty() {
        if language_looks_cjk(language) {
            format!("第{chapter_number}章")
        } else {
            format!("Chapter {chapter_number}")
        }
    } else {
        title
    };
    novel_runner::DraftOutput {
        title,
        content: fallback.content.clone(),
        summary,
        key_facts,
        continuity_updates,
        degraded: fallback.degraded,
        degraded_reason: fallback.degraded_reason.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_repair_reuses_shared_jsonish_parser_for_truncated_object() {
        let fallback = novel_runner::DraftOutput {
            title: "第1章".to_string(),
            content: "闻望宁收起蓝色胶囊。".to_string(),
            summary: "旧摘要".to_string(),
            key_facts: vec!["旧事实".to_string()],
            continuity_updates: vec!["旧连续性".to_string()],
            degraded: false,
            degraded_reason: String::new(),
        };
        let raw = r#"{"title":"蓝色胶囊","summary":"闻望宁收起蓝色胶囊","key_facts":["闻望宁获得蓝色胶囊"],"continuity_updates":["胶囊仍由闻望宁持有"]"#;

        let repaired = parse_metadata_repair_output(raw, 1, "zh-CN", &fallback);

        assert_eq!(repaired.title, "蓝色胶囊");
        assert_eq!(repaired.summary, "闻望宁收起蓝色胶囊");
        assert_eq!(repaired.key_facts, vec!["闻望宁获得蓝色胶囊"]);
        assert_eq!(repaired.continuity_updates, vec!["胶囊仍由闻望宁持有"]);
    }

    #[test]
    fn metadata_repair_budget_is_bounded_but_allows_recovery_after_one_bad_candidate() {
        assert_eq!(MAX_METADATA_REPAIR_ATTEMPTS, 5);
    }
}
