use async_trait::async_trait;
use benshu_compression::preview_text;
use benshu_infra::error::Error;
use benshu_infra::traits::kernel::KernelCapability;
use benshu_infra::{Tool, ToolDefinition};
use benshu_memory_api::Memory;
use benshu_memory_core::{
    Fact, FactProtection, MultimodalDerivedFact, MultimodalMemoryKind, MultimodalMemoryRecord,
    Relation,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Tool for searching historical conversations and knowledge
pub struct SearchHistoryTool {
    capability: Arc<dyn KernelCapability>,
}

impl SearchHistoryTool {
    pub fn new(capability: Arc<dyn KernelCapability>) -> Self {
        Self { capability }
    }
}

#[async_trait]
impl Tool for SearchHistoryTool {
    fn name(&self) -> String {
        "search_history".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Search through past conversations, trading strategies, and knowledge using natural language or keywords. \
                Use this when you need context about a topic discussed previously or to find specific historical data.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query (natural language or keywords)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of results to return (default: 5)"
                    }
                },
                "required": ["query"]
            }),
            parameters_ts: Some("interface SearchArgs {\n  query: string; // The search query\n  limit?: number; // Max results (default: 5)\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            #[serde(default = "default_limit")]
            limit: usize,
        }
        fn default_limit() -> usize {
            5
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
            tool_name: self.name(),
            message: e.to_string(),
        })?;

        // Context is currently not passed to tools, using placeholders.
        // In a multi-user environment, the Tool trait should be updated to accept context.
        let user_id = "default";
        let agent_id: Option<String> = None;

        let results = self
            .capability
            .query_memory(&args.query, args.limit)
            .await
            .map_err(|e| Error::Internal(format!("Search failed: {}", e)))?;

        if results.is_empty() {
            return Ok("No relevant history found.".to_string());
        }

        Ok(format!(
            "Search matches (via Managed Memory Pipeline):\n\n{}",
            results
        ))
    }
}

/// Tool for saving important insights to long-term memory
pub struct RememberThisTool {
    capability: Arc<dyn KernelCapability>,
}

impl RememberThisTool {
    pub fn new(capability: Arc<dyn KernelCapability>) -> Self {
        Self { capability }
    }
}

#[async_trait]
impl Tool for RememberThisTool {
    fn name(&self) -> String {
        "remember_this".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Save a key insight, fact, or trading rule to your long-term memory. \
                Use this to ensure critical information is preserved and available for future retrieval.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Short mnemonic title for this memory"
                    },
                    "content": {
                        "type": "string",
                        "description": "The detail information to be remembered"
                    },
                    "collection": {
                        "type": "string",
                        "description": "Category (e.g., 'rules', 'preferences', 'insights')"
                    }
                },
                "required": ["title", "content"]
            }),
            parameters_ts: Some("interface RememberArgs {\n  title: string; // Short title\n  content: string; // Detail information\n  collection?: string; // Category (default: 'general')\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this to preserve high-value insights, user preferences, or critical rules discovered during a conversation. DO NOT use this for temporary chat context.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            title: String,
            content: String,
            #[serde(default = "default_coll")]
            collection: String,
        }
        fn default_coll() -> String {
            "general".to_string()
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
            tool_name: self.name(),
            message: e.to_string(),
        })?;

        if args.content.trim().is_empty() {
            return Err(Error::ToolArguments {
                tool_name: self.name(),
                message: "Content cannot be empty".into(),
            }
            .into());
        }

        tracing::info!(title = %args.title, collection = %args.collection, "Saving new memory insight via KernelCapability");

        self.capability
            .record_fact(&args.content, &args.collection)
            .await?;

        Ok(format!(
            "Memory successfully saved as '{}' in collection '{}' (Isolated & Audited).",
            args.title, args.collection
        ))
    }
}

/// Tool for managing distilled facts (Memory CRUD Protocol - Phase 10)
pub struct FactManagementTool {
    memory: Arc<dyn Memory>,
}

impl FactManagementTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }

    fn slotish_key_from_text(text: &str) -> Option<String> {
        let lower = text.to_lowercase();
        let slotish_terms = [
            "测试验证码",
            "验证码",
            "手机号",
            "电话",
            "地址",
            "偏好",
            "名字",
            "姓名",
            "邮箱",
            "标记",
            "账号",
            "密码",
            "生日",
            "token",
            "code",
            "phone",
            "email",
            "preference",
            "name",
            "address",
        ];
        slotish_terms
            .iter()
            .find(|term| lower.contains(**term))
            .map(|term| (*term).to_string())
    }

    fn fact_slot_key(content: &str) -> Option<String> {
        let normalized = content
            .trim()
            .trim_matches(|ch: char| ch == '「' || ch == '」' || ch == '"' || ch == '\'')
            .replace("我的", "")
            .replace("用户的", "")
            .replace("用户", "")
            .replace("我", "")
            .trim()
            .to_string();

        let lower = normalized.to_lowercase();
        // Natural mutation text from the LLM can look like
        // "把刚才记住的测试验证码更新为「xxx」". Treat it as the same slot
        // before generic separators such as "为" split the sentence too early.
        if (normalized.contains('「')
            || normalized.contains('"')
            || normalized.contains('\'')
            || lower.contains("更新")
            || lower.contains("改成")
            || lower.contains("update")
            || lower.contains("change"))
            && Self::slotish_key_from_text(&lower).is_some()
        {
            return Self::slotish_key_from_text(&lower);
        }

        let separators = ["：", ":", " 是 ", "为", "="];
        for separator in separators {
            if let Some((left, right)) = normalized.split_once(separator) {
                let key = left.trim();
                let value = right.trim();
                if key.chars().count() >= 2 && !value.is_empty() {
                    return Some(key.to_lowercase());
                }
            }
        }

        if normalized.chars().count() <= 32 && Self::slotish_key_from_text(&lower).is_some() {
            return Self::slotish_key_from_text(&lower);
        }

        None
    }

    fn can_auto_replace(fact: &Fact) -> bool {
        matches!(fact.protection, FactProtection::Normal)
    }

    async fn delete_same_slot_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        category: &str,
        slot_key: &str,
        keep_fact_id: Option<&str>,
    ) -> anyhow::Result<usize> {
        let facts = self.memory.retrieve_facts(user_id, agent_id).await?;
        let mut deleted = 0usize;
        for fact in facts {
            if keep_fact_id.is_some_and(|keep| keep == fact.id) {
                continue;
            }
            if fact.category != category || !Self::can_auto_replace(&fact) {
                continue;
            }
            if Self::fact_slot_key(&fact.content).as_deref() == Some(slot_key) {
                self.memory.delete_fact(user_id, agent_id, &fact.id).await?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}

#[async_trait]
impl Tool for FactManagementTool {
    fn name(&self) -> String {
        "manage_facts".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Manage your distilled knowledge (facts, preferences, status). \
                Supports 'upsert' (add/update), 'list' (view all), and 'delete' (remove). \
                Use this to maintain a clean, accurate core memory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["upsert", "list", "delete", "update_importance", "find_related", "get_status", "pin", "unpin", "protect", "unprotect", "set_core_identity", "clear_core_identity"], "description": "Action to perform" },
                    "content": { "type": "string", "description": "fact content (for upsert, or the exact/unique content to delete when fact_id is not known)" },
                    "category": { "type": "string", "description": "category e.g. 'preference' (for upsert)" },
                    "fact_id": { "type": "string", "description": "UUID of the fact (for delete, update, or find_related)" },
                    "importance": { "type": "number", "description": "0.0 to 1.0 importance score" },
                    "depth": { "type": "integer", "description": "Depth for graph traversal (for find_related, default: 2)" },
                    "verified": { "type": "boolean", "description": "Whether the fact is confirmed" },
                    "relations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "predicate": { "type": "string" },
                                "target_id": { "type": "string" },
                                "strength": { "type": "number" }
                            },
                            "required": ["predicate", "target_id"]
                        }
                    }
                },
                "required": ["action"]
            }),
            parameters_ts: Some("interface FactArgs {\n  action: 'upsert' | 'list' | 'delete' | 'update_importance' | 'find_related' | 'get_status' | 'pin' | 'unpin' | 'protect' | 'unprotect' | 'set_core_identity' | 'clear_core_identity';\n  content?: string;\n  category?: string;\n  fact_id?: string;\n  importance?: number;\n  depth?: number;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use 'upsert' to record persistent truths. When updating a remembered slot such as 'phone: ...', 'preference: ...', or 'test code: ...', pass the new slot content and the tool will replace older normal facts in the same slot. Use 'list' to see knowledge. Use 'find_related' to traverse the knowledge graph. Use 'get_status' for a global system health check. For natural-language delete requests, prefer fact_id when available; if only the remembered content is available, pass content and the tool will delete only a unique exact match or matching slot.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            content: Option<String>,
            category: Option<String>,
            fact_id: Option<String>,
            importance: Option<f32>,
            depth: Option<usize>,
            verified: Option<bool>,
            relations: Option<Vec<Relation>>,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
            tool_name: self.name(),
            message: e.to_string(),
        })?;

        let user_id = "default";
        let agent_id: Option<String> = None;

        tracing::debug!(action = %args.action, fact_id = ?args.fact_id, "Executing Fact management action");

        match args.action.as_str() {
            "upsert" => {
                let mut content = args.content.ok_or_else(|| Error::ToolArguments {
                    tool_name: self.name(),
                    message: "Content required for upsert".into(),
                })?;

                if content.trim().is_empty() {
                    return Err(Error::ToolArguments {
                        tool_name: self.name(),
                        message: "Content cannot be empty".into(),
                    }
                    .into());
                }

                let category = args.category.unwrap_or_else(|| "general".into());
                if Self::fact_slot_key(&content).is_none() && content.chars().count() <= 96 {
                    if let Ok(mut existing) = self
                        .memory
                        .retrieve_facts(user_id, agent_id.as_deref())
                        .await
                    {
                        existing.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                        if let Some(slot_key) = existing.into_iter().find_map(|fact| {
                            if !Self::can_auto_replace(&fact) {
                                return None;
                            }
                            Self::fact_slot_key(&fact.content)
                        }) {
                            let value = content.trim().trim_matches(|ch: char| {
                                ch == '「' || ch == '」' || ch == '"' || ch == '\''
                            });
                            content = format!("{}：{}", slot_key, value);
                        }
                    }
                }
                let mut fact = Fact::new(content, category);
                if let Some(id) = args.fact_id {
                    fact.id = id;
                }
                if let Some(imp) = args.importance {
                    fact.importance = imp.clamp(0.0, 1.0);
                }
                if let Some(v) = args.verified {
                    fact.verified = v;
                }
                if let Some(rel) = args.relations {
                    fact.relations = rel;
                }
                let slot_key = Self::fact_slot_key(&fact.content);
                if let Some(slot_key) = slot_key.as_deref() {
                    self.delete_same_slot_facts(
                        user_id,
                        agent_id.as_deref(),
                        &fact.category,
                        slot_key,
                        Some(&fact.id),
                    )
                    .await?;
                }
                self.memory
                    .store_fact(user_id, agent_id.as_deref(), fact)
                    .await?;
                Ok("Fact successfully upserted.".into())
            }
            "list" => {
                let facts = self
                    .memory
                    .retrieve_facts(user_id, agent_id.as_deref())
                    .await?;
                if facts.is_empty() {
                    return Ok("No facts stored.".into());
                }
                let mut table = benshu_infra::format::MarkdownTable::new(vec![
                    "ID",
                    "Category",
                    "Imp",
                    "Protection",
                    "Content",
                    "Relations",
                ]);
                for f in facts {
                    let rel_count = f.relations.len();
                    let rel_info = if rel_count > 0 {
                        format!("{} ties", rel_count)
                    } else {
                        "-".to_string()
                    };
                    table.add_row(vec![
                        f.id,
                        f.category,
                        format!("{:.1}", f.importance),
                        serde_json::to_string(&f.protection)
                            .unwrap_or_else(|_| "\"normal\"".to_string())
                            .trim_matches('"')
                            .to_string(),
                        f.content,
                        rel_info,
                    ]);
                }
                Ok(format!(
                    "Core Memory (Distilled Knowledge Graph):\n\n{}",
                    table.render()
                ))
            }
            "delete" => {
                let (id, same_slot) = match args.fact_id {
                    Some(id) if !id.trim().is_empty() => {
                        let facts = self
                            .memory
                            .retrieve_facts(user_id, agent_id.as_deref())
                            .await?;
                        let same_slot = facts.iter().find(|fact| fact.id == id).and_then(|fact| {
                            Self::fact_slot_key(&fact.content)
                                .map(|slot| (fact.category.clone(), slot))
                        });
                        (id, same_slot)
                    }
                    _ => {
                        let content = args.content.ok_or_else(|| Error::ToolArguments {
                            tool_name: self.name(),
                            message: "Fact ID or content required for delete".into(),
                        })?;
                        let content = content.trim();
                        if content.is_empty() {
                            return Err(Error::ToolArguments {
                                tool_name: self.name(),
                                message: "Fact ID or non-empty content required for delete".into(),
                            }
                            .into());
                        }

                        let facts = self
                            .memory
                            .retrieve_facts(user_id, agent_id.as_deref())
                            .await?;
                        let content_slot = Self::fact_slot_key(content);
                        let matches: Vec<_> = facts
                            .into_iter()
                            .filter(|fact| {
                                fact.content.trim() == content
                                    || fact.content.contains(content)
                                    || content.contains(fact.content.trim())
                                    || content_slot.as_deref()
                                        == Self::fact_slot_key(&fact.content).as_deref()
                            })
                            .collect();

                        match matches.as_slice() {
                            [fact] => (
                                fact.id.clone(),
                                Self::fact_slot_key(&fact.content)
                                    .map(|slot| (fact.category.clone(), slot)),
                            ),
                            [] => {
                                return Err(Error::ToolArguments {
                                    tool_name: self.name(),
                                    message: format!("No fact matched delete content: {}", content),
                                }
                                .into());
                            }
                            many => {
                                let slot_matches: Vec<_> = many
                                    .iter()
                                    .filter(|fact| {
                                        content_slot.as_deref()
                                            == Self::fact_slot_key(&fact.content).as_deref()
                                    })
                                    .collect();
                                if let Some(slot) = content_slot {
                                    if !slot_matches.is_empty() {
                                        let category = slot_matches[0].category.clone();
                                        return {
                                            self.delete_same_slot_facts(
                                                user_id,
                                                agent_id.as_deref(),
                                                &category,
                                                &slot,
                                                None,
                                            )
                                            .await?;
                                            Ok(format!("Facts deleted for slot: {}.", slot))
                                        };
                                    }
                                }
                                let candidates = many
                                    .iter()
                                    .map(|fact| {
                                        format!(
                                            "- id: {}\n  category: {}\n  content: {}",
                                            fact.id, fact.category, fact.content
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                return Err(Error::ToolArguments {
                                    tool_name: self.name(),
                                    message: format!(
                                        "Delete content matched multiple facts. Retry with a fact_id from these candidates:\n{}",
                                        candidates
                                    ),
                                }
                                .into());
                            }
                        }
                    }
                };
                self.memory
                    .delete_fact(user_id, agent_id.as_deref(), &id)
                    .await?;
                let mut deleted = 1usize;
                if let Some((category, slot_key)) = same_slot {
                    deleted += self
                        .delete_same_slot_facts(
                            user_id,
                            agent_id.as_deref(),
                            &category,
                            &slot_key,
                            Some(&id),
                        )
                        .await?;
                }
                Ok(format!("{} fact(s) deleted.", deleted))
            }
            "update_importance" => {
                let id = args.fact_id.ok_or_else(|| Error::ToolArguments {
                    tool_name: self.name(),
                    message: "Fact ID required for update".into(),
                })?;
                let imp = args.importance.ok_or_else(|| Error::ToolArguments {
                    tool_name: self.name(),
                    message: "Importance value (0.0 - 1.0) required".into(),
                })?;
                self.memory
                    .update_fact_importance(user_id, agent_id.as_deref(), &id, imp.clamp(0.0, 1.0))
                    .await?;
                Ok(format!(
                    "Fact {} importance updated to {}.",
                    id,
                    imp.clamp(0.0, 1.0)
                ))
            }
            "find_related" => {
                let id = args.fact_id.ok_or_else(|| Error::ToolArguments {
                    tool_name: self.name(),
                    message: "Fact ID required for find_related".into(),
                })?;
                let depth = args.depth.unwrap_or(2);
                let facts = self
                    .memory
                    .find_related_facts(user_id, agent_id.as_deref(), &id, depth)
                    .await?;

                if facts.is_empty() {
                    return Ok("No related facts found within the specified depth.".into());
                }

                let mut table = benshu_infra::format::MarkdownTable::new(vec![
                    "ID", "Category", "Imp", "Content",
                ]);
                for f in facts {
                    table.add_row(vec![
                        f.id,
                        f.category,
                        format!("{:.1}", f.importance),
                        f.content,
                    ]);
                }
                Ok(format!(
                    "Related Knowledge Nodes (Depth: {}):\n\n{}",
                    depth,
                    table.render()
                ))
            }
            "get_status" => {
                let status = self.memory.get_global_cognitive_status().await?;
                Ok(status)
            }
            "pin"
            | "unpin"
            | "protect"
            | "unprotect"
            | "set_core_identity"
            | "clear_core_identity" => {
                let id = args.fact_id.ok_or_else(|| Error::ToolArguments {
                    tool_name: self.name(),
                    message: "Fact ID required for protection updates".into(),
                })?;
                let target = match args.action.as_str() {
                    "pin" => FactProtection::Pinned,
                    "unpin" => FactProtection::Normal,
                    "protect" => FactProtection::Protected,
                    "unprotect" => FactProtection::Normal,
                    "set_core_identity" => FactProtection::CoreIdentity,
                    "clear_core_identity" => FactProtection::Normal,
                    _ => unreachable!(),
                };

                self.memory
                    .set_fact_protection(user_id, agent_id.as_deref(), &id, target.clone())
                    .await?;
                Ok(format!(
                    "Fact {} protection updated to {}.",
                    id,
                    serde_json::to_string(&target)
                        .unwrap_or_else(|_| "\"normal\"".to_string())
                        .trim_matches('"')
                ))
            }
            _ => Err(Error::ToolArguments {
                tool_name: self.name(),
                message: format!("Invalid action: {}", args.action),
            }
            .into()),
        }
    }
}

/// Tool for tiered search - favor summaries to save tokens
pub struct TieredSearchTool {
    memory: Arc<dyn Memory>,
}

impl TieredSearchTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for TieredSearchTool {
    fn name(&self) -> String {
        "tiered_search".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Search memory and return summaries. Efficient for large datasets. \
                Use this first, then use fetch_document for full content if needed."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "limit": { "type": "integer", "description": "Max results (default: 5)" }
                },
                "required": ["query"]
            }),
            parameters_ts: Some(
                "interface TieredSearchArgs {\n  query: string;\n  limit?: number;\n}".to_string(),
            ),
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            #[serde(default = "default_limit")]
            limit: usize,
        }
        fn default_limit() -> usize {
            5
        }

        let args: Args = serde_json::from_str(arguments)?;
        let results = self
            .memory
            .search("default", None, &args.query, args.limit)
            .await?;

        if results.is_empty() {
            return Ok("No results found.".to_string());
        }

        let mut table = benshu_infra::format::MarkdownTable::new(vec![
            "#",
            "Title",
            "Collection",
            "Path",
            "Summary/Snippet",
        ]);
        for (i, res) in results.iter().enumerate() {
            let info = res
                .summary
                .as_ref()
                .cloned()
                .unwrap_or_else(|| preview_text(&res.content, 150))
                .replace('\n', " ");

            table.add_row(vec![
                (i + 1).to_string(),
                res.title.clone(),
                res.collection.as_deref().unwrap_or("-").to_string(),
                res.path.as_deref().unwrap_or("-").to_string(),
                info,
            ]);
        }

        Ok(format!("Search results (summarized):\n\n{}\n\nUse `fetch_document` with collection and path for full content.", table.render()))
    }
}

/// Tool for fetching full document content
pub struct FetchDocumentTool {
    memory: Arc<dyn Memory>,
}

impl FetchDocumentTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }
}

/// Tool for writing multimodal summaries and generation provenance into governed memory.
pub struct MultimodalMemoryTool {
    memory: Arc<dyn Memory>,
}

impl MultimodalMemoryTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MultimodalMemoryTool {
    fn name(&self) -> String {
        "multimodal_memory_writeback".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Persist multimodal understanding summaries or generation provenance into governed memory. Use this after analyzing an image/video/audio/document or after creating an image artifact.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["understanding", "generation_provenance"] },
                    "modality": { "type": "string", "description": "image, video, audio, pdf, document, or mixed" },
                    "title": { "type": "string", "description": "Short mnemonic title" },
                    "summary": { "type": "string", "description": "Short durable summary for retrieval" },
                    "content": { "type": "string", "description": "Full durable content body" },
                    "collection": { "type": "string", "description": "Target durable collection", "default": "multimodal" },
                    "source_path": { "type": "string" },
                    "source_url": { "type": "string" },
                    "route": { "type": "string" },
                    "model": { "type": "string" },
                    "prompt": { "type": "string" },
                    "artifact_locator": { "type": "string" },
                    "transient": { "type": "boolean", "default": false },
                    "derived_fact_content": { "type": "string" },
                    "derived_fact_category": { "type": "string" },
                    "derived_fact_verified": { "type": "boolean", "default": false },
                    "derived_fact_importance": { "type": "number" },
                    "metadata": {
                        "type": "object",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["kind", "modality", "title", "summary", "content"]
            }),
            parameters_ts: Some("interface MultimodalMemoryArgs {\n  kind: 'understanding' | 'generation_provenance';\n  modality: string;\n  title: string;\n  summary: string;\n  content: string;\n  collection?: string;\n  source_path?: string;\n  source_url?: string;\n  route?: string;\n  model?: string;\n  prompt?: string;\n  artifact_locator?: string;\n  transient?: boolean;\n  derived_fact_content?: string;\n  derived_fact_category?: string;\n  derived_fact_verified?: boolean;\n  derived_fact_importance?: number;\n  metadata?: Record<string, string>;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this to turn multimodal understanding or generation output into governed memory. Prefer concise summaries; store references and provenance rather than giant raw payloads.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            kind: String,
            modality: String,
            title: String,
            summary: String,
            content: String,
            #[serde(default = "default_collection")]
            collection: String,
            source_path: Option<String>,
            source_url: Option<String>,
            route: Option<String>,
            model: Option<String>,
            prompt: Option<String>,
            artifact_locator: Option<String>,
            #[serde(default)]
            transient: bool,
            derived_fact_content: Option<String>,
            derived_fact_category: Option<String>,
            #[serde(default)]
            derived_fact_verified: bool,
            derived_fact_importance: Option<f32>,
            #[serde(default)]
            metadata: HashMap<String, String>,
        }
        fn default_collection() -> String {
            "multimodal".to_string()
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
            tool_name: self.name(),
            message: e.to_string(),
        })?;

        let kind = match args.kind.as_str() {
            "understanding" => MultimodalMemoryKind::Understanding,
            "generation_provenance" => MultimodalMemoryKind::GenerationProvenance,
            other => {
                return Err(Error::ToolArguments {
                    tool_name: self.name(),
                    message: format!("Unsupported multimodal kind: {}", other),
                }
                .into())
            }
        };

        let derived_fact = match (args.derived_fact_content, args.derived_fact_category) {
            (Some(content), Some(category)) if !content.trim().is_empty() => {
                Some(MultimodalDerivedFact {
                    content,
                    category,
                    importance: args.derived_fact_importance.unwrap_or(0.6).clamp(0.0, 1.0),
                    verified: args.derived_fact_verified,
                })
            }
            _ => None,
        };

        let record = MultimodalMemoryRecord {
            kind,
            modality: args.modality,
            title: args.title,
            summary: args.summary,
            content: args.content,
            collection: args.collection,
            source_path: args.source_path,
            source_url: args.source_url,
            route: args.route,
            model: args.model,
            prompt: args.prompt,
            artifact_locator: args.artifact_locator,
            transient: args.transient,
            derived_fact,
            metadata: args.metadata,
        };

        let document = self
            .memory
            .store_multimodal_memory("default", None, record)
            .await?;

        Ok(format!(
            "Multimodal memory recorded in collection '{}' at path '{}'.",
            document.collection.as_deref().unwrap_or("multimodal"),
            document.path.as_deref().unwrap_or("unknown")
        ))
    }
}

#[async_trait]
impl Tool for FetchDocumentTool {
    fn name(&self) -> String {
        "fetch_document".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Retrieve the full content of a document by its collection and path."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "collection": { "type": "string", "description": "Document collection" },
                    "path": { "type": "string", "description": "Document virtual path" }
                },
                "required": ["collection", "path"]
            }),
            parameters_ts: Some(
                "interface FetchArgs {\n  collection: string;\n  path: string;\n}".to_string(),
            ),
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            collection: String,
            path: String,
        }
        let parsed: serde_json::Value = serde_json::from_str(arguments)?;
        let collection = parsed
            .get("collection")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let path = parsed
            .get("path")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let (Some(collection), Some(path)) = (collection, path) else {
            return Ok(serde_json::json!({
                "status": "blocked",
                "error_kind": "missing_required_argument",
                "message": "fetch_document requires both `collection` and `path` from a knowledge/search result receipt.",
                "missing": {
                    "collection": collection.is_none(),
                    "path": path.is_none()
                },
                "example_shape": {
                    "collection": "references",
                    "path": "web/example-document"
                },
                "next_step_hint": "Run tiered_search or inspect the knowledge import receipt to obtain collection/path, then call fetch_document again with both fields."
            })
            .to_string());
        };
        let args = Args {
            collection: collection.to_string(),
            path: path.to_string(),
        };

        let doc = self
            .memory
            .fetch_document(&args.collection, &args.path)
            .await?;
        match doc {
            Some(d) => {
                // Phase 14: Reward fetching (explicit utility)
                let _ = self
                    .memory
                    .update_utility(&args.collection, &args.path, 0.1)
                    .await;
                Ok(format!("# {}\n\n{}", d.title, d.content))
            }
            None => Ok("Document not found.".to_string()),
        }
    }
}
