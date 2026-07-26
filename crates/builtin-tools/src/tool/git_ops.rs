//! Git operations tool — version control automation for agents.
//!
//! Provides Git and GitHub REST API integration:
//! - Repository info, search, stars/forks
//! - Pull request management (list, create, merge)
//! - Issue operations (list, create, comment)
//! - Code search
//! - Local git commands (status, diff, log, commit)

use async_trait::async_trait;
use benshu_compression::compress_command_output;
use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use tokio::time::{timeout, Duration};

pub struct GitOpsTool;

const MAX_OUTPUT_CHARS: usize = 8192;
const GIT_TIMEOUT_SECS: u64 = 30;

#[derive(Deserialize)]
struct GitOpsArgs {
    action: String,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    head: String,
    #[serde(default)]
    base: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    number: Option<u64>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    message: String,
}

#[async_trait]
impl Tool for GitOpsTool {
    fn name(&self) -> String {
        "git_ops".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "git_ops".to_string(),
            description: "Git and GitHub operations — manage repos, PRs, issues, and local git commands".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["repo_info", "list_prs", "create_pr", "merge_pr", "close_pr", 
                                 "list_issues", "create_issue", "add_comment", "close_issue",
                                 "search_code", "local_status", "local_diff", "local_log", 
                                 "local_commit", "local_push", "local_pull", "local_fetch", "info"],
                        "description": "Git operation to perform"
                    },
                    "owner": { "type": "string", "description": "Repository owner/organization" },
                    "repo": { "type": "string", "description": "Repository name" },
                    "token": { "type": "string", "description": "GitHub agentl access token (overrides env GITHUB_TOKEN)" },
                    "title": { "type": "string", "description": "PR/Issue title" },
                    "body": { "type": "string", "description": "PR/Issue body" },
                    "head": { "type": "string", "description": "PR head branch" },
                    "base": { "type": "string", "description": "PR base branch (default: main)" },
                    "query": { "type": "string", "description": "Search query" },
                    "number": { "type": "integer", "description": "PR/Issue number" },
                    "path": { "type": "string", "description": "Working directory for local git commands" },
                    "message": { "type": "string", "description": "Commit message" }
                },
                "required": ["action"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use for GitHub API operations and local git commands. Requires GITHUB_TOKEN env var or token param for API calls.".into()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args = parse_git_ops_args(arguments).map_err(|e| Error::ToolArguments {
            tool_name: "git_ops".into(),
            message: e.to_string(),
        })?;

        let result = match args.action.as_str() {
            "info" => detect_capabilities().await,
            "repo_info" => {
                github_api_get(&args, &format!("repos/{}/{}", args.owner, args.repo)).await
            }
            "list_prs" => {
                github_api_get(&args, &format!("repos/{}/{}/pulls", args.owner, args.repo)).await
            }
            "create_pr" => {
                let body = json!({
                    "title": args.title,
                    "body": args.body,
                    "head": args.head,
                    "base": if args.base.is_empty() { "main" } else { &args.base },
                });
                github_api_post(
                    &args,
                    &format!("repos/{}/{}/pulls", args.owner, args.repo),
                    &body,
                )
                .await
            }
            "list_issues" => {
                github_api_get(&args, &format!("repos/{}/{}/issues", args.owner, args.repo)).await
            }
            "create_issue" => {
                let body = json!({ "title": args.title, "body": args.body });
                github_api_post(
                    &args,
                    &format!("repos/{}/{}/issues", args.owner, args.repo),
                    &body,
                )
                .await
            }
            "search_code" => {
                let q = if args.query.contains("repo:") {
                    args.query.clone()
                } else {
                    format!("{} repo:{}/{}", args.query, args.owner, args.repo)
                };
                github_api_get(&args, &format!("search/code?q={}", urlencoding::encode(&q))).await
            }
            "add_comment" => {
                let body = json!({ "body": args.body });
                github_api_post(
                    &args,
                    &format!(
                        "repos/{}/{}/issues/{}/comments",
                        args.owner,
                        args.repo,
                        args.number.unwrap_or(0)
                    ),
                    &body,
                )
                .await
            }
            "merge_pr" => {
                let body = json!({ "commit_title": args.title, "merge_method": "merge" });
                github_api_put(
                    &args,
                    &format!(
                        "repos/{}/{}/pulls/{}/merge",
                        args.owner,
                        args.repo,
                        args.number.unwrap_or(0)
                    ),
                    &body,
                )
                .await
            }
            "close_pr" => {
                let body = json!({ "state": "closed" });
                github_api_patch(
                    &args,
                    &format!(
                        "repos/{}/{}/pulls/{}",
                        args.owner,
                        args.repo,
                        args.number.unwrap_or(0)
                    ),
                    &body,
                )
                .await
            }
            "close_issue" => {
                let body = json!({ "state": "closed" });
                github_api_patch(
                    &args,
                    &format!(
                        "repos/{}/{}/issues/{}",
                        args.owner,
                        args.repo,
                        args.number.unwrap_or(0)
                    ),
                    &body,
                )
                .await
            }
            "local_status" | "local_diff" | "local_log" | "local_commit" | "local_push"
            | "local_pull" | "local_fetch" => local_git(&args).await,
            _ => Ok(json!({"error": format!("Unknown action: {}", args.action)})),
        }?;

        let mut result_str = serde_json::to_string_pretty(&result)?;
        if result_str.len() > MAX_OUTPUT_CHARS * 2 {
            result_str = truncate_output(&result_str, MAX_OUTPUT_CHARS);
        }
        Ok(result_str)
    }
}

fn parse_git_ops_args(arguments: &str) -> anyhow::Result<GitOpsArgs> {
    let mut value: serde_json::Value = serde_json::from_str(arguments)?;
    if let Some(map) = value.as_object_mut() {
        if !map.contains_key("action") {
            let inferred = map
                .get("operation")
                .or_else(|| map.get("command"))
                .or_else(|| map.get("query"))
                .and_then(|value| value.as_str())
                .and_then(infer_git_action);
            if let Some(action) = inferred {
                map.insert(
                    "action".to_string(),
                    serde_json::Value::String(action.to_string()),
                );
            }
        } else if let Some(action) = map.get("action").and_then(|value| value.as_str()) {
            if let Some(normalized) = infer_git_action(action) {
                map.insert(
                    "action".to_string(),
                    serde_json::Value::String(normalized.to_string()),
                );
            }
        }
    }
    Ok(serde_json::from_value(value)?)
}

fn infer_git_action(raw: &str) -> Option<&'static str> {
    let lower = raw.to_ascii_lowercase();
    let trimmed = lower.trim();
    match trimmed {
        "status" | "git status" | "local status" => Some("local_status"),
        "diff" | "git diff" | "local diff" => Some("local_diff"),
        "log" | "git log" | "history" | "local log" => Some("local_log"),
        "fetch" | "git fetch" => Some("local_fetch"),
        "pull" | "git pull" => Some("local_pull"),
        "push" | "git push" => Some("local_push"),
        "info" | "capabilities" | "health" => Some("info"),
        _ if trimmed.contains("git status") => Some("local_status"),
        _ if trimmed.contains("git diff") => Some("local_diff"),
        _ if trimmed.contains("git log") => Some("local_log"),
        _ => None,
    }
}

fn truncate_output(input: &str, limit: usize) -> String {
    compress_command_output("git_ops", input, limit).content
}

fn compress_git_output(command: &str, input: &str, limit: usize) -> String {
    compress_command_output(command, input, limit).content
}

fn is_safe_path(path_str: &str) -> bool {
    if path_str.is_empty() || path_str == "." {
        return true;
    }
    let path = Path::new(path_str);
    // Basic prevention of absolute paths or parent directory traversal
    !path.is_absolute() && !path_str.contains("..")
}

async fn detect_capabilities() -> anyhow::Result<serde_json::Value> {
    let has_git = which::which("git").is_ok();
    let has_token = std::env::var("GITHUB_TOKEN").is_ok();

    Ok(json!({
        "git_binary": has_git,
        "github_token_env": has_token,
        "actions": {
            "local_git": has_git,
            "github_api": true, // Always available but needs token for some ops
        },
        "degradation": if !has_git {
            "Local git operations (status, commit, etc.) are unavailable. Install git for local repo management."
        } else {
            "All git operations available."
        }
    }))
}

fn resolve_token(args: &GitOpsArgs) -> String {
    if !args.token.is_empty() {
        args.token.clone()
    } else {
        std::env::var("GITHUB_TOKEN").unwrap_or_default()
    }
}

async fn github_api_get(args: &GitOpsArgs, endpoint: &str) -> anyhow::Result<serde_json::Value> {
    github_api_request(args, "GET", endpoint, None).await
}

async fn github_api_post(
    args: &GitOpsArgs,
    endpoint: &str,
    body: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    github_api_request(args, "POST", endpoint, Some(body)).await
}

async fn github_api_patch(
    args: &GitOpsArgs,
    endpoint: &str,
    body: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    github_api_request(args, "PATCH", endpoint, Some(body)).await
}

async fn github_api_put(
    args: &GitOpsArgs,
    endpoint: &str,
    body: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    github_api_request(args, "PUT", endpoint, Some(body)).await
}

async fn github_api_request(
    args: &GitOpsArgs,
    method: &str,
    endpoint: &str,
    body: Option<&serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    let token = resolve_token(args);
    if (method == "POST" || method == "PATCH" || method == "PUT") && token.is_empty() {
        return Ok(json!({"error": "GitHub token required for write operations"}));
    }

    let client = reqwest::Client::new();
    let url = format!("https://api.github.com/{}", endpoint);
    let mut req = match method {
        "POST" => client.post(&url),
        "PATCH" => client.patch(&url),
        "PUT" => client.put(&url),
        _ => client.get(&url),
    };

    req = req
        .header("User-Agent", "benshu-agent")
        .header("Accept", "application/vnd.github.v3+json");

    if !token.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", token));
    }

    if let Some(b) = body {
        req = req.json(b);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return Ok(json!({"error": format!("Network error: {}", e)})),
    };

    let status = resp.status();
    let result: serde_json::Value = resp
        .json()
        .await
        .unwrap_or(json!({"error": "Failed to parse JSON response"}));

    if !status.is_success() {
        let msg = match status.as_u16() {
            401 | 403 => "Authentication failed or rate limited. Check your GITHUB_TOKEN.",
            404 => "Repository or resource not found.",
            422 => "Validation failed. Check your arguments (e.g. branch names, PR number).",
            _ => "GitHub API error.",
        };
        return Ok(json!({"error": msg, "details": result, "status": status.as_u16()}));
    }
    Ok(result)
}

async fn local_git(args: &GitOpsArgs) -> anyhow::Result<serde_json::Value> {
    if !is_safe_path(&args.path) {
        return Ok(
            json!({"error": format!("Unsafe path detected: {}. Access restricted to workspace subdirectories.", args.path)}),
        );
    }

    let cwd = if args.path.is_empty() {
        "."
    } else {
        &args.path
    };
    let (cmd_args, _) = match args.action.as_str() {
        "local_status" => (vec!["status", "--porcelain"], false),
        "local_diff" => (vec!["diff", "--stat"], false),
        "local_log" => (vec!["log", "--oneline", "-20"], false),
        "local_commit" => {
            if args.message.is_empty() {
                return Ok(json!({"error": "commit message required"}));
            }
            (vec!["commit", "-am", &args.message], true)
        }
        "local_push" => (vec!["push"], true),
        "local_pull" => (vec!["pull"], true),
        "local_fetch" => (vec!["fetch"], true),
        _ => return Ok(json!({"error": "unknown local action"})),
    };

    let cmd_future = tokio::process::Command::new("git")
        .args(&cmd_args)
        .current_dir(cwd)
        .output();

    match timeout(Duration::from_secs(GIT_TIMEOUT_SECS), cmd_future).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let command_label = format!("git {}", cmd_args.join(" "));

            Ok(json!({
                "success": output.status.success(),
                "stdout": compress_git_output(&command_label, &stdout, MAX_OUTPUT_CHARS),
                "stderr": compress_git_output(&command_label, &stderr, MAX_OUTPUT_CHARS),
            }))
        }
        Ok(Err(e)) => Ok(json!({"error": format!("Git execution failed: {}", e)})),
        Err(_) => Ok(
            json!({"error": format!("Git operation timed out after {} seconds", GIT_TIMEOUT_SECS)}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_definition() {
        let tool = GitOpsTool;
        let def = tool.definition().await;
        assert_eq!(def.name, "git_ops");
        assert!(def.is_verified);
    }

    #[tokio::test]
    async fn test_local_status() {
        let tool = GitOpsTool;
        let result = tool
            .call(r#"{"action": "local_status", "path": "."}"#)
            .await;
        assert!(result.is_ok());
    }
}
