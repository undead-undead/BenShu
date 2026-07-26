//! Tool system for AI agents
//!
//! Provides the core abstraction for defining tools that AI agents can call.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc; // This is used by Arc<parking_lot::RwLock<HashMap<String, Arc<dyn Tool>>>> // This is used by HashMap in ToolSet

use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};
use benshu_state::{ArtifactLifecycle, ArtifactManager, ArtifactRecord};

pub mod board;
#[cfg(feature = "browser")]
pub mod browser;
#[cfg(feature = "http")]
#[path = "browser/site_policy.rs"]
pub mod browser_site_policy;
#[cfg(feature = "http")]
pub mod chart;
#[cfg(feature = "http")]
pub mod cipher;
pub mod command_exec;
#[cfg(feature = "cron")]
pub mod cron;
#[cfg(feature = "http")]
pub mod data_transform;
pub mod delegation;
pub mod desktop_sense;
#[cfg(feature = "http")]
pub mod document_understand;
pub mod filesystem;
pub mod forge;
#[cfg(feature = "http")]
pub mod git_ops;
pub mod handover;
pub mod image;
#[cfg(feature = "http")]
pub mod knowledge_import;
#[cfg(feature = "http")]
pub mod knowledge_manage;
#[cfg(feature = "http")]
pub mod mailer;
pub mod media_runtime;
#[cfg(feature = "vector-db")]
pub mod memory;
#[cfg(feature = "http")]
pub mod notifier;
#[cfg(feature = "http")]
pub mod office_parse;
#[cfg(feature = "http")]
pub mod pdf_parse;
#[cfg(feature = "http")]
pub mod realtime_lookup;
pub mod refine;
pub mod runtime_surface;
pub mod sandbox_ctl;
pub mod skill_manager;
pub mod swarm;
pub mod swarm_broadcast;
pub mod system_monitor;
#[cfg(feature = "http")]
pub mod text_extract;
pub mod tool_catalog;
pub mod tool_search;
pub mod vault;
pub mod visual;
#[cfg(feature = "http")]
pub mod voice;
#[cfg(feature = "http")]
pub mod web_fetch;
#[cfg(any(feature = "http", feature = "browser"))]
pub mod web_search;
pub mod windows_control;
pub mod writing;

#[cfg(feature = "browser")]
pub use browser::BrowserTool;
#[cfg(feature = "cron")]
pub use cron::CronTool;
pub use delegation::DelegateTool;
pub use filesystem::{EditFileTool, ListDirTool, ReadFileTool, WriteFileTool};
pub use forge::ForgeSkill;
pub use handover::HandoverTool;
#[cfg(feature = "vector-db")]
pub use memory::{
    FactManagementTool, FetchDocumentTool, MultimodalMemoryTool, RememberThisTool,
    SearchHistoryTool, TieredSearchTool,
};
#[cfg(feature = "http")]
pub use office_parse::OfficeParseTool;
#[cfg(feature = "http")]
pub use pdf_parse::PdfParseTool;
#[cfg(feature = "http")]
pub use realtime_lookup::{FxLookupTool, LatestInfoLookupTool, PriceLookupTool, WeatherLookupTool};
pub use refine::RefineSkill;
pub use runtime_surface::RuntimeSurfaceTool;
#[cfg(feature = "http")]
pub use web_fetch::WebFetchTool;
#[cfg(feature = "http")]
pub use web_search::WebSearchTool;
pub use windows_control::WindowsControlTool;

pub use board::SharedBoardTool;
#[cfg(feature = "http")]
pub use chart::ChartTool;
#[cfg(feature = "http")]
pub use cipher::CipherTool;
pub use command_exec::CommandExecTool;
#[cfg(feature = "http")]
pub use data_transform::DataTransformTool;
pub use desktop_sense::DesktopSenseTool;
#[cfg(feature = "http")]
pub use document_understand::DocumentUnderstandTool;
#[cfg(feature = "http")]
pub use git_ops::GitOpsTool;
pub use image::GenerateImageTool;
#[cfg(feature = "http")]
pub use knowledge_import::KnowledgeImportUrlTool;
#[cfg(feature = "http")]
pub use knowledge_manage::KnowledgeManageDocumentTool;
#[cfg(feature = "http")]
pub use mailer::MailerTool;
pub use media_runtime::{
    ExtractAudioTrackTool, ExtractVideoFramesTool, NormalizeAudioTool, ProbeMediaTool,
    RenderVideoThumbnailTool,
};
#[cfg(feature = "http")]
pub use notifier::NotifierTool;
pub use sandbox_ctl::SandboxConfiguratorTool;
pub use skill_manager::SkillManagerTool;
pub use swarm::MultiAgentAuditTool;
pub use swarm_broadcast::SwarmBroadcastTool;
pub use system_monitor::SystemMonitorTool;
#[cfg(feature = "http")]
pub use text_extract::TextExtractTool;
pub use tool_catalog::ToolCatalogTool;
pub use tool_search::ToolSearchTool;
pub use vault::VaultManagerTool;
pub use visual::VisualAnalysisTool;
#[cfg(feature = "http")]
pub use voice::{SpeakTool, TranscribeTool};
pub use writing::{NovelStudioTool, WritingStudioTool};

// Tool and ToolDefinition are imported from infra so concrete tools do not
// depend on the brain crate for the generic tool protocol.

pub const TOOL_DEGRADATION_SCHEMA_VERSION: &str = "benshu.builtin_tools.degradation.v1";
pub const TOOL_CLEANUP_SCHEMA_VERSION: &str = "benshu.builtin_tools.cleanup.v1";
pub const TOOL_ARTIFACT_REGISTRATION_SCHEMA_VERSION: &str =
    "benshu.builtin_tools.artifact_registration.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDegradation {
    pub schema_version: String,
    pub active: bool,
    pub kind: String,
    pub reason: String,
    pub user_message: String,
    pub fallback_path: String,
    pub retryable: bool,
}

impl ToolDegradation {
    pub fn inactive() -> Self {
        Self {
            schema_version: TOOL_DEGRADATION_SCHEMA_VERSION.to_string(),
            active: false,
            kind: "none".to_string(),
            reason: "fully_available".to_string(),
            user_message: "All capabilities are available.".to_string(),
            fallback_path: "none".to_string(),
            retryable: false,
        }
    }

    pub fn active(
        kind: impl Into<String>,
        reason: impl Into<String>,
        user_message: impl Into<String>,
        fallback_path: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            schema_version: TOOL_DEGRADATION_SCHEMA_VERSION.to_string(),
            active: true,
            kind: kind.into(),
            reason: reason.into(),
            user_message: user_message.into(),
            fallback_path: fallback_path.into(),
            retryable,
        }
    }

    pub fn as_json(&self) -> serde_json::Value {
        json!(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCleanup {
    pub schema_version: String,
    pub active: bool,
    pub kind: String,
    pub reason: String,
    pub user_message: String,
    pub cleanup_hint: String,
    pub auto_cleanup_performed: bool,
}

impl ToolCleanup {
    pub fn inactive() -> Self {
        Self {
            schema_version: TOOL_CLEANUP_SCHEMA_VERSION.to_string(),
            active: false,
            kind: "none".to_string(),
            reason: "fully_managed".to_string(),
            user_message: "Temporary execution files are already managed by the tool.".to_string(),
            cleanup_hint: "none".to_string(),
            auto_cleanup_performed: true,
        }
    }

    pub fn active(
        kind: impl Into<String>,
        reason: impl Into<String>,
        user_message: impl Into<String>,
        cleanup_hint: impl Into<String>,
        auto_cleanup_performed: bool,
    ) -> Self {
        Self {
            schema_version: TOOL_CLEANUP_SCHEMA_VERSION.to_string(),
            active: true,
            kind: kind.into(),
            reason: reason.into(),
            user_message: user_message.into(),
            cleanup_hint: cleanup_hint.into(),
            auto_cleanup_performed,
        }
    }

    pub fn as_json(&self) -> serde_json::Value {
        json!(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolArtifactRegistration {
    pub schema_version: String,
    pub registered: bool,
    pub artifact_id: String,
    pub uri: String,
    pub scope: String,
    pub lifecycle: String,
    pub source_kind: String,
}

impl ToolArtifactRegistration {
    pub fn from_record(record: &ArtifactRecord) -> Self {
        Self {
            schema_version: TOOL_ARTIFACT_REGISTRATION_SCHEMA_VERSION.to_string(),
            registered: true,
            artifact_id: record.artifact_id.clone(),
            uri: record.uri.clone(),
            scope: serde_json::to_value(&record.scope)
                .ok()
                .and_then(|v| v.as_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| "outputs".to_string()),
            lifecycle: serde_json::to_value(&record.lifecycle)
                .ok()
                .and_then(|v| v.as_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| "session".to_string()),
            source_kind: record.source_kind.clone(),
        }
    }

    pub fn as_json(&self) -> serde_json::Value {
        json!(self)
    }
}

pub async fn register_tool_output_artifact(
    manager: &ArtifactManager,
    agent_id: &str,
    tool_name: &str,
    uri: &str,
    lifecycle: ArtifactLifecycle,
    kind: &str,
    metadata: HashMap<String, String>,
) -> anyhow::Result<ArtifactRecord> {
    let now = Utc::now();
    let record = ArtifactRecord {
        artifact_id: uuid::Uuid::new_v4().to_string(),
        kind: kind.to_string(),
        uri: uri.to_string(),
        scope: ArtifactManager::classify_scope(uri, None),
        lifecycle,
        created_at: now,
        updated_at: now,
        agent_id: agent_id.to_string(),
        task_id: None,
        run_id: None,
        trace_id: None,
        session_id: None,
        thread_id: None,
        tool_name: Some(tool_name.to_string()),
        media_type: None,
        virtual_path: None,
        source_kind: "builtin_tool_output".to_string(),
        metadata,
    };
    manager.save(record.clone()).await?;
    Ok(record)
}

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

#[derive(Clone)]
pub struct ToolSet {
    tools: Arc<parking_lot::RwLock<HashMap<String, Arc<dyn Tool>>>>,
    /// Cached definitions to avoid async calls during prompt generation
    cached_definitions: Arc<parking_lot::RwLock<HashMap<String, ToolDefinition>>>,
}

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

    /// Call a tool by name
    pub async fn call(&self, name: &str, arguments: &str) -> anyhow::Result<String> {
        let tool = { self.tools.read().get(name).cloned() }
            .ok_or_else(|| Error::ToolNotFound(name.to_string()))?;

        tool.call(arguments).await
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
}

#[async_trait::async_trait]
impl benshu_brain::agent::context::ContextInjector for ToolSet {
    async fn inject(
        &self,
        _history: &[benshu_brain::agent::message::Message],
    ) -> benshu_brain::error::Result<Vec<benshu_brain::agent::message::Message>> {
        if self.is_empty() {
            return Ok(Vec::new());
        }

        let mut content = String::from("## Available Tools (Index)\n\n");
        content.push_str(
            "You have access to the following tools. To save context, only descriptions are shown below. \
             Full TypeScript schemas and usage guidelines will be automatically injected into the conversation \
             the first time you use a specific tool.\n\n",
        );

        let mut sorted_tools: Vec<_> = self.iter();
        sorted_tools.sort_by_key(|(k, _)| k.clone());

        for (name, tool) in sorted_tools {
            let cached_def = { self.cached_definitions.read().get(&name).cloned() };

            let def = if let Some(d) = cached_def {
                d
            } else {
                let d = tool.definition().await;
                self.cached_definitions
                    .write()
                    .insert(name.clone(), d.clone());
                d
            };

            content.push_str(&format!("- **{}**: {}\n", name, def.description));
        }

        Ok(vec![benshu_brain::agent::message::Message::system(content)])
    }
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
///
/// # Example
/// ```ignore
/// simple_tool!(
///     name: "get_time",
///     description: "Get the current time",
///     handler: |_args| async {
///         Ok(chrono::Utc::now().to_rfc3339())
///     }
/// );
/// ```
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
        impl $crate::tool::Tool for SimpleTool {
            fn name(&self) -> String {
                $name.to_string()
            }

            async fn definition(&self) -> $crate::tool::ToolDefinition {
                $crate::tool::ToolDefinition {
                    name: $name.to_string(),
                    description: $desc.to_string(),
                    parameters: $params,
                    usage_guidelines: None,
                    safety_level: Default::default(),
                    is_binary: false,
                    is_verified: false,
                    parameters_ts: None,
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

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> String {
            "echo".to_string()
        }

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "echo".to_string(),
                description: "Echo back the input".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "Message to echo"
                        }
                    },
                    "required": ["message"]
                }),
                parameters_ts: None,
                is_binary: false,
                is_verified: true, // Internal tools are verified
                usage_guidelines: None,
                safety_level: Default::default(),
            }
        }

        async fn call(&self, arguments: &str) -> anyhow::Result<String> {
            #[derive(Deserialize)]
            struct Args {
                message: String,
            }
            let args: Args = serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: "echo".to_string(),
                message: e.to_string(),
            })?;
            Ok(args.message)
        }
    }

    #[tokio::test]
    async fn test_toolset() {
        let toolset = ToolSet::new();
        toolset.add(EchoTool);

        assert!(toolset.contains("echo"));
        assert_eq!(toolset.len(), 1);

        let result = toolset
            .call("echo", r#"{"message": "hello"}"#)
            .await
            .expect("call should succeed");
        assert_eq!(result, "hello");
    }
}
