use super::*;

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
    let has_typed_findings = ["/quality_gate/findings", "/review/findings", "/findings"]
        .into_iter()
        .any(|pointer| value.pointer(pointer).is_some());
    let has_legacy_issues = value.get("issues").and_then(Value::as_array).is_some()
        || value
            .pointer("/review/issues")
            .and_then(Value::as_array)
            .is_some();
    let legacy_untyped_review = !locally_validated && !has_typed_findings && has_legacy_issues;
    !value_has_hard_findings(value)
        && ((verdict_passed && locally_validated) || legacy_untyped_review)
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
    ["/quality_gate/findings", "/review/findings", "/findings"]
        .into_iter()
        .filter_map(|pointer| value.pointer(pointer))
        .filter_map(Value::as_array)
        .flatten()
        .any(|finding| {
            finding
                .get("disposition")
                .and_then(Value::as_str)
                .is_some_and(|disposition| disposition == "hard_block")
        })
}

pub(in crate::tool::writing::novel_workflow_driver) fn finding_codes_with_disposition(
    value: &Value,
    disposition: &str,
) -> BTreeSet<String> {
    ["/quality_gate/findings", "/review/findings", "/findings"]
        .into_iter()
        .filter_map(|pointer| value.pointer(pointer))
        .filter_map(Value::as_array)
        .flatten()
        .filter(|finding| {
            finding
                .get("disposition")
                .and_then(Value::as_str)
                .is_some_and(|value| value == disposition)
        })
        .filter_map(|finding| finding.get("code").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

pub(in crate::tool::writing::novel_workflow_driver) fn value_has_non_metadata_deterministic_repairs(
    value: &Value,
) -> bool {
    ["/quality_gate/findings", "/review/findings", "/findings"]
        .into_iter()
        .filter_map(|pointer| value.pointer(pointer))
        .filter_map(Value::as_array)
        .flatten()
        .any(|finding| {
            finding
                .get("disposition")
                .and_then(Value::as_str)
                .is_some_and(|disposition| disposition == "deterministic_repair")
                && !finding
                    .get("class")
                    .and_then(Value::as_str)
                    .is_some_and(|class| class == "metadata")
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
