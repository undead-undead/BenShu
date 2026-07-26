use super::*;

#[cfg(test)]
pub(super) fn novel_studio_error_is(value: &Value, expected: &str) -> bool {
    value
        .get("success")
        .and_then(Value::as_bool)
        .is_some_and(|success| !success)
        && value
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error == expected)
}

pub(super) fn required_string<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("novel_studio result missing `{}`", key))
}

pub(super) fn state_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn state_usize(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

pub(super) fn context_payload(packet: &Value) -> &Value {
    packet
        .get("execution_authority_context")
        .or_else(|| packet.get("prompt_context"))
        .or_else(|| packet.get("context"))
        .or_else(|| packet.pointer("/assigned_worker_policy_packet/context"))
        .unwrap_or(packet)
}

pub(super) fn compact_context_json(packet: &Value) -> anyhow::Result<String> {
    Ok(serde_json::to_string(context_payload(packet))?)
}

pub(super) fn context_project_title(packet: &Value) -> Option<String> {
    packet
        .pointer("/context/project/title")
        .or_else(|| packet.pointer("/prompt_context/project/title"))
        .or_else(|| packet.pointer("/prompt_context/authority/working_context/project/title"))
        .or_else(|| packet.pointer("/authority/working_context/project/title"))
        .or_else(|| packet.pointer("/assigned_worker_policy_packet/context/project/title"))
        .or_else(|| packet.pointer("/state/title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn project_chapter_numbers(project_path: &str) -> anyhow::Result<Vec<usize>> {
    let manifest_path = Path::new(project_path).join("project.json");
    let raw = fs::read_to_string(&manifest_path)?;
    let value: Value = serde_json::from_str(&raw)?;
    let mut numbers = value
        .get("chapters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|chapter| {
            chapter
                .get("number")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
        })
        .filter(|value| *value > 0)
        .collect::<Vec<_>>();
    numbers.sort_unstable();
    numbers.dedup();
    Ok(numbers)
}

#[cfg(test)]
pub(super) fn read_chapter_result_is_approved(read: &Value) -> bool {
    if !read
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return false;
    }
    let status = read
        .pointer("/chapter/status")
        .and_then(Value::as_str)
        .unwrap_or("");
    chapter_lifecycle::status_is_approved(status)
}

pub(super) fn project_chapter_record(
    project_path: &str,
    chapter_number: usize,
) -> anyhow::Result<Option<Value>> {
    let manifest_path = Path::new(project_path).join("project.json");
    let raw = fs::read_to_string(&manifest_path)?;
    let manifest: Value = serde_json::from_str(&raw)?;
    Ok(manifest
        .get("chapters")
        .and_then(Value::as_array)
        .and_then(|chapters| {
            chapters
                .iter()
                .find(|chapter| {
                    chapter
                        .get("number")
                        .and_then(Value::as_u64)
                        .is_some_and(|number| number as usize == chapter_number)
                })
                .cloned()
        }))
}

pub(super) fn chapter_record_value_is_approved(chapter: &Value) -> bool {
    let status = chapter.get("status").and_then(Value::as_str).unwrap_or("");
    chapter_lifecycle::status_is_approved(status)
}
