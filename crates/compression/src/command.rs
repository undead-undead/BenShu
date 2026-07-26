use crate::{head_tail_with_notice, line_window, CompressionResult, TruncationNotice};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandCompressionMode {
    Passthrough,
    Generic,
    VerbosePreserved,
    Cargo,
    GitStatus,
    GitDiff,
    GitLog,
    Pytest,
    NodeTest,
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandCompressionResult {
    pub content: String,
    pub mode: CommandCompressionMode,
    pub original_chars: usize,
    pub output_chars: usize,
    pub truncated: bool,
}

impl CommandCompressionResult {
    fn new(content: String, mode: CommandCompressionMode, original_chars: usize) -> Self {
        let output_chars = content.chars().count();
        Self {
            truncated: output_chars < original_chars,
            content,
            mode,
            original_chars,
            output_chars,
        }
    }

    fn from_compression(result: CompressionResult, mode: CommandCompressionMode) -> Self {
        Self {
            content: result.content,
            mode,
            original_chars: result.original_chars,
            output_chars: result.output_chars,
            truncated: result.truncated,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcomeKind {
    Success,
    NoMatch,
    PartialSuccess,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcome {
    pub success: bool,
    pub kind: CommandOutcomeKind,
    pub summary: String,
    pub raw_status_success: bool,
    pub exit_code: Option<i32>,
}

pub fn interpret_command_outcome(
    command: &str,
    exit_code: Option<i32>,
    raw_status_success: bool,
    stdout: &str,
    stderr: &str,
) -> CommandOutcome {
    let lower = command.to_lowercase();
    let trimmed_stdout = stdout.trim();
    let trimmed_stderr = stderr.trim();

    if raw_status_success {
        return CommandOutcome {
            success: true,
            kind: CommandOutcomeKind::Success,
            summary: "command completed successfully".to_string(),
            raw_status_success,
            exit_code,
        };
    }

    if matches!(exit_code, Some(1)) && looks_like_search(&lower) {
        return CommandOutcome {
            success: true,
            kind: CommandOutcomeKind::NoMatch,
            summary: "search command completed with no matches".to_string(),
            raw_status_success,
            exit_code,
        };
    }

    if lower.contains("robocopy") {
        if let Some(code) = exit_code {
            if (0..=7).contains(&code) {
                return CommandOutcome {
                    success: true,
                    kind: if code == 0 {
                        CommandOutcomeKind::Success
                    } else {
                        CommandOutcomeKind::PartialSuccess
                    },
                    summary: format!("robocopy completed with non-fatal exit code {code}"),
                    raw_status_success,
                    exit_code,
                };
            }
        }
    }

    let detail = if !trimmed_stderr.is_empty() {
        trimmed_stderr
    } else if !trimmed_stdout.is_empty() {
        trimmed_stdout
    } else {
        "command exited with a non-zero status"
    };

    CommandOutcome {
        success: false,
        kind: CommandOutcomeKind::Failure,
        summary: crate::preview_text(detail, 240),
        raw_status_success,
        exit_code,
    }
}

pub fn compress_command_output(
    command: &str,
    output: &str,
    max_chars: usize,
) -> CommandCompressionResult {
    let original_chars = output.chars().count();
    if max_chars == 0 || original_chars <= max_chars {
        return CommandCompressionResult::new(
            output.to_string(),
            CommandCompressionMode::Passthrough,
            original_chars,
        );
    }

    let lower = command.to_lowercase();
    if command_requests_verbose_output(&lower) {
        return CommandCompressionResult::from_compression(
            head_tail_with_notice(output, max_chars, TruncationNotice::ToolOutput),
            CommandCompressionMode::VerbosePreserved,
        );
    }

    let (mode, filtered) = if looks_like_cargo(&lower) {
        (
            CommandCompressionMode::Cargo,
            select_lines(output, cargo_relevant_line),
        )
    } else if lower.contains("pytest") {
        (
            CommandCompressionMode::Pytest,
            select_lines(output, test_relevant_line),
        )
    } else if looks_like_node_test(&lower) {
        (
            CommandCompressionMode::NodeTest,
            select_lines(output, test_relevant_line),
        )
    } else if lower.starts_with("git status") || lower.contains(" git status") {
        (
            CommandCompressionMode::GitStatus,
            limit_lines(output, 160, "git status output"),
        )
    } else if lower.starts_with("git diff") || lower.contains(" git diff") {
        (
            CommandCompressionMode::GitDiff,
            select_lines(output, git_diff_relevant_line),
        )
    } else if lower.starts_with("git log") || lower.contains(" git log") {
        (
            CommandCompressionMode::GitLog,
            limit_lines(output, 120, "git log output"),
        )
    } else if looks_like_search(&lower) {
        (
            CommandCompressionMode::Search,
            limit_lines(output, 200, "search output"),
        )
    } else {
        (CommandCompressionMode::Generic, String::new())
    };

    let candidate = if filtered.trim().is_empty() {
        head_tail_with_notice(output, max_chars, TruncationNotice::ToolOutput).content
    } else if filtered.chars().count() > max_chars {
        head_tail_with_notice(&filtered, max_chars, TruncationNotice::ToolOutput).content
    } else {
        filtered
    };

    CommandCompressionResult::new(candidate, mode, original_chars)
}

pub fn command_requests_verbose_output(command_lowercase: &str) -> bool {
    let verbose_flags = [
        "--verbose",
        "--debug",
        "--trace",
        "--nocapture",
        "--show-output",
        "--full",
        "--raw",
        "-vv",
        "-vvv",
    ];
    verbose_flags
        .iter()
        .any(|flag| command_lowercase.contains(flag))
        || command_lowercase
            .split_whitespace()
            .any(|token| token == "-v")
}

fn looks_like_cargo(command: &str) -> bool {
    command.starts_with("cargo ") || command.contains(" cargo ")
}

fn looks_like_node_test(command: &str) -> bool {
    command.contains("npm test")
        || command.contains("pnpm test")
        || command.contains("yarn test")
        || command.contains("vitest")
        || command.contains("jest")
}

fn looks_like_search(command: &str) -> bool {
    command.starts_with("rg ")
        || command.contains(" rg ")
        || command.starts_with("grep ")
        || command.contains(" grep ")
        || command.starts_with("find ")
        || command.contains(" find ")
}

fn cargo_relevant_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    test_relevant_line(line)
        || lower.contains("compiling ")
        || lower.contains("finished ")
        || lower.contains("running ")
        || lower.contains("warning:")
        || lower.contains("error[")
}

fn test_relevant_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("error")
        || lower.contains("failed")
        || lower.contains("failure")
        || lower.contains("panic")
        || lower.contains("assert")
        || lower.contains("expected")
        || lower.contains("actual")
        || lower.contains("test result")
        || lower.starts_with("----")
        || lower.contains("thread '")
        || lower.contains("stack backtrace")
        || lower.contains("warning:")
}

fn git_diff_relevant_line(line: &str) -> bool {
    line.starts_with("diff --git")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("@@")
        || line.starts_with('+')
        || line.starts_with('-')
}

fn select_lines<F>(output: &str, keep: F) -> String
where
    F: Fn(&str) -> bool,
{
    let mut kept = Vec::new();
    let mut omitted = 0usize;
    for line in output.lines() {
        if keep(line) {
            kept.push(line.to_string());
        } else {
            omitted += 1;
        }
    }

    if omitted > 0 && !kept.is_empty() {
        kept.push(format!("[{omitted} non-essential lines omitted]"));
    }
    kept.join("\n")
}

fn limit_lines(output: &str, max_lines: usize, label: &str) -> String {
    let total = output.lines().count();
    if total <= max_lines {
        return output.to_string();
    }
    let body = line_window(output, max_lines).content;
    format!("{body}\n[{} truncated: {} total lines]", label, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_output_keeps_failure_lines() {
        let output = format!(
            "{}\nerror[E0425]: cannot find value `x` in this scope\n  --> src/lib.rs:1:1\nfailures:\n    test_name\ntest result: FAILED. 0 passed; 1 failed\n{}",
            "noise\n".repeat(200),
            "more noise\n".repeat(200)
        );
        let result = compress_command_output("cargo test", &output, 400);
        assert_eq!(result.mode, CommandCompressionMode::Cargo);
        assert!(result.content.contains("error[E0425]"));
        assert!(result.content.contains("test result: FAILED"));
        assert!(result.content.len() < output.len());
    }

    #[test]
    fn verbose_flag_uses_conservative_compression() {
        let output = "line\n".repeat(1000);
        let result = compress_command_output("cargo test -- --nocapture", &output, 200);
        assert_eq!(result.mode, CommandCompressionMode::VerbosePreserved);
        assert!(result.content.contains("Output truncated"));
    }

    #[test]
    fn git_diff_keeps_hunks() {
        let output = format!(
            "{}\ndiff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new\n{}",
            "noise\n".repeat(200),
            "noise\n".repeat(200)
        );
        let result = compress_command_output("git diff", &output, 300);
        assert_eq!(result.mode, CommandCompressionMode::GitDiff);
        assert!(result.content.contains("@@ -1 +1 @@"));
        assert!(result.content.contains("+new"));
    }

    #[test]
    fn search_exit_one_is_no_match_not_failure() {
        let outcome = interpret_command_outcome("rg missing", Some(1), false, "", "");
        assert!(outcome.success);
        assert_eq!(outcome.kind, CommandOutcomeKind::NoMatch);
    }

    #[test]
    fn robocopy_non_fatal_codes_are_success() {
        let outcome = interpret_command_outcome("robocopy C:\\a C:\\b /E", Some(3), false, "", "");
        assert!(outcome.success);
        assert_eq!(outcome.kind, CommandOutcomeKind::PartialSuccess);
    }
}
