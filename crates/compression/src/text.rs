use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TruncationNotice {
    ToolOutput,
    ContextSafety,
    Generic,
    DocumentUnderstand,
    RepeatedSpecialistResult,
}

impl TruncationNotice {
    fn head_tail_message(self, omitted_chars: usize) -> String {
        match self {
            Self::ToolOutput => {
                format!("Note: Output truncated; {omitted_chars} characters omitted to save tokens")
            }
            Self::ContextSafety => {
                format!("... {omitted_chars} characters truncated for context safety ...")
            }
            Self::Generic => format!("... {omitted_chars} characters truncated ..."),
            Self::DocumentUnderstand => {
                format!("document_understand output truncated; {omitted_chars} characters omitted")
            }
            Self::RepeatedSpecialistResult => {
                "... trimmed repeated specialist result ...".to_string()
            }
        }
    }

    fn head_message(self, shown_chars: usize, original_chars: usize) -> String {
        match self {
            Self::DocumentUnderstand => format!(
                "document_understand output truncated: showing first {shown_chars} of {original_chars} chars"
            ),
            Self::ToolOutput => format!(
                "Note: Output truncated from {original_chars} to {shown_chars} chars to save tokens"
            ),
            Self::ContextSafety => format!(
                "showing first {shown_chars} of {original_chars} chars for context safety"
            ),
            Self::Generic | Self::RepeatedSpecialistResult => {
                format!("showing first {shown_chars} of {original_chars} chars")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressionResult {
    pub content: String,
    pub original_chars: usize,
    pub output_chars: usize,
    pub omitted_chars: usize,
    pub truncated: bool,
}

impl CompressionResult {
    fn unchanged(input: &str) -> Self {
        let original_chars = input.chars().count();
        Self {
            content: input.to_string(),
            original_chars,
            output_chars: original_chars,
            omitted_chars: 0,
            truncated: false,
        }
    }

    fn truncated(content: String, original_chars: usize, omitted_chars: usize) -> Self {
        let output_chars = content.chars().count();
        Self {
            content,
            original_chars,
            output_chars,
            omitted_chars,
            truncated: true,
        }
    }
}

pub fn head_tail(input: &str, max_chars: usize) -> CompressionResult {
    head_tail_with_notice(input, max_chars, TruncationNotice::Generic)
}

pub fn head_tail_with_notice(
    input: &str,
    max_chars: usize,
    notice: TruncationNotice,
) -> CompressionResult {
    let original_chars = input.chars().count();
    if max_chars == 0 || original_chars <= max_chars {
        return CompressionResult::unchanged(input);
    }

    let head_len = max_chars / 2;
    let tail_len = max_chars.saturating_sub(head_len);
    let head: String = input.chars().take(head_len).collect();
    let tail: String = input
        .chars()
        .rev()
        .take(tail_len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let omitted_chars = original_chars.saturating_sub(max_chars);
    let content = format!(
        "{head}\n\n[{}]\n\n{tail}",
        notice.head_tail_message(omitted_chars)
    );
    CompressionResult::truncated(content, original_chars, omitted_chars)
}

pub fn head_with_notice(
    input: &str,
    max_chars: usize,
    notice: TruncationNotice,
) -> CompressionResult {
    let original_chars = input.chars().count();
    if max_chars == 0 || original_chars <= max_chars {
        return CompressionResult::unchanged(input);
    }

    let head: String = input.chars().take(max_chars).collect();
    let omitted_chars = original_chars.saturating_sub(max_chars);
    let content = format!(
        "{head}\n\n[{}]",
        notice.head_message(max_chars, original_chars)
    );
    CompressionResult::truncated(content, original_chars, omitted_chars)
}

pub fn ellipsize(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        input.to_string()
    } else {
        format!("{}...", input.chars().take(max_chars).collect::<String>())
    }
}

pub fn line_window(input: &str, max_lines: usize) -> CompressionResult {
    let original_lines = input.lines().count();
    if max_lines == 0 || original_lines <= max_lines {
        return CompressionResult::unchanged(input);
    }

    let content = input.lines().take(max_lines).collect::<Vec<_>>().join("\n");
    let omitted_lines = original_lines.saturating_sub(max_lines);
    let content = format!("{content}\n\n[... {omitted_lines} lines omitted ...]");
    CompressionResult::truncated(content, input.chars().count(), 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_tail_is_utf8_safe() {
        let input = "猫".repeat(20);
        let result = head_tail_with_notice(&input, 10, TruncationNotice::ContextSafety);
        assert!(result.truncated);
        assert!(result.content.contains("characters truncated"));
    }

    #[test]
    fn ellipsize_keeps_short_text() {
        assert_eq!(ellipsize("hello", 10), "hello");
        assert_eq!(ellipsize("hello world", 5), "hello...");
    }
}
