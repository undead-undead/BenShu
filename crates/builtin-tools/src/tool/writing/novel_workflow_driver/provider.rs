use super::*;

pub(super) fn local_tool_stage_timeout_secs() -> u64 {
    std::env::var("BENSHU_NOVEL_LOCAL_TOOL_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (3..=120).contains(value))
        .unwrap_or(15)
}

pub(super) async fn call_novel_studio_json(
    tool: &NovelStudioTool,
    arguments: Value,
) -> anyhow::Result<Value> {
    let output = tool.call(&arguments.to_string()).await?;
    let value: Value = serde_json::from_str(&output)?;
    if value
        .get("success")
        .and_then(Value::as_bool)
        .is_some_and(|success| !success)
    {
        anyhow::bail!("novel_studio returned unsuccessful result: {}", output);
    }
    Ok(value)
}

pub(super) async fn call_novel_studio_json_raw(
    tool: &NovelStudioTool,
    arguments: Value,
) -> anyhow::Result<Value> {
    let output = tool.call(&arguments.to_string()).await?;
    Ok(serde_json::from_str(&output)?)
}

pub(super) async fn call_novel_studio_json_with_timeout(
    tool: &NovelStudioTool,
    arguments: Value,
    timeout_secs: u64,
    stage: &str,
) -> anyhow::Result<Value> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        call_novel_studio_json(tool, arguments),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => anyhow::bail!(
            "novel_studio stage `{}` exceeded {}s local tool budget",
            stage,
            timeout_secs
        ),
    }
}

pub(super) async fn call_novel_studio_json_raw_with_timeout(
    tool: &NovelStudioTool,
    arguments: Value,
    timeout_secs: u64,
    stage: &str,
) -> anyhow::Result<Value> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        call_novel_studio_json_raw(tool, arguments),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => anyhow::bail!(
            "novel_studio stage `{}` exceeded {}s local tool budget",
            stage,
            timeout_secs
        ),
    }
}

pub(super) async fn project_approved_target_reached(
    tool: &NovelStudioTool,
    project_path: &str,
) -> anyhow::Result<bool> {
    let status = call_novel_studio_json_with_timeout(
        tool,
        json!({
            "action": "status",
            "project_path": project_path
        }),
        local_tool_stage_timeout_secs(),
        "project_approved_target_reached_status",
    )
    .await?;
    Ok(project_approved_target_reached_from_status_packet(&status))
}

pub(super) fn project_approved_target_reached_from_status_packet(status: &Value) -> bool {
    let state = status.get("state").cloned().unwrap_or_else(|| json!({}));
    state_target_reached_by_approved_units(&state)
        && state_usize(&state, "first_unapproved_chapter").is_none()
}

pub(super) fn state_target_reached_by_approved_units(state: &Value) -> bool {
    let Some(target) = state_usize(state, "target_units").filter(|value| *value > 0) else {
        return false;
    };
    state_usize(state, "approved_units")
        .map(|approved| approved >= target)
        .unwrap_or(false)
}
