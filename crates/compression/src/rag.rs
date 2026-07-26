use crate::preview_text;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeSnippet {
    pub text: String,
    pub original_chars: usize,
    pub output_chars: usize,
    pub truncated: bool,
}

pub fn knowledge_snippet(content: &str, max_chars: usize) -> KnowledgeSnippet {
    let original_chars = content.chars().count();
    let text = preview_text(content.trim(), max_chars).replace('\n', " ");
    let output_chars = text.chars().count();
    KnowledgeSnippet {
        truncated: output_chars < original_chars,
        text,
        original_chars,
        output_chars,
    }
}

pub fn knowledge_snippet_text(content: &str, max_chars: usize) -> String {
    knowledge_snippet(content, max_chars).text
}

pub fn format_knowledge_result(
    index: usize,
    title: &str,
    collection: &str,
    path: &str,
    content: &str,
    max_chars: usize,
) -> String {
    format!(
        "Result {}:\ntitle: {}\ncollection: {}\npath: {}\ncontent:\n{}",
        index + 1,
        title,
        collection,
        path,
        knowledge_snippet_text(content, max_chars)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_snippet_is_single_line_and_truncated() {
        let snippet = knowledge_snippet(&format!("{}\n{}", "猫".repeat(20), "狗".repeat(20)), 10);
        assert!(snippet.truncated);
        assert!(!snippet.text.contains('\n'));
        assert!(snippet.text.contains("..."));
    }

    #[test]
    fn format_knowledge_result_keeps_metadata() {
        let result = format_knowledge_result(0, "Title", "docs", "a.md", "body", 100);
        assert!(result.contains("title: Title"));
        assert!(result.contains("collection: docs"));
        assert!(result.contains("content:\nbody"));
    }
}
