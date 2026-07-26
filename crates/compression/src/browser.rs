use crate::knowledge_snippet_text;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultSummaryItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub fn render_search_results(
    query: &str,
    engine: &str,
    results: &[SearchResultSummaryItem],
    max_snippet_chars: usize,
) -> String {
    let mut lines = vec![
        format!("# Browser Search Results: {query}"),
        format!("Engine: {engine}"),
        format!("Results: {}", results.len()),
        String::new(),
    ];
    for result in results {
        lines.push(format!("## {}", result.title));
        lines.push(format!("URL: {}", result.url));
        if !result.snippet.trim().is_empty() {
            lines.push(knowledge_snippet_text(&result.snippet, max_snippet_chars));
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_search_results_limits_snippets() {
        let output = render_search_results(
            "q",
            "edge",
            &[SearchResultSummaryItem {
                title: "Title".to_string(),
                url: "https://example.com".to_string(),
                snippet: "a\n".repeat(100),
            }],
            20,
        );
        assert!(output.contains("# Browser Search Results: q"));
        assert!(output.contains("URL: https://example.com"));
        assert!(output.contains("..."));
    }
}
