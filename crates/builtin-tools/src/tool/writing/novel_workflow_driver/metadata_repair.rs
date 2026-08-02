use super::*;

pub(super) const MAX_METADATA_REPAIR_ATTEMPTS: usize = 5;

pub(super) fn metadata_gate_needs_repair(write_result: &Value) -> bool {
    if write_result
        .get("metadata_fallback_applied")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return false;
    }
    metadata_gate_blocks(write_result)
        || !json_array_is_empty(write_result.pointer("/metadata_gate/repairable"))
        || value_has_metadata_repair_findings(write_result)
        || !json_array_is_empty(write_result.pointer("/truth_validation/issues"))
}

pub(super) fn metadata_gate_blocks(write_result: &Value) -> bool {
    !json_array_is_empty(write_result.pointer("/metadata_gate/blocking"))
}

pub(super) fn metadata_gate_has_repairable(write_result: &Value) -> bool {
    if write_result
        .get("metadata_fallback_applied")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return false;
    }
    !json_array_is_empty(write_result.pointer("/metadata_gate/repairable"))
        || value_has_metadata_repair_findings(write_result)
}

/// Reuse the typed finding classification emitted by the studio quality gate.
/// Metadata findings may be carried in `quality_gate.findings` (for example when
/// a title was normalized after body write), so the workflow must not route them
/// into a body revision merely because `/metadata_gate` is empty.
pub(super) fn value_has_metadata_repair_findings(value: &Value) -> bool {
    typed_findings_in_value(value).into_iter().any(|finding| {
        finding.class == chapter_quality::ChapterFindingClass::Metadata
            && finding.disposition
                == chapter_quality::ChapterFindingDisposition::DeterministicRepair
    })
}

/// A typed hard metadata finding is a concrete invariant failure, not a
/// candidate for prose revision.  Keep this classification beside the
/// existing metadata-repair predicate so the loop has one source of truth for
/// the distinction.
pub(super) fn value_has_hard_metadata_findings(value: &Value) -> bool {
    typed_findings_in_value(value).into_iter().any(|finding| {
        finding.class == chapter_quality::ChapterFindingClass::Metadata && finding.hard_blocking()
    })
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
    issues.extend(
        typed_findings_in_value(write_result)
            .into_iter()
            .filter(|finding| finding.class == chapter_quality::ChapterFindingClass::Metadata)
            .map(|finding| finding.message),
    );
    issues.sort();
    issues.dedup();
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
    rejected_titles: &[String],
) -> String {
    let body_preview = preview_text(&draft.content, 6500);
    let rejected_titles = rejected_titles
        .iter()
        .map(|title| title.trim())
        .filter(|title| !title.is_empty())
        .collect::<Vec<_>>()
        .join("；");
    if language_looks_cjk(language) {
        return format!(
            "只修复第 {chapter_number} 章的元数据，不要重写正文。\n\n\
             当前标题：{}\n\
             当前摘要：{}\n\
             当前 key_facts：{}\n\
             当前 continuity_updates：{}\n\n\
             本轮及此前已拒标题（全部禁用）：{}\n\n\
             元数据问题：\n{issues}\n\n\
             正文：\n{body_preview}\n\n\
             输出 JSON，字段必须是：title_candidates, title, summary, key_facts, continuity_updates。\n\
             要求：只输出一个 JSON 对象，不要 Markdown 或正文协议。title_candidates 必须给出 3 个彼此不同的标题核心；title 等于其中首选项。所有候选都不得包含“第N章”、Chapter、书名号、卷名或序号，不得复用任何已拒标题；必须分别根据正文中已经完成的独特事件、关键物件、地点、选择或不可逆变化命名，不能直接截取正文长句的一小段。summary、key_facts、continuity_updates 必须被正文支撑；不要输出 content；不要改写正文；所有创作字段使用中文。",
            draft.title,
            draft.summary,
            draft.key_facts.join("；"),
            draft.continuity_updates.join("；"),
            rejected_titles
        );
    }
    format!(
        "Repair only chapter {chapter_number} metadata. Do not rewrite prose.\n\n\
         Current title: {}\n\
         Current summary: {}\n\
         Current key_facts: {}\n\
         Current continuity_updates: {}\n\n\
         Rejected titles from this repair cycle (all forbidden): {}\n\n\
         Metadata issues:\n{issues}\n\n\
         Body:\n{body_preview}\n\n\
         Return exactly one JSON object with fields: title_candidates, title, summary, key_facts, continuity_updates; no Markdown or body protocol. \
         title_candidates must contain three distinct title cores and title must be the preferred first item. No candidate may contain a chapter number, book/volume label, structural prefix, or any rejected title. Derive each from a different completed unique event, object, place, choice, or irreversible change instead of clipping a prose sentence. Metadata must be supported by the body. Do not return content.",
        draft.title,
        draft.summary,
        draft.key_facts.join("; "),
        draft.continuity_updates.join("; "),
        rejected_titles
    )
}

pub(super) fn metadata_repair_title_candidates(
    raw: &str,
    fallback_title: &str,
    rejected_titles: &[String],
) -> Vec<String> {
    let value = novel_runner::extract_json(raw)
        .and_then(|json| serde_json::from_str::<Value>(&json).ok())
        .filter(Value::is_object);
    let rejected = rejected_titles
        .iter()
        .map(|title| title.trim())
        .filter(|title| !title.is_empty())
        .collect::<BTreeSet<_>>();
    let candidates = value
        .as_ref()
        .and_then(|value| value.get("title_candidates"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .chain(
            value
                .as_ref()
                .and_then(|value| value.get("title"))
                .and_then(Value::as_str),
        )
        .map(str::trim)
        .filter(|title| !title.is_empty() && !rejected.contains(title))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    let mut candidates = candidates
        .into_iter()
        .filter(|title| seen.insert(title.clone()))
        .collect::<Vec<_>>();
    let fallback_title = fallback_title.trim();
    if candidates.is_empty() && !fallback_title.is_empty() && !rejected.contains(fallback_title) {
        candidates.push(fallback_title.to_string());
    }
    candidates
}

pub(super) fn metadata_title_issue_count(write_result: &Value) -> usize {
    write_result
        .pointer("/metadata_gate/findings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|finding| {
            finding
                .get("source")
                .and_then(Value::as_str)
                .is_some_and(|source| source.starts_with("chapter_title"))
                || finding
                    .get("message")
                    .and_then(Value::as_str)
                    .is_some_and(|message| {
                        message.contains("chapter title") || message.contains("章节标题")
                    })
        })
        .count()
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

    #[test]
    fn metadata_repair_keeps_distinct_non_rejected_title_candidates() {
        let raw =
            r#"{"title_candidates":["受力点的黎明","弧线落地","受力点的黎明"],"title":"旧标题"}"#;
        let candidates = metadata_repair_title_candidates(
            raw,
            "第4章",
            &["旧标题".to_string(), "第4章".to_string()],
        );

        assert_eq!(candidates, ["受力点的黎明", "弧线落地"]);
    }

    #[test]
    fn metadata_repair_never_restores_a_rejected_fallback_title() {
        let candidates = metadata_repair_title_candidates(
            r#"{"title_candidates":[],"title":"旧标题"}"#,
            "旧标题",
            &["旧标题".to_string()],
        );

        assert!(candidates.is_empty());
    }

    #[test]
    fn quality_gate_metadata_findings_route_to_existing_metadata_repair() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": false,
                "findings": [{
                    "code": "metadata_invalid",
                    "class": "metadata",
                    "disposition": "deterministic_repair",
                    "evidence_grade": "deterministic_invariant",
                    "source": "chapter_title",
                    "message": "chapter title is still the default chapter heading",
                    "authority_evidence": [],
                    "body_evidence": [],
                    "authority_fingerprint": "authority",
                    "body_fingerprint": "body"
                }]
            },
            "metadata_gate": {"blocking": [], "repairable": []},
            "truth_validation": {"issues": []}
        });

        assert!(value_has_metadata_repair_findings(&write_result));
        assert!(metadata_gate_needs_repair(&write_result));
        assert!(metadata_gate_has_repairable(&write_result));
        assert!(metadata_issue_summary(&write_result).contains("default chapter heading"));
    }

    #[test]
    fn hard_metadata_findings_are_not_marked_as_repairable() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": false,
                "findings": [{
                    "code": "metadata_contract_conflict",
                    "class": "metadata",
                    "disposition": "hard_block",
                    "evidence_grade": "deterministic_invariant",
                    "source": "chapter_title",
                    "message": "chapter title conflicts with the sealed contract",
                    "authority_evidence": [],
                    "body_evidence": [],
                    "authority_fingerprint": "authority",
                    "body_fingerprint": "body"
                }]
            },
            "metadata_gate": {"blocking": [], "repairable": []},
            "truth_validation": {"issues": []}
        });

        assert!(!value_has_metadata_repair_findings(&write_result));
        assert!(!metadata_gate_needs_repair(&write_result));
        assert!(!metadata_gate_has_repairable(&write_result));
        assert!(value_has_hard_metadata_findings(&write_result));
    }
}
