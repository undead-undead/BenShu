use serde::{Deserialize, Serialize};

use crate::text::{head_with_notice, CompressionResult, TruncationNotice};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutputCompression {
    pub content: String,
    pub original_chars: usize,
    pub output_chars: usize,
    pub omitted_chars: usize,
    pub truncated: bool,
}

impl From<CompressionResult> for ToolOutputCompression {
    fn from(result: CompressionResult) -> Self {
        Self {
            content: result.content,
            original_chars: result.original_chars,
            output_chars: result.output_chars,
            omitted_chars: result.omitted_chars,
            truncated: result.truncated,
        }
    }
}

pub fn compress_tool_output(output: &str, max_chars: usize) -> ToolOutputCompression {
    head_with_notice(output, max_chars, TruncationNotice::ToolOutput).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_output_reports_truncation() {
        let result = compress_tool_output(&"a".repeat(100), 20);
        assert!(result.truncated);
        assert!(result.content.contains("Output truncated"));
    }
}
