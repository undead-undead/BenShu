use benshu_hardness::{
    classify_extended_pre_flight_level as classify_pre_flight_level_core,
    extended_pre_flight_allows_auto_stepdown as extended_pre_flight_allows_auto_stepdown_core,
    extended_pre_flight_runs_complexity_estimator as extended_pre_flight_runs_complexity_estimator_core,
    extended_pre_flight_runs_jit_distillation as extended_pre_flight_runs_jit_distillation_core,
    should_run_extended_pre_flight_for_turn as should_run_extended_pre_flight_for_turn_core,
    PreFlightRouteClass,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub use benshu_hardness::ExtendedPreFlightLevel;
pub use benshu_infra::traits::tool::{ToolCatalogEntry, ToolCatalogOverride};

use crate::agent::protocol::ReasoningStrategy;
use crate::error::Error;
pub use benshu_infra::agent::SafetyLevel;
pub use benshu_infra::traits::tool::{Tool, ToolDefinition};
pub use benshu_routing::{
    route_reason_for_plan, WebVerificationAnswerReadiness, WebVerificationContinuation,
    WebVerificationDecision, WebVerificationOrchestrator, WebVerificationRouteReason,
    WebVerificationTermination,
};

// Tool implementations moved to 'builtin-tools' crate.

/// Helper for macros to generate JSON schema from a type
pub fn generate_schema<T: schemars::JsonSchema>() -> serde_json::Value {
    let gen = schemars::gen::SchemaSettings::openapi3().into_generator();
    let schema = gen.into_root_schema_for::<T>();
    let value = serde_json::to_value(schema).unwrap_or(serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    }));
    flatten_schema_refs(value)
}

fn flatten_schema_refs(mut schema: serde_json::Value) -> serde_json::Value {
    let mut definitions = serde_json::Map::new();
    if let Some(obj) = schema.as_object_mut() {
        if let Some(value) = obj
            .remove("definitions")
            .and_then(|v| v.as_object().cloned())
        {
            definitions.extend(value);
        }
        if let Some(components) = obj.remove("components") {
            if let Some(schemas) = components.get("schemas").and_then(|v| v.as_object()) {
                for (key, value) in schemas {
                    definitions.insert(key.clone(), value.clone());
                }
            }
        }
    }
    inline_schema_refs(&mut schema, &definitions);
    schema
}

fn inline_schema_refs(
    value: &mut serde_json::Value,
    definitions: &serde_json::Map<String, serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(|v| v.as_str()) {
                if let Some(name) = reference
                    .strip_prefix("#/definitions/")
                    .or_else(|| reference.strip_prefix("#/components/schemas/"))
                {
                    if let Some(replacement) = definitions.get(name) {
                        let mut cloned = replacement.clone();
                        inline_schema_refs(&mut cloned, definitions);
                        *value = cloned;
                        return;
                    }
                }
            }

            for child in map.values_mut() {
                inline_schema_refs(child, definitions);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                inline_schema_refs(child, definitions);
            }
        }
        _ => {}
    }
}

fn normalize_arguments_string_from_definition(
    definition: &ToolDefinition,
    arguments: &str,
) -> Option<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(arguments).ok()?;
    let normalized = normalize_action_shorthand_from_definition(definition, parsed);
    serde_json::to_string(&normalized).ok()
}

fn normalize_action_shorthand_from_definition(
    definition: &ToolDefinition,
    args: serde_json::Value,
) -> serde_json::Value {
    let serde_json::Value::Object(mut object) = args else {
        return args;
    };
    if object
        .get("action")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.trim().is_empty())
    {
        return serde_json::Value::Object(object);
    }
    if !schema_requires_action(&definition.parameters) {
        return serde_json::Value::Object(object);
    }

    let action_values = action_enum_values(&definition.parameters);
    if action_values.is_empty() {
        return serde_json::Value::Object(object);
    }

    let matched_actions = object
        .keys()
        .filter(|key| action_values.iter().any(|action| action == *key))
        .cloned()
        .collect::<Vec<_>>();
    if matched_actions.len() != 1 {
        return serde_json::Value::Object(object);
    }

    let action = matched_actions[0].clone();
    let shorthand_payload = object.remove(&action);
    if let Some(serde_json::Value::Object(payload)) = shorthand_payload {
        for (key, value) in payload {
            object.entry(key).or_insert(value);
        }
    }
    object.insert("action".to_string(), serde_json::Value::String(action));
    serde_json::Value::Object(object)
}

fn schema_requires_action(schema: &serde_json::Value) -> bool {
    schema
        .get("required")
        .and_then(|value| value.as_array())
        .is_some_and(|required| {
            required
                .iter()
                .any(|value| value.as_str() == Some("action"))
        })
}

fn action_enum_values(schema: &serde_json::Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(|properties| properties.get("action"))
        .and_then(|action| action.get("enum"))
        .and_then(|values| values.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone)]
pub struct ToolSet {
    tools: Arc<parking_lot::RwLock<HashMap<String, Arc<dyn Tool>>>>,
    /// Cached definitions to avoid async calls during prompt generation
    cached_definitions: Arc<parking_lot::RwLock<HashMap<String, ToolDefinition>>>,
    /// Explicit catalog metadata hints for tool entries registered through runtime factory code.
    catalog_overrides: Arc<parking_lot::RwLock<HashMap<String, ToolCatalogOverride>>>,
    /// Optional event bus for tools to emit custom events
    pub(crate) event_tx: Option<tokio::sync::broadcast::Sender<crate::agent::protocol::AgentEvent>>,
    /// Current session ID for event tagging
    pub(crate) session_id: Option<String>,
}

pub use benshu_routing::{
    classify_query_capability_domain, classify_query_capability_route,
    classify_query_verification_plan, classify_query_verification_plan_with_request,
    preferred_capability_domain_for_route, query_requests_routing_judgment_only,
    resolve_capability_route, CapabilityClarificationHint, CapabilityRouteHint,
    CapabilityRouteRequest, CapabilityRouter, QueryVerificationPlan, RealtimeLookupKind,
    SourcePosture, TruthStatus, TruthVerificationPolicyEngine, VerificationDomain,
    VerificationFollowupPlan, VerificationMode, VerificationOutcome, VerificationRequirement,
    VerificationResultEnvelope, VerificationSource,
};

pub use benshu_routing::{
    build_observed_verification_result_envelope, build_pending_verification_followup_plan,
    build_pending_verification_result_envelope, build_search_result_followup_plan,
    build_source_observed_followup_plan, build_verification_followup_plan,
    build_verified_verification_result_envelope,
};

impl Default for ToolSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolSet {
    /// Create an empty toolset
    pub fn new() -> Self {
        Self {
            tools: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            cached_definitions: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            catalog_overrides: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            event_tx: None,
            session_id: None,
        }
    }

    /// Set the event bus and session ID for this toolset
    pub fn with_events(
        mut self,
        tx: tokio::sync::broadcast::Sender<crate::agent::protocol::AgentEvent>,
        session_id: Option<String>,
    ) -> Self {
        self.event_tx = Some(tx);
        self.session_id = session_id;
        self
    }

    /// Emit an event from a tool
    pub fn emit_event(&self, data: crate::agent::protocol::AgentEventData) -> bool {
        if let Some(tx) = &self.event_tx {
            let event = crate::agent::protocol::AgentEvent {
                session_id: self.session_id.clone(),
                data,
            };
            tx.send(event).is_ok()
        } else {
            false
        }
    }

    /// Add a tool to the set
    pub fn add<T: Tool + 'static>(&self, tool: T) -> &Self {
        self.tools
            .write()
            .insert(tool.name().to_string(), Arc::new(tool));
        self
    }

    /// Add a shared tool to the set
    pub fn add_shared(&self, tool: Arc<dyn Tool>) -> &Self {
        self.tools.write().insert(tool.name().to_string(), tool);
        self
    }

    /// Add a shared tool and attach explicit catalog metadata in one registration step.
    pub fn add_shared_with_catalog(
        &self,
        tool: Arc<dyn Tool>,
        catalog_override: ToolCatalogOverride,
    ) -> &Self {
        let name = tool.name();
        self.tools.write().insert(name.clone(), tool);
        self.catalog_overrides
            .write()
            .insert(name, catalog_override);
        self
    }

    pub fn annotate_catalog_entry(
        &self,
        name: impl Into<String>,
        catalog_override: ToolCatalogOverride,
    ) -> &Self {
        self.catalog_overrides
            .write()
            .insert(name.into(), catalog_override);
        self
    }

    pub fn merge_from(&self, other: &ToolSet) -> &Self {
        {
            let mut tools = self.tools.write();
            for (name, tool) in other.iter() {
                tools.insert(name, tool);
            }
        }

        {
            let mut cached = self.cached_definitions.write();
            cached.extend(other.cached_definitions.read().clone());
        }

        {
            let mut overrides = self.catalog_overrides.write();
            overrides.extend(other.catalog_overrides.read().clone());
        }

        self
    }

    pub fn normalize_arguments_from_cached_schema(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> serde_json::Value {
        let Some(definition) = self.cached_definitions.read().get(name).cloned() else {
            return args;
        };
        normalize_action_shorthand_from_definition(&definition, args)
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().get(name).cloned()
    }

    /// Check if a tool exists
    pub fn contains(&self, name: &str) -> bool {
        self.tools.read().contains_key(name)
    }

    /// Get all tool definitions
    pub async fn definitions(&self) -> Vec<ToolDefinition> {
        self.definitions_filtered(None).await
    }

    /// Get tool definitions filtered by an enabled set
    pub async fn definitions_filtered(
        &self,
        enabled: Option<&std::collections::HashSet<String>>,
    ) -> Vec<ToolDefinition> {
        let mut defs = Vec::new();
        let tools_snapshot = self.iter();

        for (name, tool) in tools_snapshot {
            // If filter is provided, skip disabled tools
            if let Some(enabled_set) = enabled {
                if !enabled_set.contains(&name) {
                    continue;
                }
            }

            // Check cache in a small block to ensure guard is dropped
            let cached = { self.cached_definitions.read().get(&name).cloned() };

            if let Some(def) = cached {
                defs.push(def);
            } else {
                let def = tool.definition().await;
                self.cached_definitions.write().insert(name, def.clone());
                defs.push(def);
            }
        }
        defs
    }

    /// Get tool definitions after applying the default deferred prompt/tool-contract filter.
    /// Returns `(visible_definitions, deferred_count)`.
    pub async fn definitions_prompt_visible_filtered(
        &self,
        enabled: Option<&HashSet<String>>,
    ) -> (Vec<ToolDefinition>, usize) {
        let defs = self.definitions_filtered(enabled).await;
        if defs.len() <= 8 {
            return (defs, 0);
        }

        let overrides = self.catalog_overrides.read().clone();
        let entries: Vec<_> = defs
            .iter()
            .cloned()
            .map(|def| {
                let override_hint = overrides.get(&def.name).cloned();
                Self::definition_to_catalog_entry(def, override_hint.as_ref())
            })
            .collect();
        let (visible_entries, deferred_count) = prompt_visible_catalog_entries(&entries);
        if deferred_count == 0 {
            return (defs, 0);
        }

        let visible_names: HashSet<_> = visible_entries
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        let visible_defs = defs
            .into_iter()
            .filter(|def| visible_names.contains(&def.name))
            .collect();
        (visible_defs, deferred_count)
    }

    pub async fn catalog(&self) -> Vec<ToolCatalogEntry> {
        let mut entries: Vec<_> = self
            .definitions()
            .await
            .into_iter()
            .map(|def| {
                let override_hint = self.catalog_overrides.read().get(&def.name).cloned();
                Self::definition_to_catalog_entry(def, override_hint.as_ref())
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    pub async fn search_catalog(&self, query: &str, limit: usize) -> Vec<ToolCatalogEntry> {
        self.search_catalog_with_request(query, limit, CapabilityRouteRequest::default())
            .await
    }

    pub async fn search_catalog_with_request(
        &self,
        query: &str,
        limit: usize,
        request: CapabilityRouteRequest,
    ) -> Vec<ToolCatalogEntry> {
        let normalized_query = query.trim().to_lowercase();
        let tokens = tokenize_query(&normalized_query);
        let router = CapabilityRouter::new(request);
        let route = router.classify_query_route(&normalized_query);
        let desired_capability_domain = route
            .and_then(preferred_capability_domain_for_route)
            .map(str::to_string)
            .or_else(|| infer_query_capability_domain(&normalized_query, &tokens));
        let preferred_tool_names = route
            .map(|value| router.preferred_tool_names(value))
            .unwrap_or_default();
        let limit = limit.max(1);

        let mut matches: Vec<(i32, ToolCatalogEntry)> = self
            .catalog()
            .await
            .into_iter()
            .filter(|entry| entry.name != "tool_search")
            .filter_map(|entry| {
                if normalized_query.is_empty() {
                    return Some((0, entry));
                }

                let score = tool_match_score(
                    &entry,
                    &normalized_query,
                    &tokens,
                    desired_capability_domain.as_deref(),
                    preferred_tool_names,
                );
                if score > 0 {
                    Some((score, entry))
                } else {
                    None
                }
            })
            .collect();

        matches.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
            score_b
                .cmp(score_a)
                .then_with(|| entry_a.name.cmp(&entry_b.name))
        });

        matches
            .into_iter()
            .take(limit)
            .map(|(_, entry)| entry)
            .collect()
    }

    /// Call a tool by name
    pub async fn call(&self, name: &str, arguments: &str) -> anyhow::Result<String> {
        let tool = { self.tools.read().get(name).cloned() }
            .ok_or_else(|| Error::ToolNotFound(name.to_string()))?;

        let cached_definition = { self.cached_definitions.read().get(name).cloned() };
        let definition = if let Some(definition) = cached_definition {
            definition
        } else {
            let definition = tool.definition().await;
            self.cached_definitions
                .write()
                .insert(name.to_string(), definition.clone());
            definition
        };
        let normalized_arguments =
            normalize_arguments_string_from_definition(&definition, arguments)
                .unwrap_or_else(|| arguments.to_string());

        // 1. Run pre-execution hook (Roadmap Phase 6.2: Backup, confirm, etc.)
        tool.pre_call(&normalized_arguments).await?;

        // 2. Execute the tool
        tool.call(&normalized_arguments).await
    }

    /// Get the number of tools
    pub fn len(&self) -> usize {
        self.tools.read().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.tools.read().is_empty()
    }

    /// Iterate over tools
    pub fn iter(&self) -> Vec<(String, Arc<dyn Tool>)> {
        self.tools
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), Arc::clone(v)))
            .collect()
    }

    /// Remove tools matching a pattern (e.g., "fission")
    pub fn remove_by_pattern(&mut self, pattern: &str) {
        let mut tools = self.tools.write();
        let mut cached = self.cached_definitions.write();
        let keys_to_remove: Vec<String> = tools
            .keys()
            .filter(|k| k.contains(pattern))
            .cloned()
            .collect();

        for key in keys_to_remove {
            tools.remove(&key);
            cached.remove(&key);
        }
    }

    fn definition_to_catalog_entry(
        def: ToolDefinition,
        override_hint: Option<&ToolCatalogOverride>,
    ) -> ToolCatalogEntry {
        let name = def.name;
        let mut capability_domain =
            infer_tool_capability_domain(&name, &def.description, def.usage_guidelines.as_deref());
        let mut source = infer_tool_source(&name);
        let mut scope = infer_tool_scope(&name);
        let mut tags = infer_tool_tags(&name, &def.description, def.usage_guidelines.as_deref());

        if let Some(override_hint) = override_hint {
            if let Some(value) = &override_hint.source {
                source = value.clone();
            }
            if let Some(value) = &override_hint.scope {
                scope = value.clone();
            }
            if let Some(value) = &override_hint.capability_domain {
                capability_domain = value.clone();
            }
            for tag in &override_hint.tags {
                if !tags.iter().any(|existing| existing == tag) {
                    tags.push(tag.clone());
                }
            }
        }

        ToolCatalogEntry {
            name,
            description: def.description,
            capability_domain,
            tags,
            source,
            scope,
            usage_guidelines: def.usage_guidelines,
            safety_level: def.safety_level,
            is_binary: def.is_binary,
            is_verified: def.is_verified,
        }
    }
}

pub use benshu_routing::{
    capability_route_debug_label, capability_route_hint_label,
    capability_route_preferred_tool_names, capability_route_requires_real_tool_call,
};

pub use benshu_routing::capability_route_prefers_direct_tool_surface;

fn durable_retrieval_storage_targets() -> &'static [&'static str] {
    &[
        "知识库",
        "资料库",
        "数据库",
        "文档库",
        "素材库",
        "语料库",
        "档案库",
        "检索库",
        "向量库",
        "入库",
        "knowledge base",
        "knowledge-base",
        "database",
        "document store",
        "document-store",
        "document repository",
        "retrieval storage",
        "retrieval store",
        "rag store",
        "vector store",
        "corpus",
        "archive",
    ]
}

fn durable_retrieval_storage_actions() -> &'static [&'static str] {
    &[
        "保存", "存进", "存入", "存到", "写入", "导入", "加入", "放进", "放到", "收进", "收入",
        "入", "收到", "入库", "save", "store", "write", "import", "ingest", "persist", "add to",
        "put into",
    ]
}

fn mentions_durable_retrieval_storage_target(normalized: &str) -> bool {
    durable_retrieval_storage_targets()
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn has_directed_durable_retrieval_storage_request(normalized: &str) -> bool {
    let action_only_requests = ["入库"];
    if action_only_requests.iter().any(|action| {
        normalized.match_indices(action).any(|(action_start, _)| {
            !normalized[..action_start].ends_with('刚')
                && ["把", "将", "请", "帮我", "需要", "并", "然后", "再"]
                    .iter()
                    .any(|lead| {
                        normalized[..action_start]
                            .rfind(lead)
                            .is_some_and(|lead_start| action_start - lead_start <= 120)
                    })
        })
    }) {
        return true;
    }

    durable_retrieval_storage_actions().iter().any(|action| {
        normalized
            .match_indices(action)
            .any(|(action_start, action_text)| {
                let action_end = action_start + action_text.len();
                durable_retrieval_storage_targets().iter().any(|target| {
                    normalized.match_indices(target).any(|(target_start, _)| {
                        target_start >= action_end && target_start - action_end <= 120
                    })
                })
            })
    })
}

fn has_negated_directed_durable_retrieval_storage_request(normalized: &str) -> bool {
    let negative_markers = [
        "不要", "别", "不必", "无需", "do not", "don't", "dont", "without",
    ];

    negative_markers.iter().any(|negative| {
        normalized
            .match_indices(negative)
            .any(|(negative_start, negative_text)| {
                let negative_end = negative_start + negative_text.len();

                ["入库"].iter().any(|action| {
                    normalized.match_indices(action).any(|(action_start, _)| {
                        action_start >= negative_end && action_start - negative_start <= 120
                    })
                }) || durable_retrieval_storage_actions().iter().any(|action| {
                    normalized
                        .match_indices(action)
                        .any(|(action_start, action_text)| {
                            let action_end = action_start + action_text.len();
                            action_start >= negative_end
                                && action_start - negative_start <= 120
                                && durable_retrieval_storage_targets().iter().any(|target| {
                                    normalized.match_indices(target).any(|(target_start, _)| {
                                        target_start >= action_end
                                            && target_start - action_end <= 120
                                    })
                                })
                        })
                })
            })
    })
}

fn query_denies_followup_persistence(normalized: &str) -> bool {
    let explicit_denials = [
        "不要保存到知识库",
        "不要保存进知识库",
        "不要存入知识库",
        "不要写入知识库",
        "不要导入知识库",
        "不要加入知识库",
        "不要入库",
        "别保存到知识库",
        "别保存进知识库",
        "别存入知识库",
        "别写入知识库",
        "别导入知识库",
        "别加入知识库",
        "别入库",
        "不保存到知识库",
        "不保存进知识库",
        "不存入知识库",
        "不写入知识库",
        "不导入知识库",
        "不加入知识库",
        "不入库",
        "do not save to the knowledge base",
        "don't save to the knowledge base",
        "dont save to the knowledge base",
        "do not store this in the knowledge base",
        "don't store this in the knowledge base",
        "dont store this in the knowledge base",
        "do not write this into the knowledge base",
        "don't write this into the knowledge base",
        "dont write this into the knowledge base",
        "do not import this into the knowledge base",
        "don't import this into the knowledge base",
        "dont import this into the knowledge base",
        "without saving to the knowledge base",
        "without writing to the knowledge base",
        "no knowledge base write",
    ];
    if explicit_denials
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return true;
    }

    has_negated_directed_durable_retrieval_storage_request(normalized)
}

pub fn query_requests_followup_execution_after_lookup(query: &str) -> bool {
    let normalized = query.to_lowercase();
    if query_denies_followup_persistence(&normalized) {
        return false;
    }
    let sequencing_markers = [
        "并保存",
        "并且保存",
        "然后保存",
        "再保存",
        "并导入",
        "并且导入",
        "然后导入",
        "再导入",
        "并入库",
        "并且入库",
        "然后入库",
        "再入库",
        "并发送",
        "并且发送",
        "然后发送",
        "再发送",
        "并通知",
        "并且通知",
        "然后通知",
        "再通知",
        "and save",
        "then save",
        "and store",
        "then store",
        "and import",
        "then import",
        "and send",
        "then send",
        "and notify",
        "then notify",
    ];
    let downstream_action_markers = [
        "保存",
        "存进",
        "存入",
        "存到",
        "放进",
        "放到",
        "收进",
        "收入",
        "收到",
        "导入",
        "入库",
        "写入",
        "加入知识库",
        "保存到知识库",
        "存到知识库",
        "通知",
        "发送",
        "发给",
        "同步到",
        "提交给",
        "交给",
        "交由",
        "save",
        "store",
        "import",
        "write into",
        "write to",
        "send",
        "notify",
        "sync to",
        "submit to",
        "hand off to",
    ];

    sequencing_markers
        .iter()
        .any(|marker| normalized.contains(marker))
        || downstream_action_markers
            .iter()
            .any(|marker| normalized.contains(marker))
}

fn query_requests_explicit_worker_delegation(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    if lowered.contains("delegate")
        || lowered.contains("delegate to")
        || lowered.contains(" worker ")
        || lowered.contains(" worker,")
        || lowered.contains(" worker.")
        || lowered.contains(" worker:")
    {
        return true;
    }

    let Some((_, rest)) = query.split_once('让') else {
        return false;
    };
    let target = rest
        .trim_start()
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '，' | ',' | '。' | '：' | ':'))
        .next()
        .unwrap_or_default()
        .trim();
    !target.is_empty()
        && target.len() <= 48
        && target
            .chars()
            .any(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

pub fn capability_route_prefers_direct_tool_surface_for_query(
    route: CapabilityRouteHint,
    query: &str,
) -> bool {
    capability_route_prefers_direct_tool_surface(route)
        && !query_requests_followup_execution_after_lookup(query)
}

pub fn capability_route_tool_allowlist_for_query(
    route: CapabilityRouteHint,
    query: Option<&str>,
) -> HashSet<String> {
    if query.is_some_and(query_requests_explicit_worker_delegation) {
        return coordinator_default_tool_names_for_query(query);
    }

    if matches!(route, CapabilityRouteHint::Memory) {
        if let Some(query) = query {
            return capability_route_preferred_tool_names_for_query(route, query)
                .iter()
                .map(|name| (*name).to_string())
                .collect();
        }
    }

    if matches!(route, CapabilityRouteHint::CapabilityGap)
        && query.is_some_and(query_requests_image_generation)
    {
        return ["delegate", "shared_board", "tool_search"]
            .into_iter()
            .map(str::to_string)
            .collect();
    }

    let mut allowed: HashSet<String> =
        if capability_route_prefers_direct_tool_surface_for_query(route, query.unwrap_or_default())
        {
            let mut direct: HashSet<String> =
                capability_route_preferred_tool_names_for_query(route, query.unwrap_or_default())
                    .iter()
                    .map(|name| (*name).to_string())
                    .collect();
            if capability_route_requires_real_tool_call(route)
                && !matches!(route, CapabilityRouteHint::RealtimeLookup(_))
            {
                direct.insert("delegate".to_string());
            }
            direct
        } else {
            coordinator_default_tool_names_for_query(query)
        };

    if let Some(query) = query {
        if matches!(route, CapabilityRouteHint::RealtimeLookup(_))
            && query_requests_followup_execution_after_lookup(query)
        {
            allowed.extend(
                capability_route_preferred_tool_names_for_query(route, query)
                    .iter()
                    .map(|name| (*name).to_string()),
            );
        }
    }

    allowed
}

pub fn coordinator_default_tool_names() -> &'static [&'static str] {
    &[
        "delegate",
        "handover",
        "shared_board",
        "tool_search",
        "read_skill_manual",
        "read_skill_asset",
        "search_history",
        "remember_this",
        "manage_facts",
    ]
}

pub fn coordinator_default_tool_names_for_query(query: Option<&str>) -> HashSet<String> {
    let mut allowed: HashSet<String> = coordinator_default_tool_names()
        .iter()
        .map(|name| (*name).to_string())
        .collect();

    if let Some(query) = query {
        if query_prefers_knowledge_base_retrieval(query) {
            allowed.remove("handover");
            allowed.remove("manage_facts");
            allowed.remove("shared_board");
            allowed.remove("tool_search");
            allowed.remove("read_skill_manual");
            allowed.remove("read_skill_asset");
            allowed.remove("remember_this");
            allowed.remove("search_history");
            allowed.remove("knowledge_search");
            allowed.remove("tiered_search");
            allowed.remove("fetch_document");
            allowed.insert("delegate".to_string());
        }

        if query_requests_memory(query, &tokenize_query(query))
            && !query_requests_memory_write(query)
            && !query_requests_fact_management(query)
        {
            allowed.remove("handover");
            allowed.remove("manage_facts");
            allowed.remove("remember_this");
            allowed.remove("shared_board");
            allowed.remove("tool_search");
            allowed.remove("read_skill_manual");
            allowed.remove("read_skill_asset");
        }
    }

    allowed
}

pub fn coordinator_chat_lite_tool_names_for_query(query: Option<&str>) -> HashSet<String> {
    let mut allowed: HashSet<String> = HashSet::new();

    let Some(query) = query else {
        return allowed;
    };

    // Immediate same-session recall should use active chat context, not durable memory tools.
    if query_prefers_session_continuity_answer(query) {
        return allowed;
    }

    if query_requests_image_generation(query) {
        allowed.insert("delegate".to_string());
        allowed.insert("shared_board".to_string());
        allowed.insert("tool_search".to_string());
        return allowed;
    }

    if query_prefers_knowledge_base_retrieval(query) {
        allowed.insert("delegate".to_string());
        return allowed;
    }

    if query_requests_fact_management(query) {
        allowed.insert("manage_facts".to_string());
        return allowed;
    }

    if query_requests_memory_write(query) {
        allowed.insert("remember_this".to_string());
        return allowed;
    }

    if query_requests_memory(query, &tokenize_query(query)) {
        allowed.insert("search_history".to_string());
    }

    allowed
}

pub fn capability_route_preferred_tool_names_for_query(
    route: CapabilityRouteHint,
    query: &str,
) -> Vec<&'static str> {
    let lowered_query = query.to_lowercase();
    let asks_browser_worker = lowered_query.contains("browser worker")
        || lowered_query.contains("browser specialist")
        || lowered_query.contains("浏览器 worker")
        || lowered_query.contains("浏览器 specialist")
        || lowered_query.contains("浏览器专员")
        || lowered_query.contains("委托 browser")
        || lowered_query.contains("委托浏览器");
    if matches!(
        route,
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch)
            | CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup)
    ) && asks_browser_worker
    {
        return vec!["delegate", "tool_search"];
    }

    if matches!(route, CapabilityRouteHint::Memory) && query_prefers_knowledge_base_retrieval(query)
    {
        return vec!["delegate"];
    }

    if matches!(route, CapabilityRouteHint::Memory) {
        if query_requests_fact_management(query) {
            return vec!["manage_facts", "search_history"];
        }
        if query_requests_memory_write(query) {
            return vec!["remember_this", "search_history"];
        }
        return vec!["search_history"];
    }

    if matches!(route, CapabilityRouteHint::RealtimeLookup(_))
        && query_requests_followup_execution_after_lookup(query)
    {
        let mut ordered = Vec::new();
        for name in ["delegate", "shared_board", "tool_search"] {
            if !ordered.contains(&name) {
                ordered.push(name);
            }
        }
        for name in realtime_lookup_web_tool_order() {
            if !ordered.contains(&name) {
                ordered.push(name);
            }
        }
        return ordered;
    }

    if let CapabilityRouteHint::RealtimeLookup(kind) = route {
        return realtime_lookup_tool_order(kind);
    }

    capability_route_preferred_tool_names(route).to_vec()
}

fn realtime_lookup_web_tool_order() -> Vec<&'static str> {
    vec!["web_search", "web_fetch", "browser_browse", "tool_search"]
}

fn realtime_lookup_tool_order(kind: RealtimeLookupKind) -> Vec<&'static str> {
    match kind {
        RealtimeLookupKind::PriceLookup => vec!["price_lookup"],
        RealtimeLookupKind::FxLookup => vec!["fx_lookup"],
        RealtimeLookupKind::WeatherLookup => vec!["weather_lookup"],
        RealtimeLookupKind::LatestInfoLookup => vec!["latest_info_lookup"],
        RealtimeLookupKind::WebSearch => realtime_lookup_web_tool_order(),
    }
}

pub use benshu_routing::capability_route_requires_source_fetch;

pub fn capability_route_clarification_message(
    query: &str,
    route: CapabilityRouteHint,
) -> Option<String> {
    let router = CapabilityRouter::default();
    match router.clarification_hint(query) {
        Some(CapabilityClarificationHint::MissingPriceTarget) => {
            build_realtime_lookup_clarification_message(RealtimeLookupKind::PriceLookup)
        }
        Some(CapabilityClarificationHint::MissingFxPair) => {
            build_realtime_lookup_clarification_message(RealtimeLookupKind::FxLookup)
        }
        Some(CapabilityClarificationHint::MissingWeatherLocation) => {
            build_realtime_lookup_clarification_message(RealtimeLookupKind::WeatherLookup)
        }
        None => match route {
            CapabilityRouteHint::RealtimeLookup(kind) => {
                build_realtime_lookup_clarification_message(kind)
            }
            _ => None,
        },
    }
}

pub fn capability_route_tool_required_failure_message(route: CapabilityRouteHint) -> String {
    match route {
        CapabilityRouteHint::DocumentUnderstanding => {
            "这次没有成功委派合适的文档 specialist，也没有成功进入可用的文档/多模态执行面，所以我先不猜测附件内容。你可以稍后重试，或把目标说得更明确一些让我重新分派。"
                .to_string()
        }
        CapabilityRouteHint::FileOps => {
            "这次没有成功调用文件系统工具，所以我先不假装已经读到、列出或修改了文件内容。你可以稍后重试，或把路径和操作说得更具体一些让我重新执行。"
                .to_string()
        }
        CapabilityRouteHint::Writing => {
            "这次没有成功委派写作 specialist，也没有获得可写产物或连续性维护的运行时证据，所以我先不假装已经完成了受治理的写作任务。你可以稍后重试，或把写作目标、长度和保存要求说得更具体一些让我重新执行。"
                .to_string()
        }
        CapabilityRouteHint::RealtimeLookup(kind) => {
            build_realtime_lookup_tool_required_failure_message(kind)
        }
        CapabilityRouteHint::RuntimeSurface => {
            "这次没有成功调用运行时工具，所以我先不编造脚本输出、安装结果或执行状态。你可以稍后重试，或把任务说得更具体一些让我重新执行。"
                .to_string()
        }
        CapabilityRouteHint::ExternalCliTools => {
            "这次没有成功调用外部程序的 CLI 工具，所以我先不编造分支状态、浏览器动作或命令输出。你可以稍后重试，或把任务说得更具体一些让我重新执行。"
                .to_string()
        }
        _ => "这次没有成功进入预期的能力路由执行面，所以我先不猜测结果。你可以稍后重试。"
            .to_string(),
    }
}

pub fn capability_route_fetch_required_failure_message(
    route: CapabilityRouteHint,
) -> Option<String> {
    match route {
        CapabilityRouteHint::RealtimeLookup(kind)
            if capability_route_requires_source_fetch(route) =>
        {
            Some(build_realtime_lookup_fetch_required_failure_message(kind))
        }
        _ => None,
    }
}

pub fn capability_route_system_message(
    user_request: &str,
    route: CapabilityRouteHint,
    media_summary: Option<&str>,
    matched_skill_manual: Option<&str>,
) -> Option<String> {
    match route {
        CapabilityRouteHint::DocumentUnderstanding => Some(
            build_document_hard_route_system_message(user_request, media_summary.unwrap_or("none")),
        ),
        CapabilityRouteHint::FileOps => {
            Some(build_file_ops_hard_route_system_message(user_request))
        }
        CapabilityRouteHint::Writing => {
            Some(build_writing_coordinator_route_system_message(user_request))
        }
        CapabilityRouteHint::RealtimeLookup(kind) => Some(
            build_realtime_lookup_hard_route_system_message(user_request, kind),
        ),
        CapabilityRouteHint::RuntimeSurface => Some(
            build_runtime_surface_hard_route_system_message(user_request, matched_skill_manual),
        ),
        CapabilityRouteHint::ExternalCliTools => Some(
            build_external_cli_tools_hard_route_system_message(user_request),
        ),
        CapabilityRouteHint::Coding => {
            Some(build_coding_coordinator_route_system_message(user_request))
        }
        CapabilityRouteHint::Communication => Some(
            build_communication_coordinator_route_system_message(user_request),
        ),
        CapabilityRouteHint::Memory => {
            Some(build_memory_coordinator_route_system_message(user_request))
        }
        CapabilityRouteHint::CapabilityGap => Some(
            build_capability_gap_coordinator_route_system_message(user_request),
        ),
        _ => None,
    }
}

pub use benshu_routing::capability_route_should_inject_system_message;

pub use benshu_routing::CoordinatorTaskMode;

pub use benshu_routing::{
    coordinator_routing_judgment_only_message, coordinator_task_mode_label,
    coordinator_task_mode_should_include_media_followup_prompt,
    coordinator_task_mode_should_include_route_prompt,
    coordinator_task_mode_should_include_tool_index,
    coordinator_task_mode_should_include_truth_guidance, coordinator_task_mode_system_message,
    select_coordinator_task_mode,
};

pub fn coordinator_preferred_specialist_domains(
    mode: CoordinatorTaskMode,
    route: Option<CapabilityRouteHint>,
    query: Option<&str>,
    has_media_followup: bool,
) -> &'static [&'static str] {
    if has_media_followup
        && matches!(
            mode,
            CoordinatorTaskMode::VisionLite | CoordinatorTaskMode::DocumentLite
        )
    {
        return &[];
    }

    match route {
        Some(CapabilityRouteHint::DocumentUnderstanding) if has_media_followup => &[],
        Some(CapabilityRouteHint::DocumentUnderstanding) => &[
            "document_understanding",
            "ocr",
            "image",
            "voice_understanding",
        ],
        Some(CapabilityRouteHint::VisualUnderstanding) => &[],
        Some(CapabilityRouteHint::VoiceUnderstanding) => {
            &["voice_understanding", "document_understanding"]
        }
        Some(CapabilityRouteHint::RuntimeSurface) => &["runtime_surface", "coding"],
        Some(CapabilityRouteHint::ExternalCliTools) => {
            &["external_cli_tools", "runtime_surface", "coding"]
        }
        Some(CapabilityRouteHint::FileOps) => &["file_ops", "coding", "document_understanding"],
        Some(CapabilityRouteHint::Writing) => &["writing"],
        Some(CapabilityRouteHint::Coding) => &["coding", "file_ops", "runtime_surface"],
        Some(CapabilityRouteHint::Communication) => &["communication"],
        Some(CapabilityRouteHint::Memory)
            if query.is_some_and(query_prefers_knowledge_base_retrieval) =>
        {
            &["knowledge"]
        }
        Some(CapabilityRouteHint::Memory) => &["memory"],
        Some(CapabilityRouteHint::CapabilityGap) => &["capability_gap", "coding"],
        Some(CapabilityRouteHint::RealtimeLookup(_)) => &["realtime_lookup"],
        Some(CapabilityRouteHint::General) | None if has_media_followup => &[],
        _ => match mode {
            CoordinatorTaskMode::VisionLite => &[],
            CoordinatorTaskMode::DocumentLite => &["document_understanding"],
            CoordinatorTaskMode::ChatLite | CoordinatorTaskMode::ToolAgent => &[],
        },
    }
}

pub fn coordinator_specialist_selection_message(
    mode: CoordinatorTaskMode,
    route: Option<CapabilityRouteHint>,
    query: Option<&str>,
    has_media_followup: bool,
) -> Option<String> {
    let domains = coordinator_preferred_specialist_domains(mode, route, query, has_media_followup);
    if domains.is_empty() {
        return None;
    }

    let domain_list = domains
        .iter()
        .map(|domain| format!("`{domain}`"))
        .collect::<Vec<_>>()
        .join(" -> ");

    Some(format!(
        "### BENSHU_SPECIALIST_SELECTION\n\
         If this turn needs specialist execution, keep BenShu in coordinator posture and choose the narrowest capability domain first.\n\
         Preferred delegation / execution domains for this turn: {domain_list}\n\
         Use `delegate` when a matching specialist is already known.\n\
         Use `tool_search` only when the correct execution surface or specialist fit is still unclear."
    ))
}

pub fn coordinator_task_mode_should_include_reasoning_prompt(
    mode: CoordinatorTaskMode,
    strategy: &ReasoningStrategy,
) -> bool {
    match mode {
        CoordinatorTaskMode::ToolAgent => matches!(
            strategy,
            ReasoningStrategy::TreeOfThoughts
                | ReasoningStrategy::Reflexion
                | ReasoningStrategy::Planning
        ),
        CoordinatorTaskMode::DocumentLite => {
            matches!(strategy, ReasoningStrategy::Planning)
        }
        CoordinatorTaskMode::ChatLite | CoordinatorTaskMode::VisionLite => false,
    }
}

pub fn classify_extended_pre_flight_level(
    user_request: &str,
    direct_route: Option<CapabilityRouteHint>,
    has_media_input: bool,
) -> ExtendedPreFlightLevel {
    let trimmed = user_request.trim();
    let requires_truth_or_freshness_verification =
        TruthVerificationPolicyEngine::default().should_include_guidance_for_query(trimmed);
    classify_pre_flight_level_core(
        user_request,
        preflight_route_class_from_hint(direct_route),
        has_media_input,
        requires_truth_or_freshness_verification,
    )
}

pub fn should_run_extended_pre_flight_for_turn(
    user_request: &str,
    direct_route: Option<CapabilityRouteHint>,
    has_media_input: bool,
) -> bool {
    should_run_extended_pre_flight_for_turn_core(classify_extended_pre_flight_level(
        user_request,
        direct_route,
        has_media_input,
    ))
}

pub fn extended_pre_flight_runs_complexity_estimator(level: ExtendedPreFlightLevel) -> bool {
    extended_pre_flight_runs_complexity_estimator_core(level)
}

pub fn extended_pre_flight_runs_jit_distillation(level: ExtendedPreFlightLevel) -> bool {
    extended_pre_flight_runs_jit_distillation_core(level)
}

pub fn extended_pre_flight_allows_auto_stepdown(level: ExtendedPreFlightLevel) -> bool {
    extended_pre_flight_allows_auto_stepdown_core(level)
}

fn preflight_route_class_from_hint(route: Option<CapabilityRouteHint>) -> PreFlightRouteClass {
    match route {
        Some(
            CapabilityRouteHint::RealtimeLookup(_)
            | CapabilityRouteHint::RuntimeSurface
            | CapabilityRouteHint::ExternalCliTools,
        ) => PreFlightRouteClass::HighRisk,
        Some(
            CapabilityRouteHint::DocumentUnderstanding
            | CapabilityRouteHint::VisualUnderstanding
            | CapabilityRouteHint::VoiceUnderstanding
            | CapabilityRouteHint::FileOps
            | CapabilityRouteHint::Coding,
        ) => PreFlightRouteClass::Complex,
        _ => PreFlightRouteClass::None,
    }
}

fn build_document_hard_route_system_message(user_request: &str, media_summary: &str) -> String {
    let has_detected_media = !media_summary.trim().is_empty()
        && !media_summary.eq_ignore_ascii_case("none")
        && !media_summary.eq_ignore_ascii_case("unknown");
    let primary_execution_rule = if has_detected_media {
        "- If native multimodal understanding is available in this turn, BenShu should inspect the provided media directly first and answer from that direct result.\n\
         - Only delegate when the user explicitly asks for OCR / PDF extraction / specialist document handling, or when direct multimodal understanding is unavailable / failed.\n"
    } else {
        "- If a narrow document, OCR, PDF, image, or voice specialist worker is available, prefer `delegate` to that worker before broad decomposition or direct tool use.\n\
         - Only fall back to direct document/media tools if no suitable specialist is available and a direct execution surface is actually enabled in this turn.\n"
    };
    format!(
        "### DOCUMENT_HARD_ROUTE\n\
         This turn is a document / multimodal understanding task.\n\
         Original request: {user_request}\n\
         Detected media: {media_summary}\n\
         Coordinator rules:\n\
         - BenShu stays in coordinator posture first. Do not default to acting as the direct document executor.\n\
         {primary_execution_rule}\
         - Use `shared_board` only when delegated document work needs lightweight coordination or synthesis.\n\
         - Use the provided attachment(s) or explicit URL/path from the user as the input source.\n\
         - Do not pretend you already saw, heard, or parsed the attachment.\n\
         - Do not summarize or extract content from an attachment unless a specialist result or direct execution result was actually produced.\n\
         - If no specialist or direct execution path succeeds, explicitly say no document/media execution surface was successfully invoked instead of guessing.\n\
         - Return only a user-facing answer after coordination and any successful execution results are available."
    )
}

fn build_runtime_surface_hard_route_system_message(
    user_request: &str,
    matched_skill_manual: Option<&str>,
) -> String {
    let progressive_loading_rule = matched_skill_manual
        .map(|skill_name| {
            format!(
                "- This request matches the skill `{skill_name}`. Call `read_skill_manual` for that skill before executing runtime steps, unless you already loaded that manual in this turn.\n"
            )
        })
        .unwrap_or_default();
    format!(
        "### RUNTIME_SURFACE_HARD_ROUTE\n\
         This turn is a runtime-surface execution task.\n\
         Original request: {user_request}\n\
         Execution rules:\n\
         - If a narrow terminal, repo, or execution specialist worker is available, prefer `delegate` to that worker before broad decomposition.\n\
         - Use `shared_board` only when delegated runtime work needs lightweight coordination or summarization.\n\
         - You must use an existing runtime-surface tool in this turn before answering.\n\
         {progressive_loading_rule}\
         - Prefer tools that execute through BenShu-managed runtimes such as `shell`, `uv`, `pixi`, `bun`, `gcc`, or another runtime-aware tool surfaced by the registry.\n\
         - If you are not sure which concrete tool fits best, call `tool_search` first and then choose the matched runtime-aware tool.\n\
         - Do not pretend a script, build, install, compile step, directory listing, or execution output was observed unless a real tool call succeeded.\n\
         - If the runtime-aware tool is unavailable or the tool call fails, explicitly say the runtime tool was not successfully invoked instead of guessing.\n\
         - Return only a user-facing answer after the tool step."
    )
}

fn build_file_ops_hard_route_system_message(user_request: &str) -> String {
    format!(
        "### FILE_OPS_HARD_ROUTE\n\
         This turn is a filesystem access task.\n\
         Original request: {user_request}\n\
         Execution rules:\n\
         - If you are the frontstage coordinator and a narrow file, code, or document specialist worker is available, prefer `delegate` to that worker before broad decomposition.\n\
         - Use `shared_board` to coordinate or summarize delegated file work when needed.\n\
         - You must use a real filesystem tool in this turn before answering.\n\
         - Prefer `read_file` for reading file contents, `list_dir` for listing directories, `edit_file` for targeted edits, and `write_file` for writing new content.\n\
         - If you are not sure which filesystem tool fits best, call `tool_search` first and then choose the matched filesystem tool.\n\
         - Do not pretend you already inspected a file path, directory listing, or file contents unless a real filesystem tool call succeeded.\n\
         - If the filesystem tool is unavailable or the tool call fails, explicitly say the filesystem tool was not successfully invoked instead of guessing.\n\
         - Return only a user-facing answer after the tool step."
    )
}

fn build_external_cli_tools_hard_route_system_message(user_request: &str) -> String {
    format!(
        "### EXTERNAL_CLI_TOOLS_HARD_ROUTE\n\
         This turn is a CLI / command execution task for an external program CLI task.\n\
         Original request: {user_request}\n\
         Execution rules:\n\
         - Prefer `delegate` to the narrowest matching specialist worker before broad decomposition.\n\
         - Use `shared_board` only when multiple delegated steps need coordination.\n\
         - You must use an existing external-CLI-capable tool in this turn before answering.\n\
         - Prefer existing adapters or wrappers for concrete program CLIs such as `git`, browser CLIs, media CLIs, or another external CLI tool surfaced by the registry.\n\
         - If you are not sure which concrete CLI tool fits best, call `tool_search` first and then choose the matched external CLI tool.\n\
         - Do not pretend a command, branch, file listing, browser action, media conversion, or execution output was observed unless a real tool call succeeded.\n\
         - If the external CLI tool is unavailable or the tool call fails, explicitly say the external CLI tool was not successfully invoked instead of guessing.\n\
         - Return only a user-facing answer after the tool step."
    )
}

fn build_realtime_lookup_hard_route_system_message(
    user_request: &str,
    kind: RealtimeLookupKind,
) -> String {
    let preferred_tools = capability_route_preferred_tool_names_for_query(
        CapabilityRouteHint::RealtimeLookup(kind),
        user_request,
    )
    .iter()
    .map(|tool| format!("`{tool}`"))
    .collect::<Vec<_>>()
    .join(" -> ");
    let now_local = chrono::Local::now();
    let absolute_date = now_local.format("%Y-%m-%d").to_string();
    let absolute_timestamp = now_local.format("%Y-%m-%d %H:%M:%S %Z").to_string();
    let query_rewrite_hint =
        build_realtime_lookup_query_rewrite_hint(user_request, kind, now_local);
    let has_followup_execution = query_requests_followup_execution_after_lookup(user_request);
    let (header, task_label, extra_rules, source_priority_rules) = match kind {
        RealtimeLookupKind::WebSearch => (
            "SEARCH_HARD_ROUTE",
            "web search / lookup",
            "- Prefer `web_search` directly when available.\n\
             - When search returns a directly relevant URL and the answer needs more than a snippet, call `web_fetch` on the best source before finalizing.\n\
             - If the request uses relative time such as today/latest/recent/current, rewrite the query with the absolute date before searching.\n\
             - If you need help discovering the right tool first, call `tool_search` and then use the matched search tool.\n\
             - Do not simulate browsing.\n\
             - Do not present remembered or estimated data as if it were fresh search results.",
            "- Prefer official, primary, or directly authoritative pages when the query is factual.\n\
             - If no clearly authoritative page appears, choose the strongest source you can fetch and mention the source in the answer.",
        ),
        RealtimeLookupKind::PriceLookup => (
            "PRICE_LOOKUP_HARD_ROUTE",
            "real-time price / market lookup",
            "- Prefer the structured `price_lookup` tool directly when available.\n\
             - Use `web_search`/`web_fetch` only as fallback when the structured lookup is unavailable, incomplete, or explicitly needs extra source verification.\n\
             - Use `browser_browse` only when the search/fetch path is blocked or the source requires page observation.\n\
             - Rewrite time-sensitive phrases using the absolute date before searching.\n\
             - Use fresh lookup results for prices, quotes, or market data.\n\
             - Include source or freshness cues when the tool provides them.\n\
             - Do not present remembered or estimated prices as if they were live data.",
            "- Prefer exchange pages, brokerage/market data pages, or clearly quoted market pages over generic blogs or forum posts.\n\
             - If multiple market pages conflict, prefer the page that is clearest about venue, timestamp, and quote currency.",
        ),
        RealtimeLookupKind::FxLookup => (
            "FX_LOOKUP_HARD_ROUTE",
            "real-time foreign-exchange lookup",
            "- Prefer the structured `fx_lookup` tool directly when available.\n\
             - Use `web_search`/`web_fetch` only as fallback when the structured lookup is unavailable, incomplete, or explicitly needs extra source verification.\n\
             - Use `browser_browse` only when the search/fetch path is blocked or the source requires page observation.\n\
             - Rewrite time-sensitive phrases using the absolute date before searching.\n\
             - Use fresh lookup results for exchange rates.\n\
             - Include source or freshness cues when the tool provides them.\n\
             - Do not present remembered or estimated exchange rates as if they were live data.",
            "- Prefer bank, exchange, or clearly quoted currency-converter pages over secondary discussion pages.\n\
             - If multiple pages conflict, prefer the one that most clearly shows the currency pair and timestamp.",
        ),
        RealtimeLookupKind::WeatherLookup => (
            "WEATHER_LOOKUP_HARD_ROUTE",
            "real-time weather lookup",
            "- Prefer the structured `weather_lookup` tool directly when available.\n\
             - Use `web_search`/`web_fetch` only as fallback when the structured lookup is unavailable, incomplete, or explicitly needs extra source verification.\n\
             - Use `browser_browse` only when the search/fetch path is blocked or the source requires page observation.\n\
             - Rewrite time-sensitive phrases using the absolute date before searching.\n\
             - Use fresh lookup results for weather, forecast, temperature, or precipitation.\n\
             - Include location/time assumptions only if the tool result supports them.\n\
             - Do not guess weather conditions from memory.",
            "- Prefer official weather agencies or clearly structured forecast pages over blogs or generic summaries.\n\
             - If multiple forecast pages differ, prefer the source that is clearest about location, time window, and update time.",
        ),
        RealtimeLookupKind::LatestInfoLookup => (
            "LATEST_INFO_HARD_ROUTE",
            "latest-information lookup",
            "- Prefer the structured `latest_info_lookup` tool directly when available.\n\
             - Use `web_search`/`web_fetch` only as fallback when the structured lookup is unavailable, incomplete, or explicitly needs extra source verification.\n\
             - Convert vague relative time words into absolute dates or timestamps before searching.\n\
             - Use fresh search results and prioritize recency.\n\
             - Avoid presenting stale remembered information as if it were current.\n\
             - If the user asks for news or updates, reflect recency in the answer.",
            "- Prefer official announcements or primary reporting first.\n\
             - If you must rely on secondary reporting, fetch the strongest recent source you can and mention it clearly.",
        ),
    };

    format!(
        "### {header}\n\
         This turn is a {task_label} task.\n\
         Absolute date today: {absolute_date}\n\
         Absolute local timestamp now: {absolute_timestamp}\n\
         Original request: {user_request}\n\
         Query rewrite hint:\n\
         {query_rewrite_hint}\n\
         Preferred tool order: {preferred_tools}\n\
         Execution rules:\n\
         - You must use an existing search-capable or lookup-capable tool in this turn before answering.\n\
         {extra_rules}\n\
         {}\n\
         {}\n\
         Source-priority rules:\n\
         {source_priority_rules}\n\
         - If no suitable lookup tool is available or the tool call fails, explicitly say the lookup tool was not successfully invoked instead of guessing.\n\
         - Return only a user-facing answer after the tool step."
        ,
        if has_followup_execution {
            "- This request also includes a downstream action after lookup. After you obtain fresh results, stay in coordinator mode and complete the follow-up execution instead of stopping after only reporting sources."
        } else {
            ""
        },
        if has_followup_execution {
            "- Treat lookup completion as phase 1 only. Even if a lookup tool reports that the answer could be finalized, you must not stop while any downstream execution requested by the user is still pending. Use `delegate` when the follow-up belongs to a specialist.\n\
             - If you call `tool_search` or `delegate`, preserve the full original request including the downstream action. Do not rewrite the task down to only the lookup phase or omit verbs like save/import/send/notify."
        } else {
            ""
        }
    )
}

fn build_realtime_lookup_clarification_message(kind: RealtimeLookupKind) -> Option<String> {
    match kind {
        RealtimeLookupKind::PriceLookup => Some(
            "我可以帮你查实时价格，但需要先确认具体标的。请告诉我你要查的是哪种资产或股票，例如 BTC、ETH、AAPL 或黄金。"
                .to_string(),
        ),
        RealtimeLookupKind::FxLookup => Some(
            "我可以帮你查汇率，但需要先确认币种对。请告诉我具体是哪个兑换哪个，例如“美元兑人民币”或“EUR/USD”。"
                .to_string(),
        ),
        RealtimeLookupKind::WeatherLookup => Some(
            "我可以帮你查天气，但需要先确认地点。请告诉我城市或地区，必要时也可以补充日期，例如“上海明天”或“北京这周末”。"
                .to_string(),
        ),
        _ => None,
    }
}

fn build_realtime_lookup_tool_required_failure_message(kind: RealtimeLookupKind) -> String {
    match kind {
        RealtimeLookupKind::WebSearch => {
            "这次没有成功调用搜索工具，所以我先不编造结果。你可以稍后重试，或换一种更明确的说法让我重新发起搜索。"
                .to_string()
        }
        RealtimeLookupKind::PriceLookup => {
            "这次没有成功调用实时查询工具，所以我先不编造价格或行情数据。你可以稍后重试，或补充更明确的标的和市场让我重新查询。"
                .to_string()
        }
        RealtimeLookupKind::FxLookup => {
            "这次没有成功调用实时查询工具，所以我先不编造汇率数据。你可以稍后重试，或补充具体币种对让我重新查询。"
                .to_string()
        }
        RealtimeLookupKind::WeatherLookup => {
            "这次没有成功调用实时查询工具，所以我先不猜测天气。你可以稍后重试，或补充更明确的地点和时间让我重新查询。"
                .to_string()
        }
        RealtimeLookupKind::LatestInfoLookup => {
            "这次没有成功调用实时查询工具，所以我先不编造最新信息。你可以稍后重试，或换一种更明确的说法让我重新发起查询。"
                .to_string()
        }
    }
}

fn build_realtime_lookup_fetch_required_failure_message(kind: RealtimeLookupKind) -> String {
    match kind {
        RealtimeLookupKind::PriceLookup => {
            "这次虽然触发了搜索，但还没有成功读取到足够可靠的来源页面，所以我先不把价格或行情当成已核实的实时结果。你可以稍后重试，我会重新查询并读取来源页面。"
                .to_string()
        }
        RealtimeLookupKind::FxLookup => {
            "这次虽然触发了搜索，但还没有成功读取到足够可靠的汇率来源页面，所以我先不把汇率当成已核实的实时结果。你可以稍后重试，我会重新查询并读取来源页面。"
                .to_string()
        }
        RealtimeLookupKind::WeatherLookup => {
            "这次虽然触发了搜索，但还没有成功读取到足够可靠的天气来源页面，所以我先不把天气结果当成已核实信息。你可以稍后重试，我会重新查询并读取来源页面。"
                .to_string()
        }
        RealtimeLookupKind::LatestInfoLookup => {
            "这次虽然触发了搜索，但还没有成功读取到足够可靠的来源页面，所以我先不把这些内容当成已核实的最新信息。你可以稍后重试，我会重新查询并读取来源页面。"
                .to_string()
        }
        RealtimeLookupKind::WebSearch => {
            "这次虽然触发了搜索，但还没有成功读取来源页面。你可以稍后重试。".to_string()
        }
    }
}

fn build_writing_coordinator_route_system_message(user_request: &str) -> String {
    format!(
        "### WRITING_COORDINATOR_ROUTE\n\
         This turn looks like a governed writing / long-form composition task.\n\
         Original request: {user_request}\n\
         Frontstage rules:\n\
         - BenShu is the public-facing coordinator, not the long-form writing executor.\n\
         - Prefer `delegate` to the narrowest writing specialist when the task needs a durable title, structure, continuity, revision state, or multi-turn stability.\n\
         - Use `tool_search` only when the correct writing specialist or execution surface is unclear.\n\
         - Preserve the user's requested genre, form, audience, length, continuity constraints, source-use constraints, and save/export requirements in the delegated task.\n\
         - For same-session continuation, preserve the visible conversation context and ask the writer to maintain the existing title, entities, rules, unresolved threads, and current progress instead of re-deciding them.\n\
         - Do not claim a written artifact was saved, exported, audited, or added to a continuity ledger unless a real delegated or tool-backed step reports that runtime evidence.\n\
         - Return a clean user-facing answer after the delegated writing step, including any saved path or blocker when present."
    )
}

fn build_coding_coordinator_route_system_message(user_request: &str) -> String {
    format!(
        "### CODING_COORDINATOR_ROUTE\n\
         This turn looks like a coding / repo / implementation task.\n\
         Original request: {user_request}\n\
         Frontstage rules:\n\
         - You are the public-facing coordinator, not the default heavy executor.\n\
         - Prefer A2A-style delegation using `delegate` to the narrowest specialist worker before reaching for broad execution tools.\n\
         - Prefer narrow execution domains such as `coding`, `file_ops`, `runtime_surface`, `external_cli_tools`, or `document_understanding` when one clearly fits.\n\
         - If the work genuinely spans multiple specialists, coordinate them explicitly with `delegate` and `shared_board` instead of inventing a new decomposition surface.\n\
         - Use `shared_board` to coordinate or summarize delegated work when needed.\n\
         - Only answer directly without delegation if the request is clearly lightweight and does not require heavy tool execution.\n\
         - Do not pretend code was inspected, patched, compiled, or executed unless a real delegated or tool-backed step succeeded.\n\
         - Return a clean user-facing answer that hides internal topology unless the user explicitly asks."
    )
}

fn build_communication_coordinator_route_system_message(user_request: &str) -> String {
    format!(
        "### COMMUNICATION_COORDINATOR_ROUTE\n\
         This turn looks like a communication / outreach task.\n\
         Original request: {user_request}\n\
         Frontstage rules:\n\
         - Prefer the smallest suitable path first: draft or coordinate before heavy execution.\n\
         - Use `mailer` / `notifier` only when the request clearly requires direct sending.\n\
         - If the task is broader than a lightweight message action, prefer `delegate` to the narrowest matching specialist worker first.\n\
         - If the task needs multiple specialists, coordinate them explicitly instead of expanding into a decomposition pass.\n\
         - Do not claim a message, email, or notification was sent unless a real tool or delegated step succeeded.\n\
         - Return only the user-facing communication result or the next missing input."
    )
}

fn build_memory_coordinator_route_system_message(user_request: &str) -> String {
    format!(
        "### MEMORY_COORDINATOR_ROUTE\n\
         This turn looks like a memory / recall / knowledge-management task.\n\
         Original request: {user_request}\n\
         Frontstage rules:\n\
         - Prefer retrieval before fact mutation.\n\
         - For personal memory recall, user preferences, prior conversation recall, or things the user asked you to remember, call `search_history` directly. Do not delegate these personal-memory reads to the `knowledge` worker.\n\
         - For knowledge-base lookup, imported document recall, saved reference lookup, or document/RAG material, delegate to the `knowledge` worker instead of directly calling retrieval tools.\n\
         - Use `remember_this` only when the user explicitly wants something stored as memory.\n\
         - If the user asks to read from the knowledge base, read back a saved document, summarize saved material, or inspect previously ingested references, do not call `remember_this`.\n\
         - Use `manage_facts` for listing, pinning, protecting, or updating curated facts. Do not use it as the first step for a normal knowledge-base lookup.\n\
         - Keep the frontstage answer concise and do not surface raw internal logs or storage implementation details.\n\
         - Do not claim something was remembered, recalled, or updated unless a real memory step succeeded.\n\
         - If the request grows into broader multi-step work, prefer delegation to a narrow specialist over expanding the frontstage tool surface."
    )
}

fn build_capability_gap_coordinator_route_system_message(user_request: &str) -> String {
    format!(
        "### CAPABILITY_GAP_COORDINATOR_ROUTE\n\
         This turn looks like a new-capability / automation / tool-building request.\n\
         Original request: {user_request}\n\
         Frontstage rules:\n\
         - Treat this as a coordination task first, not a default direct-execution task.\n\
         - If the user asks to install, add, enable, configure, or connect a skill/plugin/tool by name or URL, delegate to the `skill_manager` worker first.\n\
         - If the user only provides a skill/plugin/tool name, the first `skill_manager` step must resolve candidate sources and ask the user to confirm before installation.\n\
         - Prefer `delegate` to the narrowest specialist worker that can build the needed capability.\n\
         - If the work needs staged implementation, keep BenShu in coordinator posture and chain the needed specialists explicitly.\n\
         - Use `shared_board` if you need to coordinate sub-results.\n\
         - Do not promise that a new tool, worker, plugin, or script already exists unless a real delegated or tool-backed step succeeded.\n\
         - Return a concise user-facing answer that focuses on progress and next action, not internal topology."
    )
}

fn extract_known_currency_codes(user_input: &str) -> Vec<&'static str> {
    let lowered = user_input.to_lowercase();
    let mut positions: Vec<(usize, &'static str)> = Vec::new();
    for (marker, code) in known_currency_markers_for_lookup() {
        let position = lowered.find(marker).or_else(|| user_input.find(marker));
        if let Some(idx) = position {
            positions.push((idx, code));
        }
    }
    positions.sort_by_key(|(idx, _)| *idx);
    let mut seen = std::collections::BTreeSet::new();
    let mut ordered = Vec::new();
    for (_, code) in positions {
        if seen.insert(code) {
            ordered.push(code);
        }
    }
    ordered
}

fn resolve_query_reference_date(
    user_input: &str,
    now_local: chrono::DateTime<chrono::Local>,
) -> String {
    let lowered = user_input.to_lowercase();
    let date = if lowered.contains("tomorrow") || user_input.contains("明天") {
        now_local + chrono::Duration::days(1)
    } else if lowered.contains("day after tomorrow") || user_input.contains("后天") {
        now_local + chrono::Duration::days(2)
    } else {
        now_local
    };
    date.format("%Y-%m-%d").to_string()
}

fn build_realtime_lookup_query_rewrite_hint(
    user_input: &str,
    kind: RealtimeLookupKind,
    now_local: chrono::DateTime<chrono::Local>,
) -> String {
    let reference_date = resolve_query_reference_date(user_input, now_local);
    match kind {
        RealtimeLookupKind::WebSearch => format!(
            "Rewrite the user's request into a concrete search query using the absolute date when recency matters. Example shape: \"<topic> {reference_date}\"."
        ),
        RealtimeLookupKind::PriceLookup => {
            format!(
                "Recommended normalized query shape: \"<asset or symbol> price {reference_date}\". Infer the asset from the user's words; if the user implies a market or quote currency, include it explicitly in the final search query."
            )
        }
        RealtimeLookupKind::FxLookup => {
            let codes = extract_known_currency_codes(user_input);
            let pair = if codes.len() >= 2 {
                format!("{} {}", codes[0], codes[1])
            } else {
                "BASE QUOTE".to_string()
            };
            format!(
                "Recommended normalized query: \"{pair} exchange rate {reference_date}\". Preserve the user's requested direction in the final search query."
            )
        }
        RealtimeLookupKind::WeatherLookup => {
            format!(
                "Recommended normalized query shape: \"<location> weather {reference_date}\". Infer the location from the user's words; if the user asked for forecast details, preserve the requested weather attributes."
            )
        }
        RealtimeLookupKind::LatestInfoLookup => format!(
            "Recommended normalized query shape: \"<topic> latest news {reference_date}\" or \"<topic> update {reference_date}\". Prefer including the concrete date over vague words like latest or recent."
        ),
    }
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter_map(|token| {
            let token = token.trim();
            if token.is_empty() {
                None
            } else {
                Some(token.to_lowercase())
            }
        })
        .collect()
}

fn known_currency_markers_for_lookup() -> &'static [(&'static str, &'static str)] {
    &[
        ("usd", "USD"),
        ("美元", "USD"),
        ("美金", "USD"),
        ("us dollar", "USD"),
        ("cny", "CNY"),
        ("rmb", "CNY"),
        ("人民币", "CNY"),
        ("yuan", "CNY"),
        ("eur", "EUR"),
        ("欧元", "EUR"),
        ("euro", "EUR"),
        ("jpy", "JPY"),
        ("日元", "JPY"),
        ("yen", "JPY"),
        ("hkd", "HKD"),
        ("港币", "HKD"),
        ("港元", "HKD"),
        ("gbp", "GBP"),
        ("英镑", "GBP"),
        ("pound", "GBP"),
        ("aud", "AUD"),
        ("澳元", "AUD"),
        ("cad", "CAD"),
        ("加元", "CAD"),
        ("sgd", "SGD"),
        ("新加坡元", "SGD"),
        ("krw", "KRW"),
        ("韩元", "KRW"),
        ("twd", "TWD"),
        ("台币", "TWD"),
        ("新台币", "TWD"),
    ]
}

fn looks_like_explanatory_query(query: &str) -> bool {
    let explain_markers = [
        "什么是",
        "是什么",
        "什么意思",
        "有啥用",
        "有什么用",
        "介绍一下",
        "解释一下",
        "怎么理解",
        "区别是什么",
        "是什么东西",
        "what is",
        "what's",
        "meaning of",
        "explain",
        "introduce",
        "tell me about",
        "what does",
    ];

    explain_markers.iter().any(|marker| query.contains(marker))
}

fn looks_like_execution_request(query: &str, tokens: &[String]) -> bool {
    let has_token = |needle: &str| tokens.iter().any(|token| token == needle);
    let has_any_token = |needles: &[&str]| needles.iter().any(|needle| has_token(needle));
    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));

    contains_any(&[
        "用", "执行", "运行", "调用", "打开", "列出", "查看", "转换", "编译", "安装", "启动",
        "停止", "导出", "抓取", "检查", "run ", "use ", "execute", "invoke", "open ", "list ",
        "show ", "convert", "build", "install", "launch", "start ", "stop ",
    ]) || has_any_token(&[
        "run", "use", "execute", "invoke", "open", "list", "show", "convert", "build", "install",
        "launch", "start", "stop",
    ])
}

fn query_requests_verification(query: &str) -> bool {
    let markers = [
        "确认",
        "核实",
        "验证",
        "检查",
        "看看有没有",
        "有没有",
        "在不在",
        "存在吗",
        "是否存在",
        "可用吗",
        "是否可用",
        "装了吗",
        "安装了吗",
        "成功了吗",
        "完成了吗",
        "有没有成功",
        "有没有执行",
        "exists",
        "exist",
        "available",
        "installed",
        "ready",
        "verify",
        "confirm",
        "check whether",
    ];

    markers.iter().any(|marker| query.contains(marker))
}

fn looks_like_fact_check_request(query: &str) -> bool {
    let markers = [
        "帮我看",
        "帮我查",
        "查一下",
        "看一下",
        "看下",
        "看看",
        "当前",
        "现在",
        "是否",
        "有没",
        "有没有",
        "可不可用",
        "能不能用",
        "is there",
        "current",
        "right now",
    ];

    query_requests_verification(query) || markers.iter().any(|marker| query.contains(marker))
}

fn query_requests_tool_fact_verification(query: &str) -> bool {
    let tool_markers = [
        "工具",
        "命令",
        "cli",
        "程序",
        "插件",
        "adapter",
        "tool",
        "command",
        "binary",
        "安装",
        "git",
        "ffmpeg",
        "docker",
        "playwright",
    ];

    looks_like_fact_check_request(query) && tool_markers.iter().any(|marker| query.contains(marker))
}

fn query_requests_state_fact_verification(query: &str) -> bool {
    let state_markers = [
        "状态",
        "就绪",
        "连接",
        "在线",
        "可用",
        "host",
        "runtime",
        "模型是否",
        "模型有没有",
        "是否启动",
        "是否已启动",
        "ready",
        "status",
        "connected",
        "running",
        "host_runtime",
    ];

    looks_like_fact_check_request(query)
        && state_markers.iter().any(|marker| query.contains(marker))
}

fn query_requests_execution_fact_verification(query: &str) -> bool {
    let execution_markers = [
        "执行结果",
        "执行成功",
        "未提交改动",
        "git status",
        "shows changes",
        "show changes",
        "改了没",
        "改了没有",
        "文件是否改了",
        "文件改了吗",
        "生成了吗",
        "输出是什么",
        "结果是什么",
        "跑完了吗",
        "有没有执行",
        "有没有成功",
        "是否完成",
        "did it run",
        "did it finish",
        "was it created",
        "execution result",
        "command output",
        "finished",
        "completed",
    ];

    looks_like_fact_check_request(query)
        && execution_markers
            .iter()
            .any(|marker| query.contains(marker))
}

pub fn query_requests_document_understanding(query: &str) -> bool {
    let lowered = query.to_lowercase();
    let explicit_artifact_markers = [
        "pdf",
        "附件",
        "图片",
        "图像",
        "截图",
        "识图",
        "ocr",
        "音频",
        "语音",
        "录音",
        "视频",
        "总结这个pdf",
        "read this pdf",
        "analyze this image",
        "transcribe this audio",
        "summarize this document",
        "extract text",
    ];
    let action_markers = ["帮我看", "帮我读", "帮我解析", "帮我提取"];
    let contextual_artifact_markers = [
        "这个文件",
        "这份文件",
        "上传的文件",
        "该文件",
        "这个文档",
        "这份文档",
        "上传的文档",
        "该文档",
    ];
    let has_explicit_artifact_marker = explicit_artifact_markers
        .iter()
        .any(|marker| lowered.contains(marker) || query.contains(marker));
    let has_contextual_artifact_marker = contextual_artifact_markers
        .iter()
        .any(|marker| lowered.contains(marker) || query.contains(marker));
    let has_action_marker = action_markers.iter().any(|marker| query.contains(marker));

    has_explicit_artifact_marker
        || has_contextual_artifact_marker
        || (has_action_marker
            && ["这个", "这份", "该", "上传", "附件", "图片", "截图", "pdf"]
                .iter()
                .any(|marker| query.contains(marker) || lowered.contains(marker)))
}

pub fn query_requests_image_generation(query: &str) -> bool {
    let lowered = query.to_lowercase();
    let object_query = query
        .replace("插画师", "")
        .replace("插画家", "")
        .replace("illustrator", "");
    let object_lowered = object_query.to_lowercase();
    let understanding_marker_hit = [
        "图片理解",
        "图像理解",
        "识图",
        "看图",
        "读图",
        "分析图片",
        "理解图片",
        "理解图像",
        "image understanding",
        "analyze this image",
        "describe this image",
        "read this image",
        "ocr",
    ]
    .iter()
    .any(|marker| object_lowered.contains(marker) || object_query.contains(marker));
    if understanding_marker_hit {
        return false;
    }

    let direct_marker_hit = [
        "generate image",
        "create image",
        "draw image",
        "make image",
        "image generation",
        "text-to-image",
        "draw me",
        "illustration",
        "poster",
        "logo design",
        "画图",
        "生成图片",
        "做图",
        "文生图",
        "画一张",
        "海报",
        "插画",
        "logo",
        "生成一张图",
    ]
    .iter()
    .any(|marker| object_lowered.contains(marker) || object_query.contains(marker));

    if direct_marker_hit {
        return true;
    }

    let has_generation_verb = ["生成", "画", "做", "帮我生成", "帮我画", "请生成", "请画"]
        .iter()
        .any(|marker| query.contains(marker) || lowered.contains(marker));

    let has_image_object = [
        "图片",
        "图像",
        "配图",
        "海报",
        "插画",
        "封面",
        "壁纸",
        "logo",
        "image",
        "picture",
        "poster",
        "illustration",
        "cover",
        "wallpaper",
    ]
    .iter()
    .any(|marker| object_query.contains(marker) || object_lowered.contains(marker));

    has_generation_verb && has_image_object
}

pub fn query_prefers_session_continuity_answer(query: &str) -> bool {
    let lowered = query.trim().to_lowercase();
    if lowered.is_empty() {
        return false;
    }

    let immediacy_markers = [
        "上一条",
        "上一句",
        "上条",
        "前面那句",
        "前面那条",
        "刚才",
        "刚刚",
        "你刚才",
        "我刚才",
        "我们刚才",
        "上一个回复",
        "上一轮",
        "这轮刚才",
        "当前会话",
        "同会话",
        "这轮会话",
        "本轮会话",
        "只根据当前会话",
        "临时暗号",
        "last message",
        "last reply",
        "previous message",
        "previous reply",
        "earlier in this chat",
        "in this session",
        "current session",
        "same session",
        "what did you just",
        "what did i just",
    ];
    let recall_markers = [
        "是什么",
        "是哪句",
        "哪句话",
        "哪一句",
        "说了什么",
        "总结",
        "概括",
        "主角",
        "路径",
        "保存在哪",
        "保存路径",
        "内容",
        "聊到哪",
        "讲到哪",
        "聊过",
        "聊了",
        "连续聊过",
        "话题",
        "关键词",
        "暗号",
        "让我记住",
        "记住的那句话",
        "记住的那句",
        "what was",
        "which sentence",
        "what did you say",
        "what was that",
        "summarize",
        "summary",
        "who is",
        "where is",
        "saved path",
    ];

    immediacy_markers
        .iter()
        .any(|marker| lowered.contains(&marker.to_lowercase()))
        && recall_markers
            .iter()
            .any(|marker| lowered.contains(&marker.to_lowercase()))
}

fn infer_query_capability_domain(query: &str, tokens: &[String]) -> Option<String> {
    let has_token = |needle: &str| tokens.iter().any(|token| token == needle);
    let query_has_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let has_url = query.contains("http://") || query.contains("https://");
    let has_current_marker = query_has_any(&[
        "当前", "最新", "现任", "current", "latest", "recent", "today",
    ]);
    let has_web_lookup_action = query_has_any(&[
        "查找", "寻找", "检索", "查询", "搜索", "搜", "找", "下载", "lookup", "search", "find",
        "download",
    ]) || tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "lookup" | "search" | "find" | "download" | "web" | "google"
        )
    });
    let has_web_scope_marker = has_url
        || query_has_any(&[
            "公网",
            "网上",
            "网络",
            "网页",
            "网站",
            "站点",
            "链接",
            "公开",
            "互联网",
            "web",
            "online",
            "internet",
            "website",
            "site",
            "url",
        ]);
    let has_policy_marker =
        query_has_any(&["政策", "规则", "法规", "policy", "rule", "regulation"]);
    let mentions_currency_name = query_has_any(&[
        "美元",
        "人民币",
        "欧元",
        "日元",
        "港币",
        "英镑",
        "澳元",
        "加元",
        "新加坡元",
        "韩元",
        "台币",
    ]);
    let has_quantity_question =
        query_has_any(&["多少", "几多", "是多少", "几", "what is", "how much"]);
    let has_market_value_marker = query_has_any(&[
        "价格",
        "币价",
        "股价",
        "报价",
        "行情",
        "点数",
        "指数",
        "股票",
        "基金",
        "期货",
        "加密货币",
        "虚拟货币",
    ]) || tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "price"
                | "quote"
                | "btc"
                | "eth"
                | "stock"
                | "stocks"
                | "equity"
                | "ticker"
                | "crypto"
                | "coin"
                | "token"
                | "index"
                | "indices"
                | "points"
        )
    });
    let has_crypto_quantity_target =
        has_quantity_question && query.contains('币') && !mentions_currency_name;

    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "weather" | "forecast" | "气温"))
        || query_has_any(&["天气", "预报"])
    {
        return Some("realtime_lookup.weather".to_string());
    }

    if has_token("fx")
        || query.contains("汇率")
        || query.contains("exchange rate")
        || query.contains("currency pair")
        || (tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "汇率" | "rate" | "usd" | "cny" | "eur" | "jpy" | "hkd" | "gbp"
            )
        }) && (query.contains("exchange rate")
            || query.contains("currency pair")
            || query.contains("汇率")
            || query.contains("兑")
            || query.contains("to ")
            || mentions_currency_name
            || tokens.len() >= 2 && has_token("rate")))
        || ((query.contains("汇率") || query.contains("兑")) && mentions_currency_name)
    {
        return Some("realtime_lookup.fx".to_string());
    }

    if has_market_value_marker
        || has_crypto_quantity_target
        || (has_quantity_question
            && tokens.iter().any(|token| {
                matches!(
                    token.as_str(),
                    "nasdaq" | "dow" | "sp500" | "s&p" | "nikkei" | "hang" | "seng"
                )
            }))
    {
        return Some("realtime_lookup.price".to_string());
    }

    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "latest" | "news" | "today" | "incumbent"))
        || (has_current_marker && has_policy_marker)
        || query_has_any(&[
            "最近",
            "最新",
            "新闻",
            "现任",
            "当前政策",
            "最新政策",
            "current ceo",
            "current president",
            "current policy",
            "current version",
            "release version",
        ])
    {
        return Some("realtime_lookup.latest_info".to_string());
    }

    if has_url
        && (query_has_any(&[
            "读取", "打开", "浏览", "抓取", "页面", "网页", "标题", "摘要", "read", "open",
            "browse", "fetch", "page", "title", "summary",
        ]) || tokens
            .iter()
            .any(|token| matches!(token.as_str(), "read" | "open" | "browse" | "fetch")))
    {
        return Some("realtime_lookup.web".to_string());
    }

    if (has_web_lookup_action && has_web_scope_marker)
        || tokens
            .iter()
            .any(|token| matches!(token.as_str(), "search" | "web" | "google"))
        || query_has_any(&["网页", "搜索"])
    {
        return Some("realtime_lookup.web".to_string());
    }

    if query_requests_image_generation(query)
        || tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "draw" | "drawing" | "illustration" | "poster" | "logo" | "render"
            )
        })
        || query_has_any(&["画图", "生成图片", "做图", "文生图", "海报", "插画"])
    {
        return Some("image_generation".to_string());
    }

    if query_requests_document_understanding(query)
        || tokens
            .iter()
            .any(|token| matches!(token.as_str(), "pdf" | "document" | "ocr" | "extract"))
    {
        return Some("document_understanding".to_string());
    }

    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "image" | "vision" | "visual"))
        || query_has_any(&["图像", "截图"])
    {
        return Some("document_understanding".to_string());
    }

    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "voice" | "audio" | "speech"))
        || query_has_any(&["语音", "音频"])
    {
        return Some("voice_understanding".to_string());
    }

    if query_requests_tool_fact_verification(query)
        && tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "git"
                    | "ffmpeg"
                    | "docker"
                    | "npm"
                    | "pnpm"
                    | "yarn"
                    | "cargo"
                    | "chrome"
                    | "chromium"
                    | "playwright"
                    | "adb"
                    | "sqlite3"
                    | "ffprobe"
                    | "cli"
            )
        })
    {
        return Some("external_cli_tools".to_string());
    }

    if (query_requests_tool_fact_verification(query)
        || query_requests_state_fact_verification(query))
        && tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "bash"
                    | "powershell"
                    | "pwsh"
                    | "cmd"
                    | "shell"
                    | "terminal"
                    | "uv"
                    | "pixi"
                    | "bun"
                    | "gcc"
                    | "python"
                    | "node"
                    | "quickjs"
            )
        })
    {
        return Some("runtime_surface".to_string());
    }

    if query_requests_state_fact_verification(query)
        && query_has_any(&[
            "系统状态",
            "当前系统状态",
            "宿主状态",
            "运行时状态",
            "环境状态",
        ])
    {
        return Some("runtime_surface".to_string());
    }

    if query_requests_execution_fact_verification(query)
        && (tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "git" | "repo" | "repository" | "branch" | "status"
            )
        }) || query_has_any(&["未提交改动", "工作区", "仓库改动", "git status"]))
    {
        return Some("external_cli_tools".to_string());
    }

    if query_requests_execution_fact_verification(query)
        && query_has_any(&[
            "文件是否改了",
            "文件改了吗",
            "改了没有",
            "改了没",
            "生成了吗",
            "输出是什么",
            "结果是什么",
        ])
    {
        return Some("runtime_surface".to_string());
    }

    if !looks_like_explanatory_query(query)
        && looks_like_execution_request(query, tokens)
        && tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "git"
                    | "ffmpeg"
                    | "docker"
                    | "npm"
                    | "pnpm"
                    | "yarn"
                    | "cargo"
                    | "chrome"
                    | "chromium"
                    | "playwright"
                    | "adb"
                    | "sqlite3"
                    | "ffprobe"
                    | "cli"
            )
        })
        || (!looks_like_explanatory_query(query)
            && looks_like_execution_request(query, tokens)
            && query_has_any(&["分支", "程序自带cli", "程序自带命令"]))
    {
        return Some("external_cli_tools".to_string());
    }

    if !looks_like_explanatory_query(query)
        && looks_like_execution_request(query, tokens)
        && tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "bash"
                    | "powershell"
                    | "pwsh"
                    | "cmd"
                    | "shell"
                    | "terminal"
                    | "uv"
                    | "pixi"
                    | "bun"
                    | "gcc"
                    | "python"
                    | "node"
                    | "quickjs"
            )
        })
        || (!looks_like_explanatory_query(query)
            && looks_like_execution_request(query, tokens)
            && query_has_any(&["命令行", "终端", "脚本运行时"]))
    {
        return Some("runtime_surface".to_string());
    }

    if query_requests_file_ops(query, tokens) {
        return Some("file_ops".to_string());
    }

    if query_requests_capability_gap(query, tokens) {
        return Some("capability_gap".to_string());
    }

    if query_requests_memory(query, tokens) {
        return Some("memory".to_string());
    }

    if query_requests_coding(query, tokens) {
        return Some("coding".to_string());
    }

    if query_requests_communication(query, tokens) {
        return Some("communication".to_string());
    }

    None
}

fn query_requests_coding(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|needle| token == needle))
    };
    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_contains_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };

    let coding_markers = [
        "代码",
        "仓库",
        "repo",
        "repository",
        "bug",
        "commit",
        "patch",
        "feature",
        "pull request",
        "branch",
        "编译",
        "build",
        "cargo",
        "cargo test",
        "pytest",
        "单元测试",
        "集成测试",
        "rust",
        "python",
        "typescript",
        "javascript",
    ];

    let coding_verbs = [
        "写",
        "改",
        "修",
        "实现",
        "开发",
        "重构",
        "加上",
        "补上",
        "测试",
        "提交",
        "优化",
        "排查",
        "write",
        "fix",
        "implement",
        "build",
        "refactor",
        "test",
        "patch",
        "debug",
        "review",
    ];

    (contains_any(&coding_markers)
        || lowered_contains_any(&coding_markers)
        || has_token(&[
            "code",
            "repo",
            "repository",
            "bug",
            "fix",
            "implement",
            "refactor",
            "patch",
            "build",
            "commit",
        ]))
        && (contains_any(&coding_verbs)
            || lowered_contains_any(&coding_verbs)
            || has_token(&[
                "write",
                "fix",
                "implement",
                "build",
                "refactor",
                "test",
                "patch",
                "debug",
                "review",
            ]))
}

fn query_requests_communication(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_contains_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|needle| token == needle))
    };

    let channels = [
        "邮件",
        "邮箱",
        "email",
        "mail",
        "slack",
        "discord",
        "telegram",
        "通知",
        "提醒",
        "消息",
        "notification",
        "message",
    ];
    let verbs = [
        "发送", "发给", "通知", "提醒", "回复", "草拟", "draft", "send", "notify", "reply",
        "message",
    ];

    (contains_any(&channels)
        || lowered_contains_any(&channels)
        || has_token(&[
            "email",
            "mail",
            "slack",
            "discord",
            "telegram",
            "notify",
            "notification",
            "message",
        ]))
        && (contains_any(&verbs)
            || lowered_contains_any(&verbs)
            || has_token(&["send", "notify", "reply", "draft", "message"]))
}

fn query_requests_memory(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let blocks_durable_memory = [
        "不要保存为长期记忆",
        "不要写入长期记忆",
        "不要保存到记忆",
        "不要记住",
        "do not save",
        "don't save",
        "do not remember",
        "don't remember",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()));
    if query_prefers_session_continuity_answer(query) && blocks_durable_memory {
        return false;
    }

    let explicit_memory_marker = [
        "记住",
        "记忆",
        "memory",
        "remember",
        "recall",
        "让你记住",
        "我让你记住",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()));
    if query_prefers_session_continuity_answer(query) && !explicit_memory_marker {
        return false;
    }

    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_contains_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|needle| token == needle))
    };

    contains_any(&[
        "记住",
        "记下来",
        "还记得",
        "记得",
        "回忆",
        "想起来",
        "上次",
        "刚才",
        "之前",
        "以前",
        "历史里",
        "知识库",
        "记忆",
        "之前说过",
        "我让你记住",
        "让你记住",
    ]) || lowered_contains_any(&[
        "remember",
        "recall",
        "memory",
        "history",
        "last time",
        "earlier",
        "previous",
        "previously",
        "knowledge base",
        "previously said",
    ]) || has_token(&["remember", "recall", "memory", "history", "knowledge"])
}

fn query_requests_memory_write(query: &str) -> bool {
    let lowered = query.to_lowercase();
    [
        "记住",
        "记下来",
        "保存到记忆",
        "保存为记忆",
        "写入记忆",
        "remember this",
        "save this",
        "store this",
        "save to memory",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()))
}

fn query_requests_fact_management(query: &str) -> bool {
    let lowered = query.to_lowercase();
    let is_recall_or_check = [
        "查", "找回", "回忆", "读取", "再查", "recall", "retrieve", "look up",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()));
    let is_conditional_delete_mention = ["如果", "是否", "已经删除", "删掉了吗", "if", "whether"]
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()));
    if is_recall_or_check && is_conditional_delete_mention {
        return false;
    }

    let memory_mutation = [
        "删除", "忘记", "更新", "修改", "改成", "delete", "forget", "update", "change",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()))
        && [
            "记忆",
            "记住",
            "记得",
            "验证码",
            "刚才那个",
            "刚才的",
            "那个",
            "memory",
            "remembered",
        ]
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()));
    if memory_mutation {
        return true;
    }

    let mentions_fact_store = [
        "核心事实",
        "事实",
        "core memory",
        "core fact",
        "fact",
        "facts",
        "manage_facts",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()));
    if !mentions_fact_store {
        return false;
    }

    [
        "列出",
        "列表",
        "删除",
        "更新",
        "修改",
        "置顶",
        "保护",
        "取消保护",
        "重要性",
        "list",
        "delete",
        "update",
        "pin",
        "protect",
        "importance",
        "manage",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()))
}

pub(crate) fn query_prefers_knowledge_base_retrieval(query: &str) -> bool {
    let lowered = query.to_lowercase();
    let mentions_knowledge_base = mentions_durable_retrieval_storage_target(&lowered);

    if !mentions_knowledge_base {
        return false;
    }

    let retrieval_markers = [
        "读出",
        "读回",
        "查一下",
        "查找",
        "查询",
        "查出",
        "查回",
        "搜一下",
        "搜索",
        "检索",
        "取出",
        "取回",
        "找出",
        "找回",
        "告诉我",
        "给我",
        "列出",
        "摘要",
        "标题",
        "内容",
        "详情",
        "from the knowledge base",
        "read back",
        "read from",
        "look up",
        "lookup",
        "retrieve",
        "tell me",
        "show me",
        "summary",
        "title",
        "contents",
        "details",
    ];

    let retrieval_context_markers = [
        "从知识库",
        "在知识库",
        "知识库里",
        "知识库中",
        "从资料库",
        "在资料库",
        "资料库里",
        "资料库中",
        "from the knowledge base",
        "in the knowledge base",
    ];

    let mutation_markers = [
        "记住",
        "保存到知识库",
        "存入知识库",
        "写入知识库",
        "加入知识库",
        "更新这条事实",
        "删除这条事实",
        "保护这条事实",
        "置顶这条事实",
        "pin this fact",
        "protect this fact",
        "update this fact",
        "delete this fact",
        "save this to the knowledge base",
        "store this in the knowledge base",
        "write this into the knowledge base",
        "remember this",
    ];

    (retrieval_markers
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()))
        || retrieval_context_markers
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase())))
        && !mutation_markers
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
        && !has_directed_durable_retrieval_storage_request(&lowered)
}

fn query_requests_capability_gap(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_contains_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|needle| token == needle))
    };

    let artifact_markers = [
        "工具",
        "插件",
        "skill",
        "worker",
        "能力",
        "脚本",
        "自动化",
        "plugin",
        "tool",
        "script",
        "automation",
        "agent",
    ];
    let build_verbs = [
        "造",
        "做",
        "创建",
        "生成",
        "安装",
        "接入",
        "添加",
        "配置",
        "装上",
        "启用",
        "编写",
        "开发",
        "实现",
        "搭一个",
        "写一个",
        "build",
        "create",
        "generate",
        "install",
        "setup",
        "set up",
        "add",
        "enable",
        "configure",
        "make",
        "implement",
        "develop",
        "write",
    ];

    (contains_any(&artifact_markers)
        || lowered_contains_any(&artifact_markers)
        || has_token(&[
            "skill",
            "worker",
            "tool",
            "plugin",
            "script",
            "automation",
            "agent",
        ]))
        && (contains_any(&build_verbs)
            || lowered_contains_any(&build_verbs)
            || has_token(&[
                "build",
                "create",
                "generate",
                "install",
                "setup",
                "add",
                "enable",
                "configure",
                "make",
                "implement",
                "develop",
                "write",
            ]))
}

fn query_requests_file_ops(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let query_has_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_has_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|n| token == n))
    };

    let has_path_like_target = query_contains_filesystem_path(query)
        || query_has_any(&[
            "文件",
            "文件夹",
            "目录",
            "路径",
            "工作区",
            "workspace",
            "path",
            "folder",
            "directory",
        ])
        || has_token(&[
            "file",
            "files",
            "folder",
            "directory",
            "path",
            "workspace",
            "readme",
            "md",
            "json",
            "yaml",
            "toml",
            "txt",
            "log",
            "csv",
            "rs",
            "py",
            "js",
            "ts",
        ]);

    let has_file_op_verb = query_has_any(&[
        "读取",
        "读出",
        "打开",
        "查看",
        "看下",
        "显示",
        "列出",
        "罗列",
        "打印",
        "写入",
        "写到",
        "保存到",
        "修改",
        "编辑",
        "创建文件",
        "读取文件",
        "读取目录",
    ]) || lowered_has_any(&[
        "read ", "open ", "show ", "view ", "list ", "ls ", "cat ", "write ", "save ", "edit ",
    ]) || has_token(&[
        "read", "open", "show", "view", "list", "ls", "cat", "write", "save", "edit",
    ]);

    let has_file_output_marker =
        query_has_any(&[
            "前一行",
            "前两行",
            "前三行",
            "前几行",
            "内容",
            "全文",
            "第一行",
            "第二行",
            "第三行",
            "列一下",
        ]) || lowered_has_any(&["first line", "first lines", "top lines", "contents"]);

    (has_path_like_target && has_file_op_verb)
        || (query_contains_filesystem_path(query) && has_file_output_marker)
}

fn query_contains_filesystem_path(query: &str) -> bool {
    let trimmed = query.trim();
    let tokens = trimmed
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| matches!(c, '"' | '\'' | '，' | '。' | ',')))
        .collect::<Vec<_>>();

    tokens.iter().any(|token| {
        token.starts_with('/')
            || token.starts_with("./")
            || token.starts_with("../")
            || token.starts_with("~/")
            || (token.len() > 3
                && token.as_bytes().get(1) == Some(&b':')
                && matches!(token.as_bytes().get(2), Some(b'\\' | b'/')))
            || [
                ".md", ".txt", ".json", ".yaml", ".yml", ".toml", ".rs", ".py", ".js", ".ts",
                ".csv", ".log",
            ]
            .iter()
            .any(|ext| token.ends_with(ext))
    })
}

fn tool_match_score(
    entry: &ToolCatalogEntry,
    query: &str,
    tokens: &[String],
    desired_capability_domain: Option<&str>,
    preferred_tool_names: &[&str],
) -> i32 {
    let name = entry.name.to_lowercase();
    let description = entry.description.to_lowercase();
    let capability_domain = entry.capability_domain.to_lowercase();
    let guidelines = entry
        .usage_guidelines
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let tags = entry.tags.join(" ").to_lowercase();
    let source = entry.source.to_lowercase();
    let haystack = format!(
        "{} {} {} {} {} {}",
        name, description, capability_domain, guidelines, tags, source
    );

    let mut score = 0;
    if name == query {
        score += 120;
    } else if name.contains(query) {
        score += 80;
    }

    if description.contains(query) {
        score += 36;
    }

    if guidelines.contains(query) {
        score += 24;
    }

    if capability_domain.contains(query) {
        score += 28;
    }

    if let Some(desired) = desired_capability_domain {
        if capability_domain == desired {
            score += 72;
        } else if capability_domain.starts_with(desired) || desired.starts_with(&capability_domain)
        {
            score += 36;
        }
    }

    for (idx, preferred) in preferred_tool_names.iter().enumerate() {
        if entry.name == *preferred {
            score += 96_i32.saturating_sub((idx as i32) * 18);
        }
    }

    for token in tokens {
        if name == *token {
            score += 40;
        } else if name.contains(token) {
            score += 24;
        }

        if description.contains(token) {
            score += 10;
        }

        if capability_domain.contains(token) {
            score += 20;
        }

        if guidelines.contains(token) {
            score += 6;
        }

        if tags.contains(token) {
            score += 18;
        }

        if source.contains(token) {
            score += 6;
        }
    }

    if haystack.contains("search") && tokens.iter().any(|t| t == "search" || t == "搜索") {
        score += 8;
    }

    score
}

fn infer_tool_source(name: &str) -> String {
    if name.starts_with("mcp:") {
        "mcp".to_string()
    } else if name == "forge_skill" || name.starts_with("hardened_") {
        "forge".to_string()
    } else {
        "builtin".to_string()
    }
}

fn infer_tool_scope(name: &str) -> String {
    if name.starts_with("mcp:") {
        "external".to_string()
    } else if name == "forge_skill" || name.starts_with("hardened_") {
        "session".to_string()
    } else {
        "agent".to_string()
    }
}

fn infer_tool_capability_domain(
    name: &str,
    description: &str,
    usage_guidelines: Option<&str>,
) -> String {
    let lower_name = name.to_lowercase();
    let lower_description = description.to_lowercase();
    let lower_guidelines = usage_guidelines.unwrap_or_default().to_lowercase();
    let haystack = format!("{} {} {}", lower_name, lower_description, lower_guidelines);

    if lower_name == "generate_image"
        || haystack.contains("image generation")
        || haystack.contains("generate an image")
        || haystack.contains("text-to-image")
        || haystack.contains("文生图")
        || haystack.contains("生成图片")
        || haystack.contains("画图")
    {
        return "image_generation".to_string();
    }

    if lower_name == "web_search"
        || lower_name == "web_fetch"
        || lower_name.contains("browser_open")
        || lower_name.contains("browser_extract")
        || lower_name.contains("browser")
    {
        return "realtime_lookup.web".to_string();
    }

    if haystack.contains("weather")
        || haystack.contains("forecast")
        || haystack.contains("天气")
        || haystack.contains("预报")
    {
        return "realtime_lookup.weather".to_string();
    }

    if haystack.contains("exchange rate")
        || haystack.contains("fx")
        || haystack.contains("汇率")
        || haystack.contains("currency")
    {
        return "realtime_lookup.fx".to_string();
    }

    if haystack.contains("price")
        || haystack.contains("quote")
        || haystack.contains("btc")
        || haystack.contains("stock")
        || haystack.contains("价格")
        || haystack.contains("币价")
        || haystack.contains("股价")
    {
        return "realtime_lookup.price".to_string();
    }

    if haystack.contains("latest")
        || haystack.contains("news")
        || haystack.contains("today")
        || haystack.contains("最近")
        || haystack.contains("最新")
        || haystack.contains("新闻")
    {
        return "realtime_lookup.latest_info".to_string();
    }

    if haystack.contains("search")
        || haystack.contains("fetch")
        || haystack.contains("browser")
        || haystack.contains("搜索")
        || haystack.contains("网页")
    {
        return "realtime_lookup.web".to_string();
    }

    if haystack.contains("pdf")
        || haystack.contains("document")
        || haystack.contains("text extract")
        || haystack.contains("ocr")
        || haystack.contains("文档")
        || haystack.contains("文件内容")
    {
        return "document_understanding".to_string();
    }

    if haystack.contains("image")
        || haystack.contains("visual")
        || haystack.contains("vision")
        || haystack.contains("图像")
        || haystack.contains("截图")
    {
        return "document_understanding".to_string();
    }

    if haystack.contains("voice")
        || haystack.contains("audio")
        || haystack.contains("speech")
        || haystack.contains("语音")
        || haystack.contains("音频")
    {
        return "voice_understanding".to_string();
    }

    if haystack.contains("git")
        || haystack.contains("docker")
        || haystack.contains("npm")
        || haystack.contains("pnpm")
        || haystack.contains("yarn")
        || haystack.contains("cargo")
        || haystack.contains("ffmpeg")
        || haystack.contains("chrome")
        || haystack.contains("chromium")
        || haystack.contains("playwright")
        || haystack.contains("cli")
        || haystack.contains("command line")
    {
        return "external_cli_tools".to_string();
    }

    if haystack.contains("shell")
        || haystack.contains("bash")
        || haystack.contains("powershell")
        || haystack.contains("cmd")
        || haystack.contains("uv")
        || haystack.contains("pixi")
        || haystack.contains("bun")
        || haystack.contains("gcc")
        || haystack.contains("python")
        || haystack.contains("node")
        || haystack.contains("quickjs")
    {
        return "runtime_surface".to_string();
    }

    if haystack.contains("file")
        || haystack.contains("directory")
        || haystack.contains("filesystem")
        || haystack.contains("路径")
        || haystack.contains("目录")
    {
        return "file_ops".to_string();
    }

    if haystack.contains("code")
        || haystack.contains("repo")
        || haystack.contains("repository")
        || haystack.contains("patch")
        || haystack.contains("代码")
    {
        return "coding".to_string();
    }

    if haystack.contains("mail")
        || haystack.contains("email")
        || haystack.contains("slack")
        || haystack.contains("telegram")
        || haystack.contains("discord")
        || haystack.contains("通知")
        || haystack.contains("消息")
    {
        return "communication".to_string();
    }

    if haystack.contains("memory")
        || haystack.contains("knowledge")
        || haystack.contains("vault")
        || haystack.contains("记忆")
        || haystack.contains("知识")
    {
        return "memory".to_string();
    }

    if lower_name == "forge_skill" || lower_name.starts_with("hardened_") {
        return "capability_gap".to_string();
    }

    "general".to_string()
}

fn infer_tool_tags(name: &str, description: &str, usage_guidelines: Option<&str>) -> Vec<String> {
    let lower_name = name.to_lowercase();
    let lower_description = description.to_lowercase();
    let lower_guidelines = usage_guidelines.unwrap_or_default().to_lowercase();
    let haystack = format!("{} {} {}", lower_name, lower_description, lower_guidelines);

    let mut tags = Vec::new();
    fn push_tag(tags: &mut Vec<String>, tag: &str) {
        if !tags.iter().any(|existing| existing == tag) {
            tags.push(tag.to_string());
        }
    }

    if lower_name.starts_with("mcp:") {
        push_tag(&mut tags, "mcp");
        push_tag(&mut tags, "external");
    }

    if haystack.contains("search") || haystack.contains("搜索") || haystack.contains("latest") {
        push_tag(&mut tags, "search");
        push_tag(&mut tags, "web");
    }
    if haystack.contains("price") || haystack.contains("btc") || haystack.contains("汇率") {
        push_tag(&mut tags, "market");
    }
    if haystack.contains("pdf") || haystack.contains("document") || haystack.contains("文档") {
        push_tag(&mut tags, "document");
    }
    if haystack.contains("file") || haystack.contains("目录") || haystack.contains("filesystem") {
        push_tag(&mut tags, "filesystem");
    }
    if haystack.contains("git") || haystack.contains("repo") || haystack.contains("repository") {
        push_tag(&mut tags, "code");
        push_tag(&mut tags, "git");
        push_tag(&mut tags, "external_cli");
    }
    if haystack.contains("bash")
        || haystack.contains("powershell")
        || haystack.contains("cmd")
        || haystack.contains("uv")
        || haystack.contains("pixi")
        || haystack.contains("bun")
        || haystack.contains("quickjs")
        || haystack.contains("python")
    {
        push_tag(&mut tags, "runtime_surface");
    }
    if haystack.contains("image") || haystack.contains("visual") || haystack.contains("图像") {
        push_tag(&mut tags, "vision");
    }
    if haystack.contains("image generation")
        || haystack.contains("generate an image")
        || haystack.contains("text-to-image")
        || haystack.contains("文生图")
        || haystack.contains("生成图片")
        || haystack.contains("画图")
    {
        push_tag(&mut tags, "image_generation");
    }
    if haystack.contains("voice") || haystack.contains("audio") || haystack.contains("语音") {
        push_tag(&mut tags, "audio");
    }
    if haystack.contains("chart") || haystack.contains("plot") || haystack.contains("图表") {
        push_tag(&mut tags, "analytics");
    }
    if haystack.contains("mail") || haystack.contains("email") || haystack.contains("通知") {
        push_tag(&mut tags, "communication");
    }
    if lower_name == "forge_skill" || lower_name.starts_with("hardened_") {
        push_tag(&mut tags, "forge");
        push_tag(&mut tags, "generated");
    }

    if tags.is_empty() {
        push_tag(&mut tags, "general");
    }

    tags
}

/// Tool for managing scheduled tasks
#[cfg(feature = "cron")]
pub struct CronTool {
    scheduler: std::sync::Weak<benshu_scheduler::Scheduler>,
}

#[cfg(feature = "cron")]
impl CronTool {
    pub fn new(scheduler: std::sync::Weak<benshu_scheduler::Scheduler>) -> Self {
        Self { scheduler }
    }
}

#[cfg(feature = "cron")]
#[async_trait::async_trait]
impl Tool for CronTool {
    fn name(&self) -> String {
        "cron".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cron".to_string(),
            description: "Manage scheduled tasks.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string" }
                },
                "required": ["action"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let _ = arguments;
        Ok("Cron tool called (implementation in builtin-tools)".to_string())
    }
}

#[async_trait::async_trait]
impl crate::agent::context::ContextInjector for ToolSet {
    async fn inject(
        &self,
        _history: &[crate::agent::message::Message],
    ) -> crate::error::Result<Vec<crate::agent::message::Message>> {
        if self.is_empty() {
            return Ok(Vec::new());
        }

        let mut content = String::from("## Available Tools (Index)\n\n");
        content.push_str(
            "You have access to the following tools. To save context, only descriptions are shown below. \
             Full TypeScript schemas and usage guidelines will be automatically injected into the conversation \
             the first time you use a specific tool.\n\n",
        );
        if self.contains("tool_search") && self.len() > 8 {
            content.push_str(
                "If you are unsure which tool fits a task, call `tool_search` first to retrieve a short list of the most relevant tools before choosing one.\n\n",
            );
        }

        let catalog = self.catalog().await;
        let (visible_entries, deferred_count) = prompt_visible_catalog_entries(&catalog);

        for entry in visible_entries {
            content.push_str(&format!("- **{}**: {}\n", entry.name, entry.description));
        }

        if deferred_count > 0 {
            content.push_str(&format!(
                "\n{} additional tools are intentionally deferred from this prompt index to keep context focused. Use `tool_search` to discover and load the right long-tail tool when needed.\n",
                deferred_count
            ));
        }

        Ok(vec![crate::agent::message::Message::system(content)])
    }
}

fn prompt_visible_catalog_entries(entries: &[ToolCatalogEntry]) -> (Vec<ToolCatalogEntry>, usize) {
    if entries.len() <= 8 {
        return (entries.to_vec(), 0);
    }

    let visible: Vec<_> = entries
        .iter()
        .filter(|entry| should_keep_tool_in_prompt_index(entry))
        .cloned()
        .collect();
    let deferred_count = entries.len().saturating_sub(visible.len());

    if visible.is_empty() {
        (entries.to_vec(), 0)
    } else {
        (visible, deferred_count)
    }
}

fn should_keep_tool_in_prompt_index(entry: &ToolCatalogEntry) -> bool {
    matches!(
        entry.name.as_str(),
        "tool_search"
            | "tool_catalog"
            | "read_skill_manual"
            | "read_skill_asset"
            | "document_understand"
            | "pdf_parse"
            | "text_extract"
            | "web_search"
            | "web_fetch"
            | "price_lookup"
            | "fx_lookup"
            | "weather_lookup"
            | "latest_info_lookup"
            | "runtime_surface"
            | "read_file"
            | "list_dir"
            | "edit_file"
            | "write_file"
    ) || matches!(
        entry.capability_domain.as_str(),
        "document_understanding"
            | "realtime_lookup.web"
            | "realtime_lookup.price"
            | "realtime_lookup.fx"
            | "realtime_lookup.weather"
            | "realtime_lookup.latest_info"
            | "runtime_surface"
            | "command_exec"
            | "file_ops"
    ) || (entry.source == "builtin"
        && entry.scope == "agent"
        && !matches!(
            entry.capability_domain.as_str(),
            "external_cli_tools" | "capability_gap"
        ))
}

/// Builder for creating a ToolSet
pub struct ToolSetBuilder {
    tools: Vec<Arc<dyn Tool>>,
}

impl Default for ToolSetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolSetBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Add a tool
    pub fn tool<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    /// Add a shared tool
    pub fn shared_tool(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// Build the ToolSet
    pub fn build(self) -> ToolSet {
        let toolset = ToolSet::new();
        for tool in self.tools {
            toolset.add_shared(tool);
        }
        toolset
    }
}

/// Helper macro for creating simple tools
#[macro_export]
macro_rules! simple_tool {
    (
        name: $name:expr,
        description: $desc:expr,
        parameters: $params:expr,
        handler: $handler:expr
    ) => {{
        struct SimpleTool;

        #[async_trait::async_trait]
        impl benshu_infra::traits::tool::Tool for SimpleTool {
            fn name(&self) -> String {
                $name.to_string()
            }

            async fn definition(&self) -> benshu_infra::traits::tool::ToolDefinition {
                benshu_infra::traits::tool::ToolDefinition {
                    name: $name.to_string(),
                    description: $desc.to_string(),
                    parameters: $params,
                    usage_guidelines: None,
                    is_binary: false,
                    is_verified: false,
                    parameters_ts: None,
                    safety_level: Default::default(),
                }
            }

            async fn call(&self, arguments: &str) -> anyhow::Result<String> {
                let handler = $handler;
                handler(arguments).await
            }
        }

        SimpleTool
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SearchTool;
    struct PdfTool;
    struct ChartTool;
    struct GitTool;
    struct WeatherTool;
    struct ToolSearchIndexTool;
    struct ReadSkillManualIndexTool;
    struct ReadSkillAssetIndexTool;
    struct RuntimeSurfaceIndexTool;
    struct McpSqlTool;

    #[test]
    fn action_schema_shorthand_is_normalized_without_action_specific_rules() {
        let definition = ToolDefinition {
            name: "compound_tool".into(),
            description: "Compound action tool".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["alpha", "beta"]
                    },
                    "project_path": { "type": "string" }
                }
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            safety_level: SafetyLevel::Green,
            usage_guidelines: None,
        };

        let normalized = normalize_action_shorthand_from_definition(
            &definition,
            serde_json::json!({
                "beta": true,
                "project_path": "/tmp/project"
            }),
        );

        assert_eq!(normalized["action"], "beta");
        assert_eq!(normalized["project_path"], "/tmp/project");
        assert!(normalized.get("beta").is_none());
    }

    #[async_trait::async_trait]
    impl Tool for SearchTool {
        fn name(&self) -> String {
            "web_search".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "web_search".into(),
                description: "Search the web for fresh public information.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Green,
                usage_guidelines: Some(
                    "Use for search, latest info, prices, and current events.".into(),
                ),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for PdfTool {
        fn name(&self) -> String {
            "pdf_parse".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "pdf_parse".into(),
                description: "Parse and extract text from PDF files.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Yellow,
                usage_guidelines: Some("Use when the user provides a PDF document.".into()),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for ChartTool {
        fn name(&self) -> String {
            "chart".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "chart".into(),
                description: "Generate charts from structured data.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Green,
                usage_guidelines: Some("Use for visualizing tabular or time series data.".into()),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for GitTool {
        fn name(&self) -> String {
            "git_adapter".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "git_adapter".into(),
                description: "Run common git CLI operations through a controlled adapter.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Yellow,
                usage_guidelines: Some(
                    "Use for git status, branch, diff, and repository inspection tasks.".into(),
                ),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for WeatherTool {
        fn name(&self) -> String {
            "weather_lookup".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "weather_lookup".into(),
                description: "Lookup structured weather and forecast information.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Green,
                usage_guidelines: Some(
                    "Use for weather, forecast, temperature, and rain questions.".into(),
                ),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for ToolSearchIndexTool {
        fn name(&self) -> String {
            "tool_search".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "tool_search".into(),
                description: "Search the tool catalog.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Green,
                usage_guidelines: Some("Use to find long-tail tools.".into()),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for ReadSkillManualIndexTool {
        fn name(&self) -> String {
            "read_skill_manual".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "read_skill_manual".into(),
                description: "Read a skill manual.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Green,
                usage_guidelines: Some("Use before executing a matched skill.".into()),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for ReadSkillAssetIndexTool {
        fn name(&self) -> String {
            "read_skill_asset".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "read_skill_asset".into(),
                description: "Read a skill reference, template, or script asset.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Green,
                usage_guidelines: Some("Use after reading a skill manual.".into()),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for RuntimeSurfaceIndexTool {
        fn name(&self) -> String {
            "runtime_surface".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "runtime_surface".into(),
                description: "Inspect BenShu runtime surfaces.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Green,
                usage_guidelines: Some("Use for runtime substrate inspection.".into()),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[async_trait::async_trait]
    impl Tool for McpSqlTool {
        fn name(&self) -> String {
            "mcp_sql".into()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "mcp_sql".into(),
                description: "Query a remote MCP-backed SQL tool.".into(),
                parameters: serde_json::json!({"type":"object"}),
                parameters_ts: None,
                is_binary: false,
                is_verified: true,
                safety_level: SafetyLevel::Yellow,
                usage_guidelines: Some("Use for remote SQL access through MCP.".into()),
            }
        }

        async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[tokio::test]
    async fn search_catalog_ranks_relevant_tools_first() {
        let toolset = ToolSet::new();
        toolset.add(SearchTool).add(PdfTool).add(ChartTool);

        let results = toolset.search_catalog("btc latest price search", 3).await;
        assert_eq!(results.first().map(|e| e.name.as_str()), Some("web_search"));
    }

    #[tokio::test]
    async fn search_catalog_handles_pdf_queries() {
        let toolset = ToolSet::new();
        toolset.add(SearchTool).add(PdfTool).add(ChartTool);

        let results = toolset.search_catalog("pdf document text", 2).await;
        assert_eq!(results.first().map(|e| e.name.as_str()), Some("pdf_parse"));
    }

    #[tokio::test]
    async fn search_catalog_prefers_capability_domain_matches() {
        let toolset = ToolSet::new();
        toolset
            .add(SearchTool)
            .add(PdfTool)
            .add(ChartTool)
            .add(GitTool)
            .add(WeatherTool);

        let weather_results = toolset.search_catalog("上海明天天气", 3).await;
        assert_eq!(
            weather_results.first().map(|entry| entry.name.as_str()),
            Some("weather_lookup")
        );

        let cli_results = toolset.search_catalog("用 git cli 看当前分支", 3).await;
        assert_eq!(
            cli_results.first().map(|entry| entry.name.as_str()),
            Some("git_adapter")
        );
    }

    #[test]
    fn classify_query_capability_domain_splits_runtime_surface_and_external_cli() {
        assert_eq!(
            classify_query_capability_domain("用 git cli 看当前分支").as_deref(),
            Some("external_cli_tools")
        );
        assert_eq!(
            classify_query_capability_domain("用 powershell 列出当前目录").as_deref(),
            Some("runtime_surface")
        );
        assert_eq!(classify_query_capability_domain("什么是 git"), None);
        assert_eq!(classify_query_capability_domain("bash 是什么"), None);
        assert_eq!(classify_query_capability_domain("介绍一下 docker"), None);
    }

    #[test]
    fn capability_route_hardness_matrix_keeps_explanations_tool_free() {
        let cases = [
            "什么是 git？",
            "git 是什么？",
            "bash 是什么？",
            "解释一下 Docker 容器是什么。",
            "用一句话解释 CPU 和 GPU 的区别。",
            "为什么天空是蓝色的？",
            "帮我润色这句话：今天心情很好。",
            "你能做什么？",
            "给我讲一个简短的笑话。",
            "Rust 的 ownership 是什么？",
        ];

        for case in cases {
            assert_eq!(
                classify_query_capability_route(case),
                None,
                "plain explanation should not request a tool route: {case}"
            );
        }
    }

    #[test]
    fn capability_route_hardness_matrix_detects_real_tool_tasks() {
        let cases = [
            (
                "今天北京天气怎么样？",
                CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WeatherLookup),
            ),
            (
                "比特币现在多少钱？",
                CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::PriceLookup),
            ),
            (
                "纳斯达克点数多少？",
                CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::PriceLookup),
            ),
            (
                "今天最新时事新闻是什么？",
                CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup),
            ),
            (
                "搜索 Rust release notes 并给来源。",
                CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch),
            ),
            (
                "请读取 https://example.com 的页面标题。",
                CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch),
            ),
            ("把这条偏好记住：回答要简洁。", CapabilityRouteHint::Memory),
            (
                "我上一条让你记住的那句话是什么？",
                CapabilityRouteHint::Memory,
            ),
            (
                "请总结我上传的 PDF。",
                CapabilityRouteHint::DocumentUnderstanding,
            ),
            (
                "用 powershell 列出当前目录。",
                CapabilityRouteHint::RuntimeSurface,
            ),
            (
                "帮我修一下这个 Rust 仓库里的 bug 并提交补丁。",
                CapabilityRouteHint::Coding,
            ),
        ];

        for (case, expected) in cases {
            assert_eq!(
                classify_query_capability_route(case),
                Some(expected),
                "task query should route to the expected capability: {case}"
            );
        }
    }

    #[test]
    fn classify_query_capability_route_returns_shared_route_hints() {
        assert_eq!(
            classify_query_capability_route("回归测试：请用中文只回复主脑聊天链路正常"),
            None
        );
        assert_eq!(
            classify_query_capability_route("你好，用一句中文回复：现在可以开始测试。"),
            None
        );
        assert_eq!(classify_query_capability_route("现在可以开始测试。"), None);
        assert_eq!(
            classify_query_capability_route("帮我测试这个仓库"),
            Some(CapabilityRouteHint::Coding)
        );
        assert_eq!(
            classify_query_capability_route("帮我查 BTC 现在价格"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert_eq!(
            classify_query_capability_route("纳斯达克点数多少？"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert_eq!(
            classify_query_capability_route("比特币现在多少钱？"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert_eq!(
            classify_query_capability_route("AAPL 股票现在多少钱？"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert_eq!(
            classify_query_capability_route("北京今天天气怎么样"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WeatherLookup
            ))
        );
        assert_eq!(
            classify_query_capability_route("请读取 https://example.com 的页面标题"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_eq!(
            classify_query_capability_route("请在公网查找热门免费资料并保存成txt文档"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_eq!(
            classify_query_capability_route(
                "search the public market for downloadable free fiction"
            ),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_eq!(
            classify_query_capability_route(
                "Search for popular, downloadable, and free fantasy (玄幻/奇幻) novels available on the public web. Find up to 10 novels and their content."
            ),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_eq!(
            classify_query_capability_route("在网上寻找可下载的数据集，之后写入知识库"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_ne!(
            classify_query_capability_route("帮我写一个txt文档"),
            Some(CapabilityRouteHint::DocumentUnderstanding)
        );
        assert_eq!(
            capability_route_preferred_tool_names_for_query(
                CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch),
                "请委托 browser worker 读取 https://example.com"
            ),
            vec!["delegate", "tool_search"]
        );
        assert_eq!(
            classify_query_capability_route("用 powershell 列出当前目录"),
            Some(CapabilityRouteHint::RuntimeSurface)
        );
        assert_eq!(
            classify_query_capability_route("用 git cli 看当前分支"),
            Some(CapabilityRouteHint::ExternalCliTools)
        );
        assert_eq!(
            classify_query_capability_route("帮我总结这个 PDF"),
            Some(CapabilityRouteHint::DocumentUnderstanding)
        );
        assert_eq!(
            classify_query_capability_route(
                "请只根据我这次上传的附件回答：附件里的 SENTINEL 和结论字段分别是什么？不要查询知识库。"
            ),
            Some(CapabilityRouteHint::DocumentUnderstanding)
        );
        assert_eq!(
            classify_query_capability_route(
                "请读取 /home/biubiuboy/BenShu/data/agents/benshu/AGENT.md 的前三行"
            ),
            Some(CapabilityRouteHint::FileOps)
        );
        assert_eq!(
            classify_query_capability_route("帮我修一下这个 Rust 仓库里的 bug 并提交补丁"),
            Some(CapabilityRouteHint::Coding)
        );
        assert_eq!(
            classify_query_capability_route("帮我创建一个新的工具插件来做导出"),
            Some(CapabilityRouteHint::CapabilityGap)
        );
        assert_eq!(
            classify_query_capability_route("帮我发一封邮件通知团队今天发布"),
            Some(CapabilityRouteHint::Communication)
        );
        assert_eq!(
            classify_query_capability_route("把这条偏好记住，以后都按这个来"),
            Some(CapabilityRouteHint::Memory)
        );
        assert_eq!(
            classify_query_capability_route("我上一条让你记住的那句话是什么？只回复那句话本身。"),
            Some(CapabilityRouteHint::Memory)
        );
        assert_eq!(classify_query_capability_route("什么是 git"), None);
    }

    #[test]
    fn capability_router_allows_frontstage_coordination_routes_without_slm() {
        let router = CapabilityRouter::default();
        assert_eq!(
            router.classify_query_route("帮我修一下这个 Rust 仓库里的 bug 并提交补丁"),
            Some(CapabilityRouteHint::Coding)
        );
        assert_eq!(
            router.classify_query_route("帮我创建一个新的工具插件来做导出"),
            Some(CapabilityRouteHint::CapabilityGap)
        );
        assert_eq!(
            router.classify_query_route("帮我发一封邮件通知团队今天发布"),
            Some(CapabilityRouteHint::Communication)
        );
        assert_eq!(
            router.classify_query_route("把这条偏好记住，以后都按这个来"),
            Some(CapabilityRouteHint::Memory)
        );
    }

    #[test]
    fn resolve_capability_route_applies_shared_context_biases() {
        assert_eq!(
            resolve_capability_route(
                "帮我查 BTC 现在价格",
                CapabilityRouteRequest {
                    approved_forge_request: true,
                    ..Default::default()
                },
            ),
            None
        );

        assert_eq!(
            resolve_capability_route(
                "看这个附件里写了什么",
                CapabilityRouteRequest {
                    has_media_input: true,
                    ..Default::default()
                },
            ),
            Some(CapabilityRouteHint::DocumentUnderstanding)
        );

        assert_eq!(
            resolve_capability_route(
                "帮我查 BTC 现在价格",
                CapabilityRouteRequest {
                    force_document_understanding: true,
                    ..Default::default()
                },
            ),
            Some(CapabilityRouteHint::DocumentUnderstanding)
        );

        assert_eq!(
            resolve_capability_route(
                "请用 git_helper 帮我处理这个仓库",
                CapabilityRouteRequest {
                    runtime_surface_bias: true,
                    ..Default::default()
                },
            ),
            Some(CapabilityRouteHint::RuntimeSurface)
        );

        assert_eq!(
            resolve_capability_route(
                "帮我做一个搜索 btc 价格的工具",
                CapabilityRouteRequest {
                    suppress_document_understanding: true,
                    suppress_realtime_lookup: true,
                    ..Default::default()
                },
            ),
            None
        );

        assert_eq!(
            resolve_capability_route(
                "帮我做一个图片理解工具",
                CapabilityRouteRequest {
                    suppress_document_understanding: true,
                    ..Default::default()
                },
            ),
            None
        );
    }

    #[test]
    fn query_requests_image_generation_detects_direct_draw_requests() {
        assert!(query_requests_image_generation("请帮我生成一张图片"));
        assert!(query_requests_image_generation(
            "请帮我生成一张可爱的猫咪图片，风格温暖一点。"
        ));
        assert!(query_requests_image_generation(
            "draw image of a silver logo"
        ));
        assert!(query_requests_image_generation("做一张海报"));
        assert!(!query_requests_image_generation("帮我看一下这张图片"));
        assert!(!query_requests_image_generation(
            "请给小说生成候选书名，女主是独立插画师。"
        ));
    }

    #[test]
    fn image_generation_routes_to_specialist_without_frontstage_image_tool() {
        let query = "请帮我生成一张可爱的猫咪图片，风格温暖一点。";

        assert_eq!(
            classify_query_capability_route(query),
            Some(CapabilityRouteHint::CapabilityGap)
        );

        let allowed = capability_route_tool_allowlist_for_query(
            CapabilityRouteHint::CapabilityGap,
            Some(query),
        );
        assert!(allowed.contains("delegate"));
        assert!(allowed.contains("shared_board"));
        assert!(allowed.contains("tool_search"));
        assert!(!allowed.contains("generate_image"));

        let chat_lite_allowed = coordinator_chat_lite_tool_names_for_query(Some(query));
        assert!(chat_lite_allowed.contains("delegate"));
        assert!(!chat_lite_allowed.contains("generate_image"));
    }

    #[test]
    fn query_requests_document_understanding_detects_multimodal_phrases() {
        assert!(query_requests_document_understanding("帮我看一下这张图片"));
        assert!(query_requests_document_understanding(
            "summarize this document"
        ));
        assert!(query_requests_document_understanding(
            "帮我提取这个pdf里的文字"
        ));
        assert!(!query_requests_document_understanding("什么是 git"));
    }

    #[test]
    fn capability_route_requires_source_fetch_for_sensitive_realtime() {
        assert!(!capability_route_requires_source_fetch(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch)
        ));
        assert!(capability_route_requires_source_fetch(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::PriceLookup)
        ));
        assert!(capability_route_requires_source_fetch(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::FxLookup)
        ));
        assert!(capability_route_requires_source_fetch(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WeatherLookup)
        ));
        assert!(capability_route_requires_source_fetch(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup)
        ));
        assert!(!capability_route_requires_source_fetch(
            CapabilityRouteHint::RuntimeSurface
        ));
    }

    #[test]
    fn capability_router_exposes_shared_route_metadata() {
        let router = CapabilityRouter::default();
        let route = router
            .classify_query_route("帮我查一下美元兑人民币汇率")
            .expect("fx route");

        assert_eq!(
            route,
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::FxLookup)
        );
        assert_eq!(router.route_label(route), "realtime_lookup.fx");
        assert_eq!(
            router.preferred_capability_domain("帮我查一下美元兑人民币汇率"),
            Some("realtime_lookup.fx")
        );
        assert!(router.route_requires_source_fetch(route));
    }

    #[test]
    fn capability_router_treats_file_ops_as_hard_route() {
        let router = CapabilityRouter::default();
        let route = router
            .classify_query_route(
                "请读取 /home/biubiuboy/BenShu/data/agents/benshu/AGENT.md 的前三行",
            )
            .expect("file ops route");

        assert_eq!(route, CapabilityRouteHint::FileOps);
        assert_eq!(router.route_label(route), "file_ops");
        assert!(router.route_requires_real_tool_call(route));
        assert_eq!(
            router.preferred_tool_names(route),
            &[
                "read_file",
                "list_dir",
                "edit_file",
                "write_file",
                "tool_search"
            ]
        );
    }

    #[test]
    fn capability_route_file_ops_system_prompt_and_failure_message_are_explicit() {
        let prompt = capability_route_system_message(
            "请读取 /home/biubiuboy/BenShu/data/agents/benshu/AGENT.md 的前三行",
            CapabilityRouteHint::FileOps,
            None,
            None,
        )
        .expect("file prompt");
        assert!(prompt.contains("FILE_OPS_HARD_ROUTE"));
        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("filesystem tool"));

        let failure = capability_route_tool_required_failure_message(CapabilityRouteHint::FileOps);
        assert!(failure.contains("文件系统工具"));
        assert!(failure.contains("不假装已经读到"));
    }

    #[test]
    fn classify_query_verification_plan_handles_latest_info_queries() {
        let plan = classify_query_verification_plan("帮我查一下今天 OpenAI 最新新闻")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RealtimeLookup);
        assert_eq!(
            plan.route_hint,
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::LatestInfoLookup
            ))
        );
    }

    #[test]
    fn classify_query_verification_plan_handles_current_role_holder_queries() {
        let plan = classify_query_verification_plan("美国现任总统是谁").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RealtimeLookup);
        assert_eq!(
            plan.route_hint,
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::LatestInfoLookup
            ))
        );
    }

    #[test]
    fn classify_query_verification_plan_handles_english_current_role_holder_queries() {
        let plan = classify_query_verification_plan("who is the current ceo of openai")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RealtimeLookup);
        assert_eq!(
            plan.route_hint,
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::LatestInfoLookup
            ))
        );
    }

    #[test]
    fn classify_query_verification_plan_handles_current_price_queries() {
        let plan = classify_query_verification_plan("帮我查一下英伟达当前股价")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RealtimeLookup);
        assert_eq!(
            plan.route_hint,
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
    }

    #[test]
    fn classify_query_verification_plan_handles_release_version_queries() {
        let plan =
            classify_query_verification_plan("告诉我 Bun 最新发布版本").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RealtimeLookup);
        assert_eq!(
            plan.route_hint,
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::LatestInfoLookup
            ))
        );
    }

    #[test]
    fn classify_query_verification_plan_handles_recent_release_note_queries() {
        let plan =
            classify_query_verification_plan("最近 bun 发布了什么版本").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RealtimeLookup);
        assert_eq!(
            plan.route_hint,
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::LatestInfoLookup
            ))
        );
    }

    #[test]
    fn classify_query_verification_plan_handles_tool_fact_queries() {
        let plan = classify_query_verification_plan("帮我确认有没有 ffmpeg 这个 cli 工具")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ToolFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ToolInventoryCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::ExternalCliTools));
    }

    #[test]
    fn classify_query_verification_plan_handles_current_policy_queries() {
        let plan = classify_query_verification_plan("当前 OpenAI API 定价政策是什么")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RealtimeLookup);
        assert_eq!(
            plan.route_hint,
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::LatestInfoLookup
            ))
        );
    }

    #[test]
    fn classify_query_verification_plan_handles_cli_installation_confirmation_queries() {
        let plan =
            classify_query_verification_plan("帮我确认 git 有没有安装").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ToolFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ToolInventoryCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::ExternalCliTools));
    }

    #[test]
    fn classify_query_verification_plan_handles_english_cli_installation_queries() {
        let plan =
            classify_query_verification_plan("is ffmpeg installed").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ToolFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ToolInventoryCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::ExternalCliTools));
    }

    #[test]
    fn classify_query_verification_plan_handles_english_tool_availability_queries() {
        let plan = classify_query_verification_plan("is docker available right now")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ToolFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ToolInventoryCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::ExternalCliTools));
    }

    #[test]
    fn classify_query_verification_plan_handles_runtime_state_queries() {
        let plan = classify_query_verification_plan("帮我确认 python runtime 现在是否可用")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::StateFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RuntimeStateCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::RuntimeSurface));
    }

    #[test]
    fn classify_query_verification_plan_handles_runtime_ready_confirmation_queries() {
        let plan = classify_query_verification_plan("帮我确认 quickjs runtime 已经准备好了吗")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::StateFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RuntimeStateCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::RuntimeSurface));
    }

    #[test]
    fn classify_query_verification_plan_handles_english_runtime_ready_queries() {
        let plan = classify_query_verification_plan("is quickjs ready right now")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::StateFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RuntimeStateCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::RuntimeSurface));
    }

    #[test]
    fn classify_query_verification_plan_handles_current_system_state_queries() {
        let plan =
            classify_query_verification_plan("帮我看当前系统状态").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::StateFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::RuntimeStateCheck);
    }

    #[test]
    fn classify_query_verification_plan_handles_file_change_confirmation_queries() {
        let plan =
            classify_query_verification_plan("帮我看文件是否改了").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ExecutionFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ExecutionResultCheck);
    }

    #[test]
    fn classify_query_verification_plan_handles_git_status_confirmation_queries() {
        let plan = classify_query_verification_plan("帮我确认当前目录有没有未提交改动")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ExecutionFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ExecutionResultCheck);
    }

    #[test]
    fn classify_query_verification_plan_handles_english_git_status_confirmation_queries() {
        let plan = classify_query_verification_plan("check whether git status shows changes")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ExecutionFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ExecutionResultCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::ExternalCliTools));
    }

    #[test]
    fn classify_query_verification_plan_handles_english_execution_completion_queries() {
        let plan = classify_query_verification_plan("did git status show changes")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ExecutionFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ExecutionResultCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::ExternalCliTools));
    }

    #[test]
    fn classify_query_verification_plan_handles_external_tool_invocation_queries() {
        let plan =
            classify_query_verification_plan("帮我调用 ffmpeg 跑一下").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ExecutionFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ExecutionResultCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::ExternalCliTools));
    }

    #[test]
    fn classify_query_verification_plan_handles_runtime_invocation_queries() {
        let plan = classify_query_verification_plan("帮我调用 python runtime 跑一下")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::ExecutionFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ExecutionResultCheck);
        assert_eq!(plan.route_hint, Some(CapabilityRouteHint::RuntimeSurface));
    }

    #[test]
    fn classify_query_verification_plan_handles_medical_high_risk_queries() {
        let plan = classify_query_verification_plan("我现在胸口疼要不要立刻吃药")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::WebSearchFetch);
        assert_eq!(plan.route_hint, None);
    }

    #[test]
    fn classify_query_verification_plan_handles_legal_high_risk_queries() {
        let plan = classify_query_verification_plan("这个合同这样签有没有法律风险，我该不该签")
            .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::WebSearchFetch);
        assert_eq!(plan.route_hint, None);
    }

    #[test]
    fn classify_query_verification_plan_handles_financial_high_risk_queries() {
        let plan =
            classify_query_verification_plan("我现在应该怎么报税最省").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::WebSearchFetch);
        assert_eq!(plan.route_hint, None);
    }

    #[test]
    fn classify_query_verification_plan_allows_local_context_for_explanations() {
        let plan = classify_query_verification_plan("什么是 git").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(
            plan.requirement,
            VerificationRequirement::LocalContextAllowed
        );
        assert_eq!(plan.mode, VerificationMode::LocalContextOnly);
        assert_eq!(plan.route_hint, None);
    }

    #[test]
    fn classify_query_verification_plan_allows_local_context_for_static_external_entity_explanations(
    ) {
        let plan = classify_query_verification_plan("介绍一下 OpenAI").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(
            plan.requirement,
            VerificationRequirement::LocalContextAllowed
        );
        assert_eq!(plan.mode, VerificationMode::LocalContextOnly);
        assert_eq!(plan.route_hint, None);
    }

    #[test]
    fn classify_query_verification_plan_allows_local_context_for_english_static_external_entity_explanations(
    ) {
        let plan =
            classify_query_verification_plan("tell me about OpenAI").expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(
            plan.requirement,
            VerificationRequirement::LocalContextAllowed
        );
        assert_eq!(plan.mode, VerificationMode::LocalContextOnly);
        assert_eq!(plan.route_hint, None);
    }

    #[test]
    fn classify_query_verification_plan_keeps_document_understanding_hard_gate() {
        let plan = classify_query_verification_plan_with_request(
            "帮我总结这个 PDF 附件",
            CapabilityRouteRequest {
                force_document_understanding: true,
                has_media_input: true,
                ..Default::default()
            },
        )
        .expect("verification plan");

        assert_eq!(plan.domain, VerificationDomain::KnowledgeFact);
        assert_eq!(plan.requirement, VerificationRequirement::Required);
        assert_eq!(plan.mode, VerificationMode::ToolLookup);
        assert_eq!(
            plan.route_hint,
            Some(CapabilityRouteHint::DocumentUnderstanding)
        );
    }

    #[test]
    fn capability_router_exposes_shared_route_policies() {
        let router = CapabilityRouter::default();

        assert_eq!(
            router.route_debug_label(CapabilityRouteHint::DocumentUnderstanding),
            "document hard route"
        );
        assert_eq!(
            router.route_debug_label(CapabilityRouteHint::RuntimeSurface),
            "runtime_surface hard route"
        );
        assert!(router.route_requires_real_tool_call(CapabilityRouteHint::DocumentUnderstanding));
        assert!(
            router.route_requires_real_tool_call(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert!(router.route_requires_real_tool_call(CapabilityRouteHint::ExternalCliTools));
        assert!(!router.route_requires_real_tool_call(CapabilityRouteHint::General));
        assert_eq!(
            router.preferred_tool_names(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            )),
            &["web_search", "web_fetch", "browser_browse", "tool_search"]
        );
        assert_eq!(
            router.preferred_tool_names(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::FxLookup
            )),
            &["web_search", "web_fetch", "browser_browse", "tool_search"]
        );
        assert_eq!(
            router.preferred_tool_names(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WeatherLookup
            )),
            &["web_search", "web_fetch", "browser_browse", "tool_search"]
        );
        assert_eq!(
            router.preferred_tool_names(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::LatestInfoLookup
            )),
            &["web_search", "web_fetch", "browser_browse", "tool_search"]
        );
    }

    #[test]
    fn capability_router_exposes_shared_clarification_hints() {
        let router = CapabilityRouter::default();
        assert_eq!(
            router.clarification_hint("帮我查一下价格"),
            Some(CapabilityClarificationHint::MissingPriceTarget)
        );
        assert_eq!(
            router.clarification_hint("帮我查一下汇率"),
            Some(CapabilityClarificationHint::MissingFxPair)
        );
        assert_eq!(
            router.clarification_hint("明天天气怎么样"),
            Some(CapabilityClarificationHint::MissingWeatherLocation)
        );
        assert_eq!(router.clarification_hint("BTC 现在多少钱"), None);
        assert_eq!(router.clarification_hint("美元兑人民币汇率"), None);
        assert_eq!(router.clarification_hint("上海明天天气怎么样"), None);
    }

    #[tokio::test]
    async fn catalog_infers_source_scope_and_tags() {
        let toolset = ToolSet::new();
        toolset.add(SearchTool).add(PdfTool);

        let catalog = toolset.catalog().await;
        let search = catalog
            .iter()
            .find(|entry| entry.name == "web_search")
            .unwrap();
        assert_eq!(search.source, "builtin");
        assert_eq!(search.scope, "agent");
        assert_eq!(search.capability_domain, "realtime_lookup.web");
        assert!(search.tags.iter().any(|tag| tag == "search"));
    }

    #[tokio::test]
    async fn catalog_applies_runtime_registration_overrides() {
        let toolset = ToolSet::new();
        toolset.add(SearchTool);
        toolset.annotate_catalog_entry(
            "web_search",
            ToolCatalogOverride {
                source: Some("skill".into()),
                scope: Some("agent".into()),
                capability_domain: Some("runtime_surface".into()),
                tags: vec!["skill".into(), "runtime_surface".into()],
            },
        );

        let catalog = toolset.catalog().await;
        let search = catalog
            .iter()
            .find(|entry| entry.name == "web_search")
            .unwrap();
        assert_eq!(search.source, "skill");
        assert_eq!(search.scope, "agent");
        assert_eq!(search.capability_domain, "runtime_surface");
        assert!(search.tags.iter().any(|tag| tag == "skill"));
    }

    #[tokio::test]
    async fn add_shared_with_catalog_registers_tool_and_override_together() {
        let toolset = ToolSet::new();
        let tool: Arc<dyn Tool> = Arc::new(SearchTool);
        toolset.add_shared_with_catalog(
            tool,
            ToolCatalogOverride {
                source: Some("forge".into()),
                scope: Some("session".into()),
                capability_domain: Some("runtime_surface".into()),
                tags: vec!["forge".into(), "session".into()],
            },
        );

        assert!(toolset.contains("web_search"));
        let catalog = toolset.catalog().await;
        let search = catalog
            .iter()
            .find(|entry| entry.name == "web_search")
            .unwrap();
        assert_eq!(search.source, "forge");
        assert_eq!(search.scope, "session");
        assert_eq!(search.capability_domain, "runtime_surface");
        assert!(search.tags.iter().any(|tag| tag == "forge"));
        assert!(search.tags.iter().any(|tag| tag == "session"));
    }

    #[tokio::test]
    async fn context_injector_defers_long_tail_tools_from_prompt_index() {
        let toolset = ToolSet::new();
        toolset
            .add(SearchTool)
            .add(PdfTool)
            .add(ChartTool)
            .add(GitTool)
            .add(WeatherTool)
            .add(ToolSearchIndexTool)
            .add(ReadSkillManualIndexTool)
            .add(ReadSkillAssetIndexTool)
            .add(RuntimeSurfaceIndexTool)
            .add(McpSqlTool);
        toolset.annotate_catalog_entry(
            "mcp_sql",
            ToolCatalogOverride {
                source: Some("mcp".into()),
                scope: Some("agent".into()),
                capability_domain: Some("external_cli_tools".into()),
                tags: vec!["mcp".into()],
            },
        );

        let injected = crate::agent::context::ContextInjector::inject(&toolset, &[])
            .await
            .expect("inject");
        let content = injected.first().expect("system message").content.as_text();

        assert!(content.contains("tool_search"));
        assert!(content.contains("read_skill_manual"));
        assert!(content.contains("read_skill_asset"));
        assert!(content.contains("runtime_surface"));
        assert!(content.contains("web_search"));
        assert!(!content.contains("git_adapter"));
        assert!(!content.contains("mcp_sql"));
        assert!(content.contains("additional tools are intentionally deferred"));
    }

    #[test]
    fn execution_routes_do_not_inject_system_prompt_by_default() {
        assert!(!capability_route_should_inject_system_message(
            CapabilityRouteHint::DocumentUnderstanding
        ));
        assert!(!capability_route_should_inject_system_message(
            CapabilityRouteHint::FileOps
        ));
        assert!(!capability_route_should_inject_system_message(
            CapabilityRouteHint::RuntimeSurface
        ));
        assert!(!capability_route_should_inject_system_message(
            CapabilityRouteHint::ExternalCliTools
        ));

        assert!(capability_route_should_inject_system_message(
            CapabilityRouteHint::Writing
        ));
        assert!(capability_route_should_inject_system_message(
            CapabilityRouteHint::Coding
        ));
        assert!(capability_route_should_inject_system_message(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup)
        ));
    }

    #[test]
    fn coordinator_task_mode_matches_route_and_media_shape() {
        assert_eq!(
            select_coordinator_task_mode(None, false),
            CoordinatorTaskMode::ChatLite
        );
        assert_eq!(
            select_coordinator_task_mode(None, true),
            CoordinatorTaskMode::VisionLite
        );
        assert_eq!(
            select_coordinator_task_mode(Some(CapabilityRouteHint::VisualUnderstanding), false),
            CoordinatorTaskMode::VisionLite
        );
        assert_eq!(
            select_coordinator_task_mode(Some(CapabilityRouteHint::DocumentUnderstanding), false),
            CoordinatorTaskMode::DocumentLite
        );
        assert_eq!(
            select_coordinator_task_mode(Some(CapabilityRouteHint::Coding), false),
            CoordinatorTaskMode::ToolAgent
        );
    }

    #[test]
    fn routing_judgment_only_queries_are_detected() {
        assert!(query_requests_routing_judgment_only(
            "如果我要执行 Windows 命令，你应该把任务交给谁？不要直接执行，只说路由判断。"
        ));
        assert!(query_requests_routing_judgment_only(
            "route only: who should handle this, do not execute"
        ));
        assert!(!query_requests_routing_judgment_only(
            "帮我执行一个 PowerShell 命令并告诉我输出"
        ));
    }

    #[test]
    fn coordinator_prompt_components_are_scoped_by_task_mode() {
        assert!(!coordinator_task_mode_should_include_reasoning_prompt(
            CoordinatorTaskMode::ChatLite,
            &ReasoningStrategy::Reflexion
        ));
        assert!(!coordinator_task_mode_should_include_reasoning_prompt(
            CoordinatorTaskMode::VisionLite,
            &ReasoningStrategy::Reflexion
        ));
        assert!(coordinator_task_mode_should_include_reasoning_prompt(
            CoordinatorTaskMode::ToolAgent,
            &ReasoningStrategy::Reflexion
        ));

        assert!(!coordinator_task_mode_should_include_media_followup_prompt(
            CoordinatorTaskMode::ChatLite,
            true
        ));
        assert!(coordinator_task_mode_should_include_media_followup_prompt(
            CoordinatorTaskMode::DocumentLite,
            true
        ));

        assert!(!coordinator_task_mode_should_include_route_prompt(
            CoordinatorTaskMode::ChatLite,
            CapabilityRouteHint::Coding
        ));
        assert!(coordinator_task_mode_should_include_route_prompt(
            CoordinatorTaskMode::ToolAgent,
            CapabilityRouteHint::Coding
        ));
    }

    #[test]
    fn coordinator_task_modes_do_not_require_prompt_tool_index() {
        assert!(!coordinator_task_mode_should_include_tool_index(
            CoordinatorTaskMode::ChatLite
        ));
        assert!(!coordinator_task_mode_should_include_tool_index(
            CoordinatorTaskMode::VisionLite
        ));
        assert!(!coordinator_task_mode_should_include_tool_index(
            CoordinatorTaskMode::DocumentLite
        ));
        assert!(!coordinator_task_mode_should_include_tool_index(
            CoordinatorTaskMode::ToolAgent
        ));
    }

    #[test]
    fn coordinator_specialist_domains_follow_route_before_mode() {
        assert_eq!(
            coordinator_preferred_specialist_domains(
                CoordinatorTaskMode::ToolAgent,
                Some(CapabilityRouteHint::DocumentUnderstanding),
                None,
                false
            ),
            &[
                "document_understanding",
                "ocr",
                "image",
                "voice_understanding"
            ]
        );
        assert_eq!(
            coordinator_preferred_specialist_domains(
                CoordinatorTaskMode::ToolAgent,
                Some(CapabilityRouteHint::RuntimeSurface),
                None,
                false
            ),
            &["runtime_surface", "coding"]
        );
    }

    #[test]
    fn coordinator_specialist_domains_stay_empty_for_plain_chat() {
        assert!(coordinator_preferred_specialist_domains(
            CoordinatorTaskMode::ChatLite,
            None,
            None,
            false
        )
        .is_empty());
    }

    #[test]
    fn coordinator_specialist_selection_message_uses_media_fallback_hint() {
        let message = coordinator_specialist_selection_message(
            CoordinatorTaskMode::VisionLite,
            None,
            None,
            true,
        );
        assert!(
            message.is_none(),
            "frontstage vision/media turns should answer directly before specialist selection"
        );
    }

    #[test]
    fn extended_preflight_trigger_is_finer_than_old_route_blanket() {
        assert!(!should_run_extended_pre_flight_for_turn(
            "帮我记住今天讨论的风格",
            Some(CapabilityRouteHint::Memory),
            false,
        ));
        assert!(!should_run_extended_pre_flight_for_turn(
            "帮我写一句提醒",
            Some(CapabilityRouteHint::Communication),
            false,
        ));
        assert!(should_run_extended_pre_flight_for_turn(
            "请给我今天最新价格和来源链接",
            None,
            false,
        ));
        assert!(should_run_extended_pre_flight_for_turn(
            "帮我看看这个仓库并改代码",
            Some(CapabilityRouteHint::Coding),
            false,
        ));
    }

    #[test]
    fn extended_preflight_levels_split_light_complex_and_high_risk() {
        assert_eq!(
            classify_extended_pre_flight_level(
                "帮我记住今天讨论的风格",
                Some(CapabilityRouteHint::Memory),
                false
            ),
            ExtendedPreFlightLevel::None
        );
        assert_eq!(
            classify_extended_pre_flight_level(
                "第一部分：把现有聊天上下文整理成几个主题。\n第二部分：把每个主题拆成待办。\n第三部分：给出一个结构化输出。",
                None,
                false
            ),
            ExtendedPreFlightLevel::ComplexTask
        );
        assert_eq!(
            classify_extended_pre_flight_level(
                "告诉我今天 OpenAI API 定价并给我来源链接",
                Some(CapabilityRouteHint::RealtimeLookup(
                    RealtimeLookupKind::LatestInfoLookup
                )),
                false
            ),
            ExtendedPreFlightLevel::HighRiskTask
        );
        assert!(extended_pre_flight_runs_complexity_estimator(
            ExtendedPreFlightLevel::ComplexTask
        ));
        assert!(extended_pre_flight_runs_jit_distillation(
            ExtendedPreFlightLevel::HighRiskTask
        ));
        assert!(!extended_pre_flight_allows_auto_stepdown(
            ExtendedPreFlightLevel::HighRiskTask
        ));
    }

    #[test]
    fn only_realtime_lookup_prefers_direct_tool_surface() {
        assert!(capability_route_prefers_direct_tool_surface(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup)
        ));
        assert!(!capability_route_prefers_direct_tool_surface(
            CapabilityRouteHint::DocumentUnderstanding
        ));
        assert!(!capability_route_prefers_direct_tool_surface(
            CapabilityRouteHint::RuntimeSurface
        ));
        assert!(!capability_route_prefers_direct_tool_surface(
            CapabilityRouteHint::Coding
        ));
    }

    #[test]
    fn compound_realtime_requests_keep_followup_execution_enabled() {
        let query = "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。";

        assert!(query_requests_followup_execution_after_lookup(query));
        assert!(query_requests_followup_execution_after_lookup(
            "请搜索起点中文网免费玄幻小说，把公开元数据放进知识库。"
        ));
        assert!(query_requests_followup_execution_after_lookup(
            "请整理这些资料并收进知识库。"
        ));
        assert!(query_requests_followup_execution_after_lookup(
            "查找柳叶刀最新治疗心脏病的论文，然后存入数据库。"
        ));
        assert!(query_requests_followup_execution_after_lookup(
            "Find recent papers and save them to the document store."
        ));
        assert!(!capability_route_prefers_direct_tool_surface_for_query(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup),
            query
        ));

        let allowed = capability_route_tool_allowlist_for_query(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup),
            Some(query),
        );
        assert!(allowed.contains("web_search"));
        assert!(allowed.contains("web_fetch"));
        assert!(allowed.contains("browser_browse"));
        assert!(allowed.contains("delegate"));
        assert!(allowed.contains("tool_search"));
        assert!(!allowed.contains("latest_info_lookup"));
    }

    #[test]
    fn compound_realtime_prompt_forces_phase_two_execution() {
        let query = "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。";
        let prompt = build_realtime_lookup_hard_route_system_message(
            query,
            RealtimeLookupKind::LatestInfoLookup,
        );

        assert!(prompt.contains("downstream action after lookup"));
        assert!(prompt.contains("Treat lookup completion as phase 1 only"));
        assert!(prompt.contains("you must not stop while any downstream execution"));
        assert!(prompt.contains("Use `delegate` when the follow-up belongs to a specialist"));
        assert!(prompt.contains("preserve the full original request"));
    }

    #[test]
    fn compound_realtime_preferred_tools_become_coordinator_first() {
        let query = "请搜索柳叶刀最新治疗心脏病的论文，并保存进知识库。";
        let preferred = capability_route_preferred_tool_names_for_query(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup),
            query,
        );

        assert_eq!(preferred.first().copied(), Some("delegate"));
        assert!(preferred.contains(&"web_search"));
        assert!(preferred.contains(&"web_fetch"));
        assert!(preferred.contains(&"browser_browse"));
        assert!(!preferred.contains(&"latest_info_lookup"));
    }

    #[test]
    fn plain_realtime_requests_still_use_direct_tool_surface() {
        let query = "请帮我查 OpenAI 今天的最新消息";

        assert!(!query_requests_followup_execution_after_lookup(query));
        assert!(capability_route_prefers_direct_tool_surface_for_query(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup),
            query
        ));

        let allowed = capability_route_tool_allowlist_for_query(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup),
            Some(query),
        );
        assert!(allowed.contains("latest_info_lookup"));
        assert!(allowed.contains("web_search"));
        assert!(allowed.contains("web_fetch"));
        assert!(allowed.contains("browser_browse"));
        assert!(!allowed.contains("delegate"));
    }

    #[test]
    fn negated_knowledge_persistence_does_not_force_followup_execution() {
        assert!(!query_requests_followup_execution_after_lookup(
            "请测试 web_fetch，不要写入知识库。"
        ));
        assert!(!query_requests_followup_execution_after_lookup(
            "Please fetch this URL, do not save to the knowledge base."
        ));
        assert!(!query_requests_followup_execution_after_lookup(
            "请测试 web_fetch，不要写入数据库。"
        ));
    }

    #[test]
    fn knowledge_base_readback_queries_delegate_to_knowledge_worker() {
        let query = "请从知识库里读出你刚刚保存的那条资料，告诉我标题、doi 和一句摘要。";
        let allowed = coordinator_default_tool_names_for_query(Some(query));

        assert!(allowed.contains("delegate"));
        assert!(!allowed.contains("knowledge_search"));
        assert!(!allowed.contains("tiered_search"));
        assert!(!allowed.contains("fetch_document"));
        assert!(!allowed.contains("handover"));
        assert!(!allowed.contains("manage_facts"));
        assert!(!allowed.contains("shared_board"));
        assert!(!allowed.contains("tool_search"));
        assert!(!allowed.contains("remember_this"));
        assert!(!allowed.contains("read_skill_manual"));
        assert!(!allowed.contains("read_skill_asset"));
    }

    #[test]
    fn natural_knowledge_lookup_queries_delegate_to_knowledge_worker() {
        let query = "从知识库里查一下 worker-chat-ok，只返回结果或未找到。";

        assert!(query_prefers_knowledge_base_retrieval(query));
        assert!(query_prefers_knowledge_base_retrieval(
            "从数据库里查一下 worker-chat-ok，只返回结果或未找到。"
        ));
        assert!(!query_prefers_knowledge_base_retrieval(
            "把这篇论文存入数据库，之后再总结。"
        ));

        let allowed = coordinator_chat_lite_tool_names_for_query(Some(query));
        assert_eq!(allowed, ["delegate".to_string()].into_iter().collect());

        let preferred =
            capability_route_preferred_tool_names_for_query(CapabilityRouteHint::Memory, query);
        assert_eq!(preferred, vec!["delegate"]);
    }

    #[test]
    fn chat_lite_plain_chat_has_no_default_orchestration_tools() {
        let allowed = coordinator_chat_lite_tool_names_for_query(Some("你好，随便聊两句"));

        assert!(allowed.is_empty());
    }

    #[test]
    fn structured_realtime_queries_expose_only_the_direct_runtime_tool() {
        let weather = capability_route_tool_allowlist_for_query(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WeatherLookup),
            Some("北京今天天气怎么样"),
        );
        assert!(weather.contains("weather_lookup"));
        assert_eq!(weather.len(), 1);
        assert!(!weather.contains("delegate"));
        assert!(!weather.contains("shared_board"));

        let price = capability_route_tool_allowlist_for_query(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::PriceLookup),
            Some("纳斯达克点数多少？"),
        );
        assert!(price.contains("price_lookup"));
        assert_eq!(price.len(), 1);
        assert!(!price.contains("delegate"));
        assert!(!price.contains("shared_board"));
    }

    #[test]
    fn explicit_worker_realtime_request_keeps_delegate_surface() {
        let allowed = capability_route_tool_allowlist_for_query(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup),
            Some("请让 researcher 搜索今天人工智能领域的最新新闻。"),
        );

        assert!(allowed.contains("delegate"));
        assert!(!allowed.contains("latest_info_lookup"));
    }

    #[test]
    fn realtime_route_prompt_prefers_structured_tool_before_search_fallback() {
        let price_prompt = build_realtime_lookup_hard_route_system_message(
            "帮我查 BTC 现在价格",
            RealtimeLookupKind::PriceLookup,
        );
        assert!(price_prompt.contains("structured `price_lookup`"));
        assert!(price_prompt.contains("Use `web_search`/`web_fetch` only as fallback"));
        assert!(!price_prompt.contains("Prefer `web_search` directly"));

        let weather_prompt = build_realtime_lookup_hard_route_system_message(
            "北京今天天气怎么样",
            RealtimeLookupKind::WeatherLookup,
        );
        assert!(weather_prompt.contains("structured `weather_lookup`"));
        assert!(!weather_prompt.contains("Prefer `web_search` directly"));
    }

    #[test]
    fn memory_route_prefers_knowledge_worker_for_knowledge_base_readback() {
        let query = "请从知识库里读出刚保存的资料内容。";
        let preferred =
            capability_route_preferred_tool_names_for_query(CapabilityRouteHint::Memory, query);

        assert_eq!(preferred, vec!["delegate"]);
        assert!(!preferred.contains(&"manage_facts"));
    }

    #[test]
    fn memory_route_prefers_knowledge_domain_for_knowledge_base_readback() {
        let query = "请从知识库里读出刚保存的资料内容。";
        assert_eq!(
            coordinator_preferred_specialist_domains(
                CoordinatorTaskMode::ToolAgent,
                Some(CapabilityRouteHint::Memory),
                Some(query),
                false
            ),
            &["knowledge"]
        );
        assert!(coordinator_specialist_selection_message(
            CoordinatorTaskMode::ToolAgent,
            Some(CapabilityRouteHint::Memory),
            Some(query),
            false
        )
        .is_some());
    }

    #[test]
    fn personal_memory_recall_uses_search_history_not_fact_management_or_knowledge_worker() {
        let query = "你还记得我的面板记忆架构测试标记是什么吗？只回答 marker 本身。";

        assert_eq!(
            classify_query_capability_route(query),
            Some(CapabilityRouteHint::Memory)
        );

        let preferred =
            capability_route_preferred_tool_names_for_query(CapabilityRouteHint::Memory, query);
        assert_eq!(preferred, vec!["search_history"]);

        let allowed =
            capability_route_tool_allowlist_for_query(CapabilityRouteHint::Memory, Some(query));
        assert!(allowed.contains("search_history"));
        assert!(!allowed.contains("delegate"));
        assert!(!allowed.contains("remember_this"));
        assert!(!allowed.contains("manage_facts"));

        assert_eq!(
            coordinator_preferred_specialist_domains(
                CoordinatorTaskMode::ToolAgent,
                Some(CapabilityRouteHint::Memory),
                Some(query),
                false
            ),
            &["memory"]
        );
    }
}
