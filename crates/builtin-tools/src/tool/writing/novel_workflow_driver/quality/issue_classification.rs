use super::*;

pub(in crate::tool::writing::novel_workflow_driver) fn typed_findings_in_value(
    value: &Value,
) -> Vec<chapter_quality::ChapterFinding> {
    [
        "/quality_gate/findings",
        "/metadata_gate/findings",
        "/review/findings",
        "/findings",
    ]
    .into_iter()
    .filter_map(|pointer| value.pointer(pointer))
    .filter_map(Value::as_array)
    .flatten()
    .filter_map(|finding| {
        serde_json::from_value::<chapter_quality::ChapterFinding>(finding.clone()).ok()
    })
    .collect()
}

pub(in crate::tool::writing::novel_workflow_driver) fn needs_revision(value: &Value) -> bool {
    value_has_hard_findings(value)
        || !json_array_is_empty(value.pointer("/truth_validation/issues"))
}

pub(in crate::tool::writing::novel_workflow_driver) fn audit_passed(value: &Value) -> bool {
    let verdict_passed = value
        .pointer("/review/verdict")
        .and_then(Value::as_str)
        .or_else(|| value.get("verdict").and_then(Value::as_str))
        .is_some_and(|verdict| verdict.eq_ignore_ascii_case("passed"));
    let locally_validated = value
        .pointer("/review/locally_validated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    !value_has_hard_findings(value) && verdict_passed && locally_validated
}

pub(in crate::tool::writing::novel_workflow_driver) fn audit_next_action_blocked(
    value: &Value,
) -> bool {
    let blocked = value
        .get("next_action")
        .and_then(Value::as_str)
        .is_some_and(|action| action.eq_ignore_ascii_case("blocked"))
        || value
            .pointer("/review_cycle/next_action")
            .and_then(Value::as_str)
            .is_some_and(|action| action.eq_ignore_ascii_case("blocked"));
    blocked && value_has_hard_findings(value)
}

pub(in crate::tool::writing::novel_workflow_driver) fn value_has_hard_findings(
    value: &Value,
) -> bool {
    typed_findings_in_value(value)
        .iter()
        .any(chapter_quality::ChapterFinding::hard_blocking)
}

pub(in crate::tool::writing::novel_workflow_driver) fn finding_codes_with_disposition(
    value: &Value,
    disposition: chapter_quality::ChapterFindingDisposition,
) -> BTreeSet<String> {
    typed_findings_in_value(value)
        .into_iter()
        .filter(|finding| match disposition {
            chapter_quality::ChapterFindingDisposition::HardBlock => finding.hard_blocking(),
            expected => finding.disposition == expected,
        })
        .map(|finding| finding.code)
        .collect()
}

pub(in crate::tool::writing::novel_workflow_driver) fn value_has_local_cleanup_repairs(
    value: &Value,
) -> bool {
    typed_findings_in_value(value).into_iter().any(|finding| {
        finding.disposition == chapter_quality::ChapterFindingDisposition::DeterministicRepair
            && finding.class != chapter_quality::ChapterFindingClass::Metadata
            && finding.code != "length_below_target"
    })
}

pub(in crate::tool::writing::novel_workflow_driver) fn write_result_is_clean_for_rule_audit(
    value: &Value,
) -> bool {
    value
        .pointer("/quality_gate/passed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && json_array_is_empty(value.pointer("/quality_gate/repairable"))
        && !value_has_hard_findings(value)
        && json_array_is_empty(value.pointer("/truth_validation/issues"))
}
