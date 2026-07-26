#[cfg(test)]
pub(super) fn user_facing_task_brief(task: &str) -> String {
    if task.contains("[BENSHU_DIRECT_WRITER_CONTINUATION]") {
        let mut parts = Vec::new();
        for line in task.lines().map(str::trim) {
            for prefix in [
                "题材/方向：",
                "简述：",
                "总目标字数：",
                "每章目标字数档位：",
                "导出格式：",
                "用户最新要求：",
            ] {
                if let Some(value) = line.strip_prefix(prefix).map(str::trim) {
                    if !value.is_empty() {
                        parts.push(format!("{prefix}{value}"));
                    }
                }
            }
        }
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }
    let marker = "Original user request:";
    if let Some(rest) = task.split(marker).nth(1) {
        let before_delegated = rest.split("Delegated task:").next().unwrap_or(rest).trim();
        if !before_delegated.is_empty() {
            return before_delegated.to_string();
        }
    }
    task.trim().to_string()
}
