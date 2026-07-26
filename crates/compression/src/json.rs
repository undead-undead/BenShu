use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredFetchSummary {
    pub content: String,
    pub item_count: usize,
}

pub fn compact_known_json_api_response(url: &str, text: &str, max_items: usize) -> Option<String> {
    let lowered_url = url.to_ascii_lowercase();
    if !lowered_url.contains("api.github.com/search/repositories") {
        return None;
    }

    let payload = serde_json::from_str::<serde_json::Value>(text).ok()?;
    let items = payload.get("items")?.as_array()?;
    let compact_items = items
        .iter()
        .take(max_items)
        .map(|item| {
            serde_json::json!({
                "full_name": item.get("full_name").and_then(|value| value.as_str()).unwrap_or(""),
                "html_url": item.get("html_url").and_then(|value| value.as_str()).unwrap_or(""),
                "stargazers_count": item.get("stargazers_count").and_then(|value| value.as_u64()).unwrap_or(0),
                "description": item.get("description").and_then(|value| value.as_str()).unwrap_or(""),
                "language": item.get("language").and_then(|value| value.as_str()).unwrap_or(""),
                "updated_at": item.get("updated_at").and_then(|value| value.as_str()).unwrap_or("")
            })
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&serde_json::json!({
        "total_count": payload.get("total_count").and_then(|value| value.as_u64()).unwrap_or(0),
        "incomplete_results": payload.get("incomplete_results").and_then(|value| value.as_bool()).unwrap_or(false),
        "items": compact_items
    }))
    .ok()
}

pub fn summarize_github_search_items(
    url: &str,
    text: &str,
    max_items: usize,
) -> Option<StructuredFetchSummary> {
    let lowered_url = url.to_ascii_lowercase();
    if !lowered_url.contains("api.github.com/search/") {
        return None;
    }

    let payload = serde_json::from_str::<serde_json::Value>(text).ok()?;
    let items = payload.get("items")?.as_array()?;
    let mut lines = Vec::new();
    for item in items.iter().take(max_items) {
        let full_name = item
            .get("full_name")
            .and_then(|value| value.as_str())
            .or_else(|| item.get("name").and_then(|value| value.as_str()))
            .unwrap_or("unknown");
        let html_url = item
            .get("html_url")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let stars = item
            .get("stargazers_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let description = item
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        lines.push(format!(
            "- {full_name} ({stars} stars): {description}\n  {html_url}"
        ));
    }

    if lines.is_empty() {
        return None;
    }

    Some(StructuredFetchSummary {
        content: lines.join("\n"),
        item_count: lines.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compacts_github_repository_search_response() {
        let payload = serde_json::json!({
            "total_count": 1,
            "incomplete_results": false,
            "items": [{
                "full_name": "owner/repo",
                "html_url": "https://github.com/owner/repo",
                "stargazers_count": 42,
                "description": "demo",
                "language": "Rust",
                "updated_at": "2026-01-01"
            }]
        });
        let compact = compact_known_json_api_response(
            "https://api.github.com/search/repositories?q=demo",
            &payload.to_string(),
            5,
        )
        .expect("compact github response");
        assert!(compact.contains("owner/repo"));
        assert!(!compact.contains("archive_url"));
    }
}
