pub(crate) fn sanitize_generated_file_artifact(output: &str, task: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return empty_artifact_failure(task);
    }

    let without_thinking = strip_tagged_thinking_blocks(trimmed);
    let without_channels = strip_provider_channels(&without_thinking);
    without_channels.trim().to_string()
}

fn empty_artifact_failure(task: &str) -> String {
    format!(
        "# BenShu 文件产物\n\n原始请求：\n{}\n\n生成失败：worker 返回了空内容。",
        task.trim()
    )
}

fn strip_provider_channels(value: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Visibility {
        Visible,
        Hidden,
    }

    let mut visibility = Visibility::Visible;
    let mut output = Vec::new();
    for line in value.lines() {
        let lowered = line.to_ascii_lowercase();
        if is_hidden_channel_marker(&lowered) {
            visibility = Visibility::Hidden;
            continue;
        }
        if is_visible_channel_marker(&lowered) {
            visibility = Visibility::Visible;
            if let Some(rest) = content_after_channel_marker(line) {
                if !rest.is_empty() {
                    output.push(rest.to_string());
                }
            }
            continue;
        }
        if is_channel_close_marker(&lowered) {
            visibility = Visibility::Visible;
            if let Some(rest) = content_after_channel_marker(line) {
                if !rest.is_empty() {
                    output.push(rest.to_string());
                }
            }
            continue;
        }
        if line_contains_control_token(line) {
            continue;
        }
        if visibility == Visibility::Visible {
            output.push(line.to_string());
        }
    }
    output.join("\n")
}

fn is_hidden_channel_marker(lowered: &str) -> bool {
    channel_marker(lowered)
        && ["analysis", "thought", "reasoning"]
            .iter()
            .any(|name| lowered.contains(name))
}

fn is_visible_channel_marker(lowered: &str) -> bool {
    channel_marker(lowered) && lowered.contains("final")
}

fn is_channel_close_marker(lowered: &str) -> bool {
    (lowered.contains("<channel|>") || lowered.contains("<|channel|>"))
        && !["analysis", "thought", "reasoning", "final"]
            .iter()
            .any(|name| lowered.contains(name))
}

fn channel_marker(value: &str) -> bool {
    value.contains("<|channel") || value.contains("<channel|")
}

fn content_after_channel_marker(line: &str) -> Option<&str> {
    for token in ["<|message|>", "<|channel|>", "<channel|>"] {
        if let Some((_, rest)) = line.split_once(token) {
            return Some(rest.trim());
        }
    }
    None
}

fn line_contains_control_token(line: &str) -> bool {
    [
        "<|channel>",
        "<|channel|>",
        "<channel|>",
        "<|message|>",
        "<|start|>",
        "<|end|>",
    ]
    .iter()
    .any(|token| line.contains(token))
}

fn strip_tagged_thinking_blocks(value: &str) -> String {
    let mut remaining = value;
    let mut output = String::with_capacity(value.len());
    loop {
        let Some(start) = remaining.find("<think>") else {
            output.push_str(remaining);
            break;
        };
        output.push_str(&remaining[..start]);
        let hidden = &remaining[start + "<think>".len()..];
        let Some(end) = hidden.find("</think>") else {
            break;
        };
        remaining = &hidden[end + "</think>".len()..];
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_hidden_channel_body_and_preserves_final_paragraphs() {
        let raw =
            "<|channel>analysis\n内部推理不能出现\n<|channel>final\n# 标题\n\n第一段。\n\n第二段。";
        let cleaned = sanitize_generated_file_artifact(raw, "写文章");

        assert!(!cleaned.contains("内部推理"));
        assert_eq!(cleaned, "# 标题\n\n第一段。\n\n第二段。");
    }

    #[test]
    fn removes_think_blocks() {
        let raw = "<think>不要泄露的推理</think># 标题\n\n正文";
        let cleaned = sanitize_generated_file_artifact(raw, "写文章");
        assert_eq!(cleaned, "# 标题\n\n正文");
    }

    #[test]
    fn malformed_channel_close_keeps_following_visible_content() {
        let raw = "<|channel>thought\n内部推理\n<channel|># 标题\n\n正文";
        let cleaned = sanitize_generated_file_artifact(raw, "写文章");

        assert_eq!(cleaned, "# 标题\n\n正文");
    }
}
