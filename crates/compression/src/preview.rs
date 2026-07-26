use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewResult {
    pub content: String,
    pub original_chars: usize,
    pub output_chars: usize,
    pub truncated: bool,
}

pub fn preview_text(input: &str, max_chars: usize) -> String {
    preview_text_result(input, max_chars).content
}

pub fn preview_text_result(input: &str, max_chars: usize) -> PreviewResult {
    let original_chars = input.chars().count();
    if max_chars == 0 || original_chars <= max_chars {
        return PreviewResult {
            content: input.to_string(),
            original_chars,
            output_chars: original_chars,
            truncated: false,
        };
    }

    let content = format!("{}...", input.chars().take(max_chars).collect::<String>());
    PreviewResult {
        output_chars: content.chars().count(),
        content,
        original_chars,
        truncated: true,
    }
}

pub fn preview_text_with_total(input: &str, max_chars: usize) -> String {
    let preview = preview_text_result(input, max_chars);
    if preview.truncated {
        format!(
            "{} ({} chars total)",
            preview.content, preview.original_chars
        )
    } else {
        preview.content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_text_is_utf8_safe() {
        let preview = preview_text("猫猫猫猫猫", 3);
        assert_eq!(preview, "猫猫猫...");
    }

    #[test]
    fn preview_text_with_total_reports_original_length() {
        let preview = preview_text_with_total("hello world", 5);
        assert_eq!(preview, "hello... (11 chars total)");
    }

    #[test]
    fn preview_text_keeps_short_input() {
        let preview = preview_text("hello", 10);
        assert_eq!(preview, "hello");
    }
}
