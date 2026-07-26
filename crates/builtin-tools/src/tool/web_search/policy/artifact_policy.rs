use serde_json::{Map, Value};
use std::collections::HashMap;

pub fn parse_artifact_policy_yaml(yaml: &str) -> Result<Option<Value>, String> {
    let trimmed = yaml.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let value = serde_yaml_ng::from_str::<Value>(trimmed)
        .map_err(|err| format!("Invalid artifact policy YAML: {err}"))?;
    Ok(normalize_artifact_policy_value(value))
}

pub fn artifact_policy_to_yaml(policy: &Value) -> String {
    serde_yaml_ng::to_string(policy)
        .map(|yaml| yaml.trim_end().to_string())
        .unwrap_or_default()
}

pub fn normalize_artifact_policy_value(value: Value) -> Option<Value> {
    let mut policy = if value.is_null() {
        return None;
    } else if value.get("handles").is_some() {
        value
    } else {
        value.get("artifact_policy").cloned()?
    };

    if policy.is_null() {
        return None;
    }
    compile_artifact_policy_terms(&mut policy);
    Some(policy)
}

pub fn artifact_policy_capabilities(policy: &Option<Value>, limit: usize) -> Vec<String> {
    let mut capabilities = Vec::new();
    let Some(policy) = policy else {
        return capabilities;
    };
    let Some(handles) = policy.get("handles").and_then(Value::as_array) else {
        return capabilities;
    };

    for handle in handles {
        if let Some(artifact) = handle.get("artifact").and_then(Value::as_str) {
            push_unique(&mut capabilities, artifact.to_string());
        }
        if capabilities.len() >= limit {
            break;
        }
    }
    capabilities
}

pub fn artifact_policy_match_score(
    artifact_policy: &Option<Value>,
    requested_role: &str,
    task: &str,
) -> i32 {
    let Some(policy) = artifact_policy else {
        return 0;
    };

    let mut strings = Vec::new();
    collect_policy_strings(policy, &mut strings);
    let requested_role = normalize_label(requested_role);
    let task = normalize_label(task);
    let mut score = 0;
    for value in strings {
        let normalized = normalize_label(&value);
        if normalized.len() < 3 {
            continue;
        }
        if requested_role == normalized {
            score += 5_000;
        } else if requested_role.contains(&normalized) || normalized.contains(&requested_role) {
            score += 2_500;
        }
        if !task.is_empty() && task.contains(&normalized) {
            score += 1_200;
        }
    }
    score.min(8_000)
}

pub fn collect_policy_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if !text.trim().is_empty() {
                out.push(text.trim().to_string());
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_policy_strings(value, out);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                collect_policy_strings(value, out);
            }
        }
        _ => {}
    }
}

pub fn policy_handle_matches_task(handle: &Value, task: &str) -> bool {
    let lowered = task.to_ascii_lowercase();
    for key in ["triggers", "keywords", "aliases", "intents", "artifact"] {
        if policy_value_matches_task(handle.get(key), task, &lowered) {
            return true;
        }
    }
    false
}

pub fn policy_value_matches_task(value: Option<&Value>, original: &str, lowered: &str) -> bool {
    match value {
        Some(Value::String(text)) => policy_text_matches_task(text, original, lowered),
        Some(Value::Array(values)) => values
            .iter()
            .any(|value| policy_value_matches_task(Some(value), original, lowered)),
        _ => false,
    }
}

pub fn policy_text_matches_task(text: &str, original: &str, lowered: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 2 {
        return false;
    }
    if trimmed.chars().any(|ch| !ch.is_ascii()) {
        original.contains(trimmed)
    } else {
        lowered.contains(&trimmed.to_ascii_lowercase())
    }
}

pub fn push_policy_string_field(handle: &Value, key: &str, target: &mut Vec<String>) {
    if let Some(value) = handle.get(key).and_then(Value::as_str) {
        push_unique(target, value.to_string());
    }
}

pub fn push_policy_string_array(handle: &Value, key: &str, target: &mut Vec<String>) {
    let Some(values) = handle.get(key) else {
        return;
    };
    match values {
        Value::String(value) => push_unique(target, value.to_string()),
        Value::Array(values) => {
            for value in values {
                if let Some(value) = value.as_str() {
                    push_unique(target, value.to_string());
                }
            }
        }
        _ => {}
    }
}

pub fn string_items(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub fn push_unique(target: &mut Vec<String>, value: String) {
    if !target
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&value))
    {
        target.push(value);
    }
}

pub fn normalize_label(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch) {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn compile_artifact_policy_terms(policy: &mut Value) {
    let terms = artifact_policy_terms(policy);
    if terms.is_empty() {
        return;
    }
    compile_term_references(policy, &terms, false);
}

fn artifact_policy_terms(policy: &Value) -> HashMap<String, Vec<String>> {
    let mut terms = HashMap::new();
    let Some(object) = policy.get("terms").and_then(Value::as_object) else {
        return terms;
    };
    for (name, value) in object {
        let aliases = if let Some(items) = value.as_array() {
            string_items(items)
        } else if let Some(items) = value.get("aliases").and_then(Value::as_array) {
            string_items(items)
        } else if let Some(items) = value.get("terms").and_then(Value::as_array) {
            string_items(items)
        } else {
            Vec::new()
        };
        if !aliases.is_empty() {
            terms.insert(name.to_ascii_lowercase(), aliases);
        }
    }
    terms
}

fn compile_term_references(
    value: &mut Value,
    terms: &HashMap<String, Vec<String>>,
    inside_terms: bool,
) {
    match value {
        Value::Object(object) => {
            let now_inside_terms = inside_terms || object.contains_key("aliases");
            if !now_inside_terms {
                compile_object_term_references(object, terms);
            }
            for (key, child) in object.iter_mut() {
                compile_term_references(child, terms, inside_terms || key == "terms");
            }
        }
        Value::Array(items) => {
            for item in items {
                compile_term_references(item, terms, inside_terms);
            }
        }
        _ => {}
    }
}

fn compile_object_term_references(
    object: &mut Map<String, Value>,
    terms: &HashMap<String, Vec<String>>,
) {
    let keys = object.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let Some(target_key) = key.strip_suffix("_from").map(ToString::to_string) else {
            continue;
        };
        let Some(refs) = object.remove(&key) else {
            continue;
        };
        let aliases = expand_term_refs(&refs, terms);
        if aliases.is_empty() {
            continue;
        }
        let entry = object
            .entry(target_key)
            .or_insert_with(|| Value::Array(Vec::new()));
        if !entry.is_array() {
            *entry = Value::Array(Vec::new());
        }
        let items = entry.as_array_mut().expect("array checked");
        for alias in aliases {
            if !items
                .iter()
                .filter_map(Value::as_str)
                .any(|existing| existing.eq_ignore_ascii_case(&alias))
            {
                items.push(Value::String(alias));
            }
        }
    }
}

fn expand_term_refs(refs: &Value, terms: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut expanded = Vec::new();
    for reference in term_ref_names(refs) {
        if let Some(aliases) = terms.get(&reference.to_ascii_lowercase()) {
            for alias in aliases {
                push_unique(&mut expanded, alias.clone());
            }
        } else {
            push_unique(&mut expanded, reference);
        }
    }
    expanded
}

fn term_ref_names(value: &Value) -> Vec<String> {
    match value {
        Value::String(value) => vec![value.trim().to_string()],
        Value::Array(items) => items
            .iter()
            .flat_map(term_ref_names)
            .filter(|item| !item.is_empty())
            .collect(),
        Value::Object(object) => object
            .get("term")
            .or_else(|| object.get("ref"))
            .map(term_ref_names)
            .or_else(|| object.get("terms").map(term_ref_names))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_policy_yaml_accepts_wrapped_documents_and_terms() {
        let policy = parse_artifact_policy_yaml(
            "artifact_policy:\n  terms:\n    paper:\n      aliases: [论文, paper]\n  handles:\n    - artifact: academic_paper\n      triggers_from: [paper]\n",
        )
        .expect("parse")
        .expect("policy");

        assert!(policy.get("artifact_policy").is_none());
        let handle = &policy["handles"][0];
        assert!(policy_handle_matches_task(handle, "查找论文"));
        assert!(policy_handle_matches_task(handle, "find paper"));
    }
}
