use async_trait::async_trait;
use chrono::Utc;
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use benshu_engram::HybridSearchEngine;
use benshu_inference::QuantLevel;
use benshu_infra::error::Error;
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};

use super::web_fetch::{WebFetchConfig, WebFetchTool};

const DEFAULT_COLLECTION: &str = "references";
const WEB_IMPORT_INGEST_SOURCE: &str = "knowledge_import_url";
const IMPORT_FETCH_TIMEOUT: Duration = Duration::from_secs(60);
const IMPORT_MAX_BODY_SIZE: usize = 4 * 1024 * 1024;
const IMPORT_MAX_OUTPUT_CHARS: usize = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct KnowledgeImportUrlArgs {
    url: String,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default = "default_unverified")]
    unverified: bool,
}

fn default_unverified() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct WebFetchStructuredPayload {
    url: String,
    content_type: String,
    backend: String,
    content: String,
}

pub struct KnowledgeImportUrlTool {
    search_engine: Arc<HybridSearchEngine>,
}

impl KnowledgeImportUrlTool {
    pub fn new(search_engine: Arc<HybridSearchEngine>) -> Self {
        Self { search_engine }
    }

    fn quant_level(collection: &str) -> QuantLevel {
        match collection.to_ascii_lowercase().as_str() {
            "experience" | "anti_pattern" => QuantLevel::Cold,
            "agent" | "core" | "identity" => QuantLevel::Full,
            _ => QuantLevel::Warm,
        }
    }

    fn clean_content(content: &str) -> String {
        let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
        let mut cleaned = String::with_capacity(normalized.len());
        let mut pending_blank_separator = false;

        for line in normalized.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if !cleaned.is_empty() {
                    pending_blank_separator = true;
                }
                continue;
            }

            if !cleaned.is_empty() {
                if pending_blank_separator {
                    cleaned.push_str("\n\n");
                } else {
                    cleaned.push('\n');
                }
            }
            cleaned.push_str(trimmed);
            pending_blank_separator = false;
        }

        cleaned
    }

    fn infer_title(explicit_title: Option<&str>, url: &Url, content: &str) -> String {
        if let Some(title) = explicit_title
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return title.to_string();
        }

        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(content) {
            if let Some(title) = Self::structured_payload_title(&payload) {
                return title;
            }
        }

        for line in content.lines() {
            let candidate = line.trim().trim_matches('#').trim();
            if candidate.len() >= 8 && candidate.len() <= 140 {
                return candidate.to_string();
            }
        }

        if let Some(segment) = url
            .path_segments()
            .and_then(|segments| segments.rev().find(|segment| !segment.trim().is_empty()))
        {
            let readable = segment.replace(['-', '_'], " ").trim().to_string();
            if !readable.is_empty() {
                return readable;
            }
        }

        url.host_str()
            .filter(|host| !host.trim().is_empty())
            .unwrap_or("web import")
            .to_string()
    }

    fn structured_payload_title(payload: &serde_json::Value) -> Option<String> {
        if let Some(title) = Self::json_title_value(payload.get("title")) {
            return Some(title);
        }

        if let Some(uid) = payload
            .get("result")
            .and_then(|value| value.get("uids"))
            .and_then(|value| value.as_array())
            .and_then(|items| items.first())
            .and_then(|value| value.as_str())
        {
            if let Some(title) = Self::json_title_value(
                payload
                    .get("result")
                    .and_then(|value| value.get(uid))
                    .and_then(|value| value.get("title")),
            ) {
                return Some(title);
            }
        }

        if let Some(items) = payload
            .get("message")
            .and_then(|value| value.get("items"))
            .and_then(|value| value.as_array())
        {
            for item in items {
                if let Some(title) = Self::json_title_value(item.get("title")) {
                    return Some(title);
                }
            }
        }

        if let Some(results) = payload.get("results").and_then(|value| value.as_array()) {
            for item in results {
                for key in ["title", "display_name", "full_name", "name"] {
                    if let Some(title) = Self::json_title_value(item.get(key)) {
                        return Some(title);
                    }
                }
            }
        }

        if let Some(items) = payload.get("items").and_then(|value| value.as_array()) {
            for item in items {
                for key in ["title", "full_name", "name"] {
                    if let Some(title) = Self::json_title_value(item.get(key)) {
                        return Some(title);
                    }
                }
            }
        }

        Self::find_nested_title(payload)
    }

    fn find_nested_title(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(title) = Self::json_title_value(map.get("title")) {
                    return Some(title);
                }
                for nested in map.values() {
                    if let Some(title) = Self::find_nested_title(nested) {
                        return Some(title);
                    }
                }
                None
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    if let Some(title) = Self::find_nested_title(item) {
                        return Some(title);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn json_title_value(value: Option<&serde_json::Value>) -> Option<String> {
        let raw = match value? {
            serde_json::Value::String(text) => text.as_str(),
            serde_json::Value::Array(items) => items.iter().find_map(|item| item.as_str())?,
            _ => return None,
        };
        let title = raw.trim().trim_matches('#').trim();
        (title.len() >= 8 && title.len() <= 240).then(|| title.to_string())
    }

    fn slugify(value: &str) -> String {
        let mut slug = String::with_capacity(value.len());
        let mut prev_dash = false;
        for ch in value.chars() {
            let mapped = if ch.is_ascii_alphanumeric() {
                prev_dash = false;
                Some(ch.to_ascii_lowercase())
            } else if !prev_dash {
                prev_dash = true;
                Some('-')
            } else {
                None
            };
            if let Some(mapped) = mapped {
                slug.push(mapped);
            }
        }
        slug.trim_matches('-').to_string()
    }

    fn infer_path(explicit_path: Option<&str>, url: &Url, title: &str) -> String {
        if let Some(path) = explicit_path
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return path.replace('\\', "/");
        }

        let host_slug = Self::slugify(url.host_str().unwrap_or("web"));
        let title_slug = Self::slugify(title);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.as_str().hash(&mut hasher);
        let short_suffix = format!("{:08x}", hasher.finish());
        let leaf = if title_slug.is_empty() {
            "document"
        } else {
            &title_slug
        };

        format!("web/{}/{}-{}", host_slug, leaf, short_suffix)
    }

    fn build_metadata(
        source_url: &str,
        content_type: &str,
        backend: &str,
        path: &str,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("document_contract_version".to_string(), "1".to_string());
        metadata.insert("document_policy_owner".to_string(), "brain".to_string());
        metadata.insert(
            "document_durable_authority".to_string(),
            "engram".to_string(),
        );
        metadata.insert(
            "document_persistence_scope".to_string(),
            "durable".to_string(),
        );
        metadata.insert(
            "document_context_role".to_string(),
            "durable_document".to_string(),
        );
        metadata.insert("document_lifecycle_state".to_string(), "active".to_string());
        metadata.insert(
            "document_ingest_source".to_string(),
            WEB_IMPORT_INGEST_SOURCE.to_string(),
        );
        metadata.insert("source_url".to_string(), source_url.to_string());
        metadata.insert("import_source".to_string(), "web".to_string());
        metadata.insert("content_type".to_string(), content_type.to_string());
        metadata.insert("fetch_backend".to_string(), backend.to_string());
        metadata.insert("document_path".to_string(), path.to_string());
        metadata.insert(
            "imported_at".to_string(),
            Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        );
        metadata
    }

    fn content_contains_verification_challenge(content: &str) -> bool {
        let lowered = content.to_ascii_lowercase();
        lowered.contains("正在进行安全验证")
            || lowered.contains("请稍候")
            || lowered.contains("cloudflare")
            || lowered.contains("enable javascript and cookies to continue")
            || lowered.contains("security verification")
            || lowered.contains("anti-bot")
            || lowered.contains("challenge page")
    }

    fn fetched_content_has_real_results(url: &Url, content: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(content) else {
            return true;
        };

        let lowered_url = url.as_str().to_ascii_lowercase();

        if lowered_url.contains("/entrez/eutils/esearch.fcgi") {
            let count = payload
                .get("esearchresult")
                .and_then(|value| value.get("count"))
                .and_then(|value| value.as_str())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let id_count = payload
                .get("esearchresult")
                .and_then(|value| value.get("idlist"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            return count > 0 || id_count > 0;
        }

        if lowered_url.contains("api.crossref.org/works") {
            let total = payload
                .get("message")
                .and_then(|value| value.get("total-results"))
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let item_count = payload
                .get("message")
                .and_then(|value| value.get("items"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            return total > 0 || item_count > 0;
        }

        if lowered_url.contains("api.openalex.org/works") {
            let total = payload
                .get("meta")
                .and_then(|value| value.get("count"))
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let item_count = payload
                .get("results")
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            return total > 0 || item_count > 0;
        }

        if lowered_url.contains("api.github.com/search/") {
            let total = payload
                .get("total_count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let item_count = payload
                .get("items")
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            return total > 0 || item_count > 0;
        }

        true
    }
}

#[async_trait]
impl Tool for KnowledgeImportUrlTool {
    fn name(&self) -> String {
        "knowledge_import_url".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Fetch a public web page and store its cleaned text into durable retrieval storage for later RAG retrieval. Use this when the user asks to save a URL, article, paper page, source, or record into a knowledge base, database, document store, repository, or similar durable store.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The http/https URL to import into the knowledge base."
                    },
                    "collection": {
                        "type": "string",
                        "description": "Target knowledge collection. Defaults to 'references'."
                    },
                    "title": {
                        "type": "string",
                        "description": "Optional override title for the stored document."
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional override virtual path inside the collection."
                    },
                    "unverified": {
                        "type": "boolean",
                        "description": "Whether the imported page should remain unverified. Defaults to true."
                    }
                },
                "required": ["url"]
            }),
            parameters_ts: Some(
                "type KnowledgeImportUrlArgs = { url: string; collection?: string; title?: string; path?: string; unverified?: boolean }".to_string(),
            ),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some(
                "Use when the user explicitly wants a webpage, article, or paper URL saved into durable retrieval storage. Treat user terms such as knowledge base, database, document store, repository, corpus, 资料库, 数据库, 文档库, and 知识库 as durable retrieval-storage intent when the surrounding instruction asks to save/import/store source material. This stores the fetched document in Engram for later search, rather than writing it into short-term memory or facts.".to_string(),
            ),
            safety_level: SafetyLevel::Green,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: KnowledgeImportUrlArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: e.to_string(),
            })?;

        let url = args.url.trim();
        if url.is_empty() {
            return Err(Error::ToolArguments {
                tool_name: self.name(),
                message: "URL cannot be empty".to_string(),
            }
            .into());
        }

        let fetch_tool = WebFetchTool::new(WebFetchConfig {
            timeout: IMPORT_FETCH_TIMEOUT,
            max_body_size: IMPORT_MAX_BODY_SIZE,
            max_output_chars: IMPORT_MAX_OUTPUT_CHARS,
            max_retries: 2,
            ..WebFetchConfig::default()
        })
        .map_err(|e| anyhow::anyhow!("Failed to initialize web fetch: {}", e))?;
        let fetch_payload = json!({
            "url": url,
            "structured": true,
        });
        let fetch_result = fetch_tool.call(&fetch_payload.to_string()).await?;
        let observation: WebFetchStructuredPayload = serde_json::from_str(&fetch_result)
            .map_err(|e| anyhow::anyhow!("Failed to decode structured web fetch payload: {}", e))?;

        let parsed_url =
            Url::parse(&observation.url).map_err(|e| anyhow::anyhow!("Invalid URL: {}", e))?;
        let cleaned_content = Self::clean_content(&observation.content);
        if cleaned_content.is_empty() {
            anyhow::bail!("Fetched page content is empty after cleaning");
        }
        if Self::content_contains_verification_challenge(&cleaned_content) {
            anyhow::bail!(
                "Fetched page is an anti-bot/security verification page, not real source content"
            );
        }
        if !Self::fetched_content_has_real_results(&parsed_url, &cleaned_content) {
            anyhow::bail!(
                "Fetched source is a structured lookup response with zero usable results"
            );
        }

        let collection = args
            .collection
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_COLLECTION)
            .to_string();
        let title = Self::infer_title(args.title.as_deref(), &parsed_url, &cleaned_content);
        let path = Self::infer_path(args.path.as_deref(), &parsed_url, &title);
        let metadata = Self::build_metadata(
            &observation.url,
            &observation.content_type,
            &observation.backend,
            &path,
        );

        self.search_engine.index_at_level(
            &collection,
            &path,
            &title,
            &cleaned_content,
            Self::quant_level(&collection),
            args.unverified,
            metadata,
        )?;

        Ok(format!(
            "runtime_effect: knowledge.imported\nstorage_target: durable_knowledge_store\ncollection: {}\npath: {}\ntitle: {}\nsource_url: {}\n\nImported web knowledge into collection '{}' at path '{}' with title '{}'. Source URL: {}",
            collection, path, title, observation.url,
            collection, path, title, observation.url
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::KnowledgeImportUrlTool;
    use reqwest::Url;

    #[test]
    fn infer_path_is_stable_for_same_url() {
        let url = Url::parse("https://example.com/papers/attention-is-all-you-need").unwrap();
        let first = KnowledgeImportUrlTool::infer_path(None, &url, "Attention Is All You Need");
        let second = KnowledgeImportUrlTool::infer_path(None, &url, "Attention Is All You Need");
        assert_eq!(first, second);
        assert!(first.starts_with("web/example-com/attention-is-all-you-need-"));
    }

    #[test]
    fn clean_content_collapses_blank_runs() {
        let cleaned = KnowledgeImportUrlTool::clean_content("A\r\n\r\n\r\nB\n\n\nC");
        assert_eq!(cleaned, "A\n\nB\n\nC");
    }

    #[test]
    fn clean_content_preserves_adjacent_non_empty_lines() {
        let cleaned =
            KnowledgeImportUrlTool::clean_content("Title\nSubtitle\n\nBody line 1\nBody line 2");
        assert_eq!(cleaned, "Title\nSubtitle\n\nBody line 1\nBody line 2");
    }

    #[test]
    fn detects_verification_challenge_pages() {
        assert!(
            KnowledgeImportUrlTool::content_contains_verification_challenge(
                "请稍候……\n正在进行安全验证\nEnable JavaScript and cookies to continue"
            )
        );
        assert!(
            !KnowledgeImportUrlTool::content_contains_verification_challenge(
                "A real paper abstract with methods and results."
            )
        );
    }

    #[test]
    fn rejects_zero_result_structured_lookup_payloads() {
        let url =
            Url::parse("https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed")
                .unwrap();
        let content = r#"{"esearchresult":{"count":"0","idlist":[]}}"#;

        assert!(!KnowledgeImportUrlTool::fetched_content_has_real_results(
            &url, content
        ));
    }

    #[test]
    fn infer_title_uses_pubmed_esummary_article_title() {
        let url = Url::parse(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id=123",
        )
        .unwrap();
        let content = r#"{
          "header": {"type": "esummary"},
          "result": {
            "uids": ["123"],
            "123": {
              "title": "Incidence, risk factors, and cardiovascular impact of hypertension in people with HIV: a secondary analysis of the REPRIEVE trial."
            }
          }
        }"#;

        let title = KnowledgeImportUrlTool::infer_title(None, &url, content);

        assert_eq!(
            title,
            "Incidence, risk factors, and cardiovascular impact of hypertension in people with HIV: a secondary analysis of the REPRIEVE trial."
        );
    }

    #[test]
    fn infer_title_uses_crossref_first_item_title() {
        let url = Url::parse("https://api.crossref.org/works?query=heart").unwrap();
        let content = r#"{
          "message": {
            "items": [
              {"title": ["A trial title from Crossref"]}
            ]
          }
        }"#;

        let title = KnowledgeImportUrlTool::infer_title(None, &url, content);

        assert_eq!(title, "A trial title from Crossref");
    }
}
