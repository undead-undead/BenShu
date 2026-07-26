use std::path::Path;

use serde_json::{json, Value};

pub(crate) fn recoverable_path_error_result(
    error: &anyhow::Error,
    tool_name: &str,
    action: &str,
    attempted_path: &str,
    workspace: &Path,
    safe_output_root: &str,
) -> Option<Value> {
    let message = error.to_string();
    let lowered = message.to_ascii_lowercase();
    let error_kind = if lowered.contains("access denied")
        && lowered.contains("outside authorized workspaces")
    {
        "path_outside_workspace"
    } else if lowered.contains("path traversal is not allowed") {
        "path_traversal"
    } else if lowered.contains("path is empty") || lowered.contains("project_path is required") {
        "missing_project_path"
    } else if lowered.contains("not a directory") {
        "path_not_directory"
    } else {
        return None;
    };

    Some(json!({
        "success": false,
        "recoverable": true,
        "tool": tool_name,
        "action": action,
        "error_kind": error_kind,
        "error": message,
        "attempted_path": attempted_path.trim(),
        "authorized_workspace": workspace.to_string_lossy(),
        "safe_output_root": safe_output_root,
        "next_step_hint": "Retry with a relative project_path under safe_output_root, or reuse the project_path returned by the last successful init/list/status call. Do not invent absolute host paths."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_only_known_recoverable_path_errors() {
        let workspace = Path::new("/workspace");
        let value = recoverable_path_error_result(
            &anyhow::anyhow!("project_path is required for status"),
            "novel_studio",
            "status",
            "",
            workspace,
            "data/generated/novels",
        )
        .expect("recoverable path error");
        assert_eq!(value["error_kind"], "missing_project_path");

        assert!(recoverable_path_error_result(
            &anyhow::anyhow!("manifest schema is invalid"),
            "novel_studio",
            "status",
            "demo",
            workspace,
            "data/generated/novels",
        )
        .is_none());
    }
}
