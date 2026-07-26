use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use benshu_engram::HybridSearchEngine;
use benshu_infra::error::Error;
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};

#[derive(Debug, Deserialize)]
struct KnowledgeManageArgs {
    action: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    metadata: HashMap<String, String>,
    #[serde(default)]
    confirmation_phrase: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    5
}

pub struct KnowledgeManageDocumentTool {
    search_engine: Arc<HybridSearchEngine>,
}

enum DocumentPathResolution {
    Found(benshu_engram::prelude::Document),
    Ambiguous(Vec<benshu_engram::prelude::Document>),
    Missing,
}

impl KnowledgeManageDocumentTool {
    pub fn new(search_engine: Arc<HybridSearchEngine>) -> Self {
        Self { search_engine }
    }

    fn delete_confirmation_phrase(collection: &str, path: &str) -> String {
        format!("DELETE {}/{}", collection, path)
    }

    fn update_confirmation_phrase(collection: &str, path: &str) -> String {
        format!("UPDATE {}/{}", collection, path)
    }

    fn format_candidate(
        index: usize,
        doc: &benshu_engram::prelude::Document,
        score: Option<f64>,
    ) -> String {
        let source = doc
            .metadata
            .get("source_url")
            .map(|url| format!("\n  source_url: {}", url))
            .unwrap_or_default();
        let score = score
            .map(|score| format!("\n  score: {:.4}", score))
            .unwrap_or_default();
        format!(
            "{}. title: {}\n  collection: {}\n  path: {}\n  docid: {}{}{}",
            index, doc.title, doc.collection, doc.path, doc.docid, score, source
        )
    }

    fn path_matches_request(stored_path: &str, requested_path: &str) -> bool {
        let requested = requested_path.trim();
        if stored_path == requested || stored_path.ends_with(&format!("/{requested}")) {
            return true;
        }

        let stored_name = Path::new(stored_path)
            .file_name()
            .and_then(|value| value.to_str());
        let requested_name = Path::new(requested)
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty());

        stored_name.is_some() && requested_name.is_some() && stored_name == requested_name
    }

    fn resolve_document_path(
        &self,
        collection: &str,
        requested_path: &str,
    ) -> anyhow::Result<DocumentPathResolution> {
        if let Some(doc) = self.search_engine.get_by_path(collection, requested_path)? {
            return Ok(DocumentPathResolution::Found(doc));
        }

        let matches = self
            .search_engine
            .list_documents_in_collection(collection)?
            .into_iter()
            .filter(|doc| Self::path_matches_request(&doc.path, requested_path))
            .collect::<Vec<_>>();

        match matches.len() {
            0 => Ok(DocumentPathResolution::Missing),
            1 => Ok(DocumentPathResolution::Found(
                matches
                    .into_iter()
                    .next()
                    .expect("single match exists by length check"),
            )),
            _ => Ok(DocumentPathResolution::Ambiguous(matches)),
        }
    }

    fn format_ambiguous_path(
        collection: &str,
        requested_path: &str,
        docs: &[benshu_engram::prelude::Document],
    ) -> String {
        let candidates = docs
            .iter()
            .enumerate()
            .map(|(idx, doc)| Self::format_candidate(idx + 1, doc, None))
            .collect::<Vec<_>>()
            .join("\n\n");
        format!(
            "Multiple knowledge documents in collection '{}' match path '{}'. Please confirm with the exact collection/path from one candidate:\n\n{}",
            collection, requested_path, candidates
        )
    }

    fn safe_path_segment(value: &str) -> String {
        let mut out = String::with_capacity(value.len().min(80));
        let mut last_dash = false;
        for ch in value.chars() {
            let normalized = if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ch.is_whitespace()
                || matches!(ch, '-' | '_' | '.' | '/' | '\\' | ':' | '，' | '。')
            {
                Some('-')
            } else {
                None
            };
            if let Some(ch) = normalized {
                if ch == '-' {
                    if !last_dash && !out.is_empty() {
                        out.push(ch);
                    }
                    last_dash = true;
                } else {
                    out.push(ch);
                    last_dash = false;
                }
            }
            if out.len() >= 80 {
                break;
            }
        }
        let trimmed = out.trim_matches('-');
        if trimmed.is_empty() {
            "note".to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn infer_create_path(title: &str) -> String {
        let slug = Self::safe_path_segment(title);
        format!("manual/{}-{}.md", Utc::now().timestamp_millis(), slug)
    }
}

#[async_trait]
impl Tool for KnowledgeManageDocumentTool {
    fn name(&self) -> String {
        "knowledge_manage_document".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Manage durable retrieval-storage documents. Supports creating user-provided text documents, searching/listing candidates, replacing document content, and physically deleting a specific document only after explicit confirmation. Use this for natural-language requests like saving text to a knowledge base, database, document store, repository, corpus, 资料库, 数据库, 文档库, or 知识库, not for core memory facts.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "search", "list", "update", "delete"],
                        "description": "Action to perform. Use create for explicit user-provided text that should be saved as a durable knowledge document. Use search/list before update/delete unless the exact collection and path are already known."
                    },
                    "query": {
                        "type": "string",
                        "description": "Natural language query to locate candidate documents."
                    },
                    "collection": {
                        "type": "string",
                        "description": "Knowledge collection for list/delete."
                    },
                    "path": {
                        "type": "string",
                        "description": "Document path inside the collection for update/delete."
                    },
                    "title": {
                        "type": "string",
                        "description": "Replacement title for update. Defaults to the current title or path."
                    },
                    "content": {
                        "type": "string",
                        "description": "Full document body for create or replacement document content for update."
                    },
                    "metadata": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Metadata to merge into the replacement document during update."
                    },
                    "confirmation_phrase": {
                        "type": "string",
                        "description": "Required for update/delete. Must exactly match UPDATE {collection}/{path} or DELETE {collection}/{path}."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum candidates to return."
                    }
                },
                "required": ["action"]
            }),
            parameters_ts: Some("type KnowledgeManageDocumentArgs = { action: 'create' | 'search' | 'list' | 'update' | 'delete'; query?: string; collection?: string; path?: string; title?: string; content?: string; metadata?: Record<string, string>; confirmation_phrase?: string; limit?: number }".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("For explicit user-provided text that should be saved to durable retrieval storage, call action='create'. Treat user terms such as knowledge base, database, document store, repository, corpus, 资料库, 数据库, 文档库, and 知识库 as this same storage class when the surrounding instruction asks to save/import/store. For URL ingestion, use knowledge_import_url instead. For natural-language update/delete requests, first call action='search' or action='list' to identify candidate documents, then ask the user to confirm the exact phrase returned by this tool. Only call action='update' or action='delete' after the user explicitly confirms that phrase. Panel/UI deletion may call the gateway delete API directly.".to_string()),
            safety_level: SafetyLevel::Yellow,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: KnowledgeManageArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: e.to_string(),
            })?;

        match args.action.as_str() {
            "create" => {
                let content = args
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| Error::ToolArguments {
                        tool_name: self.name(),
                        message: "content is required for create".to_string(),
                    })?;
                let collection = args
                    .collection
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("knowledge");
                let title = args
                    .title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("User provided knowledge");
                let inferred_path;
                let path = if let Some(path) = args
                    .path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    path
                } else {
                    inferred_path = Self::infer_create_path(title);
                    inferred_path.as_str()
                };
                let mut metadata = args.metadata;
                metadata.insert("knowledge_manage_action".to_string(), "create".to_string());
                metadata.insert(
                    "ingest_source".to_string(),
                    "knowledge_manage_create".to_string(),
                );
                metadata.insert("created_from".to_string(), "natural_language".to_string());
                let doc = self
                    .search_engine
                    .replace_document_content(collection, path, title, content, metadata)?;
                Ok(format!(
                    "runtime_effect: knowledge.imported\nstorage_target: durable_knowledge_store\ncollection: {}\npath: {}\ntitle: {}\ndocid: {}\n\nKnowledge document created: {}/{}\ntitle: {}\ndocid: {}",
                    doc.collection, doc.path, doc.title, doc.docid,
                    doc.collection, doc.path, doc.title, doc.docid
                ))
            }
            "search" => {
                let query = args
                    .query
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| Error::ToolArguments {
                        tool_name: self.name(),
                        message: "query is required for search".to_string(),
                    })?;
                let results = self
                    .search_engine
                    .search(query, args.limit.max(1).min(20))?;
                if results.is_empty() {
                    return Ok("No matching knowledge documents found.".to_string());
                }
                let candidates = results
                    .iter()
                    .enumerate()
                    .map(|(idx, result)| {
                        Self::format_candidate(idx + 1, &result.document, Some(result.rrf_score))
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                Ok(format!(
                    "Knowledge document candidates:\n\n{}\n\nTo update one candidate, ask the user to confirm exactly: UPDATE <collection>/<path>\nTo delete one candidate, ask the user to confirm exactly: DELETE <collection>/<path>",
                    candidates
                ))
            }
            "list" => {
                let collection = args
                    .collection
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("knowledge");
                let mut docs = self
                    .search_engine
                    .list_documents_in_collection(collection)?;
                docs.sort_by(|left, right| {
                    right
                        .updated_at_ms
                        .cmp(&left.updated_at_ms)
                        .then_with(|| left.path.cmp(&right.path))
                });
                if docs.is_empty() {
                    return Ok(format!(
                        "No knowledge documents found in collection '{}'.",
                        collection
                    ));
                }
                let candidates = docs
                    .iter()
                    .take(args.limit.max(1).min(50))
                    .enumerate()
                    .map(|(idx, doc)| Self::format_candidate(idx + 1, doc, None))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                Ok(format!("Knowledge documents:\n\n{}", candidates))
            }
            "update" => {
                let collection = args
                    .collection
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| Error::ToolArguments {
                        tool_name: self.name(),
                        message: "collection is required for update".to_string(),
                    })?;
                let path = args
                    .path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| Error::ToolArguments {
                        tool_name: self.name(),
                        message: "path is required for update".to_string(),
                    })?;
                let existing = match self.resolve_document_path(collection, path)? {
                    DocumentPathResolution::Found(doc) => doc,
                    DocumentPathResolution::Ambiguous(docs) => {
                        return Ok(Self::format_ambiguous_path(collection, path, &docs));
                    }
                    DocumentPathResolution::Missing => {
                        return Ok(format!(
                            "Knowledge document not found, nothing updated: {}/{}",
                            collection, path
                        ));
                    }
                };
                let resolved_path = existing.path.as_str();
                let expected = Self::update_confirmation_phrase(collection, resolved_path);
                if args.confirmation_phrase.as_deref() != Some(expected.as_str()) {
                    return Ok(format!(
                        "Update requires explicit user confirmation because it replaces the stored knowledge document body. To update this knowledge document, the user must confirm exactly:\n{}",
                        expected
                    ));
                }
                let content = args
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| Error::ToolArguments {
                        tool_name: self.name(),
                        message: "content is required for update".to_string(),
                    })?;
                let title = args
                    .title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(&existing.title);
                let mut metadata = existing.metadata.clone();
                metadata.extend(args.metadata);
                metadata.insert("knowledge_manage_action".to_string(), "update".to_string());
                metadata.insert(
                    "knowledge_manage_previous_docid".to_string(),
                    existing.docid.clone(),
                );

                let updated = self.search_engine.replace_document_content(
                    collection,
                    resolved_path,
                    title,
                    content,
                    metadata,
                )?;
                Ok(format!(
                    "runtime_effect: knowledge.updated\nstorage_target: durable_knowledge_store\ncollection: {}\npath: {}\ntitle: {}\ndocid: {}\n\nKnowledge document updated: {}/{}\ntitle: {}\ndocid: {}",
                    updated.collection, updated.path, updated.title, updated.docid,
                    updated.collection, updated.path, updated.title, updated.docid
                ))
            }
            "delete" => {
                let collection = args
                    .collection
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| Error::ToolArguments {
                        tool_name: self.name(),
                        message: "collection is required for delete".to_string(),
                    })?;
                let path = args
                    .path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| Error::ToolArguments {
                        tool_name: self.name(),
                        message: "path is required for delete".to_string(),
                    })?;
                let existing = match self.resolve_document_path(collection, path)? {
                    DocumentPathResolution::Found(doc) => doc,
                    DocumentPathResolution::Ambiguous(docs) => {
                        return Ok(Self::format_ambiguous_path(collection, path, &docs));
                    }
                    DocumentPathResolution::Missing => {
                        return Ok(format!(
                            "Knowledge document not found, nothing deleted: {}/{}",
                            collection, path
                        ));
                    }
                };
                let resolved_path = existing.path.as_str();
                let expected = Self::delete_confirmation_phrase(collection, resolved_path);
                if args.confirmation_phrase.as_deref() != Some(expected.as_str()) {
                    return Ok(format!(
                        "Deletion requires explicit user confirmation. To physically delete this knowledge document, the user must confirm exactly:\n{}",
                        expected
                    ));
                }

                self.search_engine
                    .delete_document(collection, resolved_path)?;
                Ok(format!(
                    "runtime_effect: knowledge.deleted\nstorage_target: durable_knowledge_store\ncollection: {}\npath: {}\n\nKnowledge document physically deleted: {}/{}",
                    collection, resolved_path,
                    collection, resolved_path
                ))
            }
            other => Err(Error::ToolArguments {
                tool_name: self.name(),
                message: format!("invalid action: {}", other),
            }
            .into()),
        }
    }
}
