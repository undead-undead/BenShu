use benshu_compression::preview_text;
use serde::{Deserialize, Serialize};

const BACKGROUND_SLOT_PROJECT_FACTS: &str = "background_slot.project_facts";
const BACKGROUND_SLOT_CURRENT_TASK: &str = "background_slot.current_task";
const BACKGROUND_SLOT_COMPLETED_WORK: &str = "background_slot.completed_work";
const BACKGROUND_SLOT_PENDING_WORK: &str = "background_slot.pending_work";
const BACKGROUND_SLOT_KEY_FILES: &str = "background_slot.key_files";
const BACKGROUND_SLOT_TEST_RESULTS: &str = "background_slot.test_results";
const BACKGROUND_SLOT_RISKS: &str = "background_slot.risks";
const BACKGROUND_SLOT_VERIFICATION_NEEDS: &str = "background_slot.verification_needs";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BackgroundCompressionSlots {
    #[serde(default)]
    pub project_facts: Vec<String>,
    #[serde(default)]
    pub current_task: Option<String>,
    #[serde(default)]
    pub completed_work: Vec<String>,
    #[serde(default)]
    pub pending_work: Vec<String>,
    #[serde(default)]
    pub key_files: Vec<String>,
    #[serde(default)]
    pub test_results: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub verification_needs: Vec<String>,
}

impl BackgroundCompressionSlots {
    pub fn is_empty(&self) -> bool {
        self.project_facts.is_empty()
            && self.current_task.is_none()
            && self.completed_work.is_empty()
            && self.pending_work.is_empty()
            && self.key_files.is_empty()
            && self.test_results.is_empty()
            && self.risks.is_empty()
            && self.verification_needs.is_empty()
    }

    pub fn apply_budget_caps(&mut self) {
        self.current_task = cap_optional_text(self.current_task.take(), 180);
        cap_vec_len(&mut self.project_facts, 5);
        cap_vec_text(&mut self.project_facts, 140);
        cap_vec_len(&mut self.completed_work, 5);
        cap_vec_text(&mut self.completed_work, 140);
        cap_vec_len(&mut self.pending_work, 5);
        cap_vec_text(&mut self.pending_work, 140);
        cap_vec_len(&mut self.key_files, 8);
        cap_vec_text(&mut self.key_files, 180);
        cap_vec_len(&mut self.test_results, 6);
        cap_vec_text(&mut self.test_results, 180);
        cap_vec_len(&mut self.risks, 4);
        cap_vec_text(&mut self.risks, 140);
        cap_vec_len(&mut self.verification_needs, 4);
        cap_vec_text(&mut self.verification_needs, 140);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundQualitySignal {
    Candidate,
    Guarded,
    Skipped,
    Stable,
    Strong,
    Rejected,
}

impl Default for BackgroundQualitySignal {
    fn default() -> Self {
        Self::Candidate
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BackgroundEvidenceRef {
    pub source_kind: String,
    pub source_id: String,
    pub confidence: Option<f32>,
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackgroundRevision {
    pub revision: u64,
    pub previous_revision: Option<u64>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub update_reason: Option<String>,
}

impl Default for BackgroundRevision {
    fn default() -> Self {
        Self {
            revision: 0,
            previous_revision: None,
            updated_at: chrono::Utc::now(),
            update_reason: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PersonaBackgroundLayer {
    pub identity_summary: Option<String>,
    pub speaking_style: Option<String>,
    pub relationship_frame: Option<String>,
    pub safety_notes: Vec<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

impl PersonaBackgroundLayer {
    pub fn is_empty(&self) -> bool {
        self.identity_summary.is_none()
            && self.speaking_style.is_none()
            && self.relationship_frame.is_none()
            && self.safety_notes.is_empty()
            && self.metadata.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RelationshipBackgroundLayer {
    pub user_profile_summary: Option<String>,
    pub user_preferences: Vec<String>,
    pub relationship_summary: Option<String>,
    pub long_term_topics: Vec<String>,
    pub emotional_markers: Vec<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

impl RelationshipBackgroundLayer {
    pub fn is_empty(&self) -> bool {
        self.user_profile_summary.is_none()
            && self.user_preferences.is_empty()
            && self.relationship_summary.is_none()
            && self.long_term_topics.is_empty()
            && self.emotional_markers.is_empty()
            && self.metadata.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackendContextKind {
    Artifact,
    Collection,
    Web,
    ToolResult,
    Route,
    Multimodal,
    MultimodalRoute,
    TaskState,
    MemoryRecall,
}

impl BackendContextKind {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Artifact => "Artifact context",
            Self::Collection => "Collection context",
            Self::Web => "Web context",
            Self::ToolResult => "Tool result",
            Self::Route => "Route context",
            Self::Multimodal => "Multimodal context",
            Self::MultimodalRoute => "Multimodal route",
            Self::TaskState => "Task state",
            Self::MemoryRecall => "Memory recall",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BackendContextRecord {
    pub kind: Option<BackendContextKind>,
    pub value: String,
    pub source: Option<String>,
}

impl BackendContextRecord {
    pub fn render(&self) -> String {
        let value = self.value.trim();
        if value.is_empty() {
            return String::new();
        }

        match self.kind.as_ref() {
            Some(kind) => format!("{}: {}", kind.as_label(), value),
            None => value.to_string(),
        }
    }

    pub fn normalized_key(&self) -> String {
        let kind = self
            .kind
            .as_ref()
            .map(BackendContextKind::as_label)
            .unwrap_or("context");
        format!("{kind}::{}", self.value.trim())
    }

    pub fn from_legacy_text(value: &str) -> Self {
        for (prefix, kind) in [
            ("Artifact context: ", BackendContextKind::Artifact),
            ("Collection context: ", BackendContextKind::Collection),
            ("Web context: ", BackendContextKind::Web),
            ("Tool result: ", BackendContextKind::ToolResult),
            ("Route context: ", BackendContextKind::Route),
            ("Multimodal context: ", BackendContextKind::Multimodal),
            ("Multimodal route: ", BackendContextKind::MultimodalRoute),
            ("Task state: ", BackendContextKind::TaskState),
            ("Memory recall: ", BackendContextKind::MemoryRecall),
        ] {
            if let Some(rest) = value.strip_prefix(prefix) {
                return Self {
                    kind: Some(kind),
                    value: rest.trim().to_string(),
                    source: None,
                };
            }
        }

        Self {
            kind: None,
            value: value.trim().to_string(),
            source: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RetrievedMemoryObject {
    pub recall_source: String,
    pub recall_kind: Option<String>,
    pub collection: Option<String>,
    pub retrieval_query: Option<String>,
    pub recall_summary: Option<String>,
}

impl RetrievedMemoryObject {
    pub fn render(&self) -> String {
        let source = self.recall_source.trim();
        if source.is_empty() {
            return String::new();
        }

        let mut parts = vec![format!("source={source}")];
        push_opt(&mut parts, "kind", self.recall_kind.as_deref());
        push_opt(&mut parts, "collection", self.collection.as_deref());
        push_opt(&mut parts, "query", self.retrieval_query.as_deref());
        push_opt(&mut parts, "summary", self.recall_summary.as_deref());

        format!("Memory Recall Object: {}", parts.join(", "))
    }

    pub fn normalized_key(&self) -> String {
        format!(
            "{}::{}::{}",
            self.recall_source.trim(),
            self.recall_kind.as_deref().unwrap_or_default().trim(),
            self.retrieval_query
                .as_deref()
                .or(self.recall_summary.as_deref())
                .unwrap_or_default()
                .trim()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WebSessionObject {
    pub url: String,
    pub page_title: Option<String>,
    pub task_goal: Option<String>,
}

impl WebSessionObject {
    pub fn render(&self) -> String {
        let url = self.url.trim();
        if url.is_empty() {
            return String::new();
        }

        let mut parts = vec![format!("url={url}")];
        push_opt(&mut parts, "page_title", self.page_title.as_deref());
        push_opt(&mut parts, "task_goal", self.task_goal.as_deref());

        format!("Web Session Object: {}", parts.join(", "))
    }

    pub fn normalized_key(&self) -> String {
        format!(
            "{}::{}",
            self.url.trim(),
            self.page_title
                .as_deref()
                .or(self.task_goal.as_deref())
                .unwrap_or_default()
                .trim()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ArtifactSessionObject {
    pub path: String,
    pub collection: Option<String>,
    pub task_goal: Option<String>,
}

impl ArtifactSessionObject {
    pub fn render(&self) -> String {
        let path = self.path.trim();
        if path.is_empty() {
            return String::new();
        }

        let mut parts = vec![format!("path={path}")];
        push_opt(&mut parts, "collection", self.collection.as_deref());
        push_opt(&mut parts, "task_goal", self.task_goal.as_deref());

        format!("Artifact Session Object: {}", parts.join(", "))
    }

    pub fn normalized_key(&self) -> String {
        format!(
            "{}::{}",
            self.path.trim(),
            self.collection
                .as_deref()
                .or(self.task_goal.as_deref())
                .unwrap_or_default()
                .trim()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskSessionObject {
    pub state: String,
    pub title: Option<String>,
    pub goal: Option<String>,
}

impl TaskSessionObject {
    pub fn render(&self) -> String {
        let state = self.state.trim();
        if state.is_empty() {
            return String::new();
        }

        let mut parts = vec![format!("state={state}")];
        push_opt(&mut parts, "title", self.title.as_deref());
        push_opt(&mut parts, "goal", self.goal.as_deref());

        format!("Task Session Object: {}", parts.join(", "))
    }

    pub fn normalized_key(&self) -> String {
        format!(
            "{}::{}::{}",
            self.state.trim(),
            self.title.as_deref().unwrap_or_default().trim(),
            self.goal.as_deref().unwrap_or_default().trim()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolSessionObject {
    pub tool_name: String,
    pub result_summary: Option<String>,
    pub route: Option<String>,
    pub source_ref: Option<String>,
}

impl ToolSessionObject {
    pub fn render(&self) -> String {
        let tool_name = self.tool_name.trim();
        if tool_name.is_empty() {
            return String::new();
        }

        let mut parts = vec![format!("tool={tool_name}")];
        push_opt(&mut parts, "summary", self.result_summary.as_deref());
        push_opt(&mut parts, "route", self.route.as_deref());
        push_opt(&mut parts, "source_ref", self.source_ref.as_deref());

        format!("Tool Session Object: {}", parts.join(", "))
    }

    pub fn normalized_key(&self) -> String {
        format!(
            "{}::{}::{}",
            self.tool_name.trim(),
            self.route.as_deref().unwrap_or_default().trim(),
            self.source_ref.as_deref().unwrap_or_default().trim()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MultimodalSessionObject {
    pub locator: String,
    pub route: Option<String>,
    pub modality: Option<String>,
    pub collection: Option<String>,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub task_goal: Option<String>,
}

impl MultimodalSessionObject {
    pub fn render(&self) -> String {
        let locator = self.locator.trim();
        if locator.is_empty() {
            return String::new();
        }

        let mut parts = vec![format!("locator={locator}")];
        push_opt(&mut parts, "route", self.route.as_deref());
        push_opt(&mut parts, "modality", self.modality.as_deref());
        push_opt(&mut parts, "collection", self.collection.as_deref());
        push_opt(&mut parts, "source_url", self.source_url.as_deref());
        push_opt(&mut parts, "title", self.title.as_deref());
        push_opt(&mut parts, "task_goal", self.task_goal.as_deref());

        format!("Multimodal Session Object: {}", parts.join(", "))
    }

    pub fn normalized_key(&self) -> String {
        format!(
            "{}::{}",
            self.locator.trim(),
            self.route.as_deref().unwrap_or_default().trim()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SessionBackgroundState {
    pub active_topics: Vec<String>,
    pub backend_contexts: Vec<String>,
    #[serde(default)]
    pub backend_context_records: Vec<BackendContextRecord>,
    #[serde(default)]
    pub retrieved_memory_objects: Vec<RetrievedMemoryObject>,
    #[serde(default)]
    pub web_session_objects: Vec<WebSessionObject>,
    #[serde(default)]
    pub artifact_session_objects: Vec<ArtifactSessionObject>,
    #[serde(default)]
    pub task_session_objects: Vec<TaskSessionObject>,
    #[serde(default)]
    pub tool_session_objects: Vec<ToolSessionObject>,
    #[serde(default)]
    pub multimodal_session_objects: Vec<MultimodalSessionObject>,
    pub open_loops: Vec<String>,
    pub recent_emotional_state: Option<String>,
    pub ongoing_goals: Vec<String>,
    pub workspace_focus: Option<String>,
    pub pending_followups: Vec<String>,
    pub summary: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

impl SessionBackgroundState {
    pub fn compression_slots(&self) -> BackgroundCompressionSlots {
        BackgroundCompressionSlots {
            project_facts: metadata_lines(&self.metadata, BACKGROUND_SLOT_PROJECT_FACTS),
            current_task: metadata_optional_text(&self.metadata, BACKGROUND_SLOT_CURRENT_TASK),
            completed_work: metadata_lines(&self.metadata, BACKGROUND_SLOT_COMPLETED_WORK),
            pending_work: metadata_lines(&self.metadata, BACKGROUND_SLOT_PENDING_WORK),
            key_files: metadata_lines(&self.metadata, BACKGROUND_SLOT_KEY_FILES),
            test_results: metadata_lines(&self.metadata, BACKGROUND_SLOT_TEST_RESULTS),
            risks: metadata_lines(&self.metadata, BACKGROUND_SLOT_RISKS),
            verification_needs: metadata_lines(&self.metadata, BACKGROUND_SLOT_VERIFICATION_NEEDS),
        }
    }

    pub fn set_compression_slots(&mut self, mut slots: BackgroundCompressionSlots) {
        slots.apply_budget_caps();
        set_metadata_lines(
            &mut self.metadata,
            BACKGROUND_SLOT_PROJECT_FACTS,
            &slots.project_facts,
        );
        set_metadata_optional_text(
            &mut self.metadata,
            BACKGROUND_SLOT_CURRENT_TASK,
            slots.current_task.as_deref(),
        );
        set_metadata_lines(
            &mut self.metadata,
            BACKGROUND_SLOT_COMPLETED_WORK,
            &slots.completed_work,
        );
        set_metadata_lines(
            &mut self.metadata,
            BACKGROUND_SLOT_PENDING_WORK,
            &slots.pending_work,
        );
        set_metadata_lines(
            &mut self.metadata,
            BACKGROUND_SLOT_KEY_FILES,
            &slots.key_files,
        );
        set_metadata_lines(
            &mut self.metadata,
            BACKGROUND_SLOT_TEST_RESULTS,
            &slots.test_results,
        );
        set_metadata_lines(&mut self.metadata, BACKGROUND_SLOT_RISKS, &slots.risks);
        set_metadata_lines(
            &mut self.metadata,
            BACKGROUND_SLOT_VERIFICATION_NEEDS,
            &slots.verification_needs,
        );
    }

    pub fn compression_slots_are_empty(&self) -> bool {
        self.compression_slots().is_empty()
    }

    pub fn apply_compression_slot_budget_caps(&mut self) {
        let slots = self.compression_slots();
        self.set_compression_slots(slots);
    }

    pub fn canonical_backend_context_records(&self) -> Vec<BackendContextRecord> {
        if !self.backend_context_records.is_empty() {
            return self.backend_context_records.clone();
        }

        self.backend_contexts
            .iter()
            .map(|value| BackendContextRecord::from_legacy_text(value))
            .filter(|record| !record.value.trim().is_empty())
            .collect()
    }

    pub fn sync_backend_context_storage(&mut self) {
        let records = self.canonical_backend_context_records();
        self.backend_context_records = records.clone();
        self.backend_contexts = records
            .iter()
            .map(BackendContextRecord::render)
            .filter(|value| !value.is_empty())
            .collect();
    }

    pub fn is_empty(&self) -> bool {
        self.active_topics.is_empty()
            && self.backend_contexts.is_empty()
            && self.backend_context_records.is_empty()
            && self.retrieved_memory_objects.is_empty()
            && self.web_session_objects.is_empty()
            && self.artifact_session_objects.is_empty()
            && self.task_session_objects.is_empty()
            && self.tool_session_objects.is_empty()
            && self.multimodal_session_objects.is_empty()
            && self.open_loops.is_empty()
            && self.recent_emotional_state.is_none()
            && self.ongoing_goals.is_empty()
            && self.workspace_focus.is_none()
            && self.pending_followups.is_empty()
            && self.summary.is_none()
            && self.metadata.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RecentWindowSummary {
    pub summary: String,
    pub pruned_message_count: usize,
    pub covered_message_count: usize,
    pub metadata: std::collections::HashMap<String, String>,
}

impl RecentWindowSummary {
    pub fn is_empty(&self) -> bool {
        self.summary.trim().is_empty()
            && self.pruned_message_count == 0
            && self.covered_message_count == 0
            && self.metadata.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundCompressionDecision {
    Skip,
    RefreshSessionLayer,
    PromoteRelationshipFact,
    RewriteWholeEnvelope,
    RejectCandidate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackgroundEnvelope {
    pub persona_layer: Option<PersonaBackgroundLayer>,
    pub relationship_layer: Option<RelationshipBackgroundLayer>,
    pub session_layer: Option<SessionBackgroundState>,
    pub recent_window_summary: Option<RecentWindowSummary>,
    pub revision: BackgroundRevision,
    #[serde(default)]
    pub source_refs: Vec<BackgroundEvidenceRef>,
    #[serde(default)]
    pub quality_signal: BackgroundQualitySignal,
    pub compression_reason: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

impl Default for BackgroundEnvelope {
    fn default() -> Self {
        Self {
            persona_layer: None,
            relationship_layer: None,
            session_layer: None,
            recent_window_summary: None,
            revision: BackgroundRevision::default(),
            source_refs: Vec::new(),
            quality_signal: BackgroundQualitySignal::default(),
            compression_reason: None,
            updated_at: chrono::Utc::now(),
            metadata: std::collections::HashMap::new(),
        }
    }
}

impl BackgroundEnvelope {
    pub fn is_empty(&self) -> bool {
        self.persona_layer
            .as_ref()
            .is_none_or(PersonaBackgroundLayer::is_empty)
            && self
                .relationship_layer
                .as_ref()
                .is_none_or(RelationshipBackgroundLayer::is_empty)
            && self
                .session_layer
                .as_ref()
                .is_none_or(SessionBackgroundState::is_empty)
            && self
                .recent_window_summary
                .as_ref()
                .is_none_or(RecentWindowSummary::is_empty)
            && self.source_refs.is_empty()
            && self.compression_reason.is_none()
            && self.metadata.is_empty()
    }

    pub fn apply_budget_caps(&mut self) {
        if let Some(persona_layer) = self.persona_layer.as_mut() {
            persona_layer.identity_summary =
                cap_optional_text(persona_layer.identity_summary.take(), 240);
            persona_layer.speaking_style =
                cap_optional_text(persona_layer.speaking_style.take(), 160);
            persona_layer.relationship_frame =
                cap_optional_text(persona_layer.relationship_frame.take(), 200);
            cap_vec_len(&mut persona_layer.safety_notes, 4);
            cap_vec_text(&mut persona_layer.safety_notes, 120);
        }

        if let Some(relationship_layer) = self.relationship_layer.as_mut() {
            relationship_layer.user_profile_summary =
                cap_optional_text(relationship_layer.user_profile_summary.take(), 240);
            relationship_layer.relationship_summary =
                cap_optional_text(relationship_layer.relationship_summary.take(), 220);
            cap_vec_len(&mut relationship_layer.user_preferences, 6);
            cap_vec_text(&mut relationship_layer.user_preferences, 120);
            cap_vec_len(&mut relationship_layer.long_term_topics, 4);
            cap_vec_text(&mut relationship_layer.long_term_topics, 100);
            cap_vec_len(&mut relationship_layer.emotional_markers, 4);
            cap_vec_text(&mut relationship_layer.emotional_markers, 80);
        }

        if let Some(session_layer) = self.session_layer.as_mut() {
            session_layer.sync_backend_context_storage();
            session_layer.recent_emotional_state =
                cap_optional_text(session_layer.recent_emotional_state.take(), 120);
            session_layer.workspace_focus =
                cap_optional_text(session_layer.workspace_focus.take(), 160);
            session_layer.summary = cap_optional_text(session_layer.summary.take(), 320);
            cap_vec_len(&mut session_layer.active_topics, 5);
            cap_vec_text(&mut session_layer.active_topics, 100);
            cap_vec_len(&mut session_layer.backend_contexts, 8);
            cap_vec_text(&mut session_layer.backend_contexts, 140);
            cap_vec_len(&mut session_layer.backend_context_records, 8);
            for record in &mut session_layer.backend_context_records {
                record.value = cap_text(std::mem::take(&mut record.value), 140);
                record.source = cap_optional_text(record.source.take(), 120);
            }
            cap_vec_len(&mut session_layer.retrieved_memory_objects, 6);
            for object in &mut session_layer.retrieved_memory_objects {
                object.recall_source = cap_text(std::mem::take(&mut object.recall_source), 80);
                object.recall_kind = cap_optional_text(object.recall_kind.take(), 60);
                object.collection = cap_optional_text(object.collection.take(), 80);
                object.retrieval_query = cap_optional_text(object.retrieval_query.take(), 140);
                object.recall_summary = cap_optional_text(object.recall_summary.take(), 160);
            }
            cap_vec_len(&mut session_layer.web_session_objects, 6);
            for object in &mut session_layer.web_session_objects {
                object.url = cap_text(std::mem::take(&mut object.url), 140);
                object.page_title = cap_optional_text(object.page_title.take(), 120);
                object.task_goal = cap_optional_text(object.task_goal.take(), 140);
            }
            cap_vec_len(&mut session_layer.artifact_session_objects, 6);
            for object in &mut session_layer.artifact_session_objects {
                object.path = cap_text(std::mem::take(&mut object.path), 140);
                object.collection = cap_optional_text(object.collection.take(), 80);
                object.task_goal = cap_optional_text(object.task_goal.take(), 140);
            }
            cap_vec_len(&mut session_layer.task_session_objects, 6);
            for object in &mut session_layer.task_session_objects {
                object.state = cap_text(std::mem::take(&mut object.state), 100);
                object.title = cap_optional_text(object.title.take(), 120);
                object.goal = cap_optional_text(object.goal.take(), 140);
            }
            cap_vec_len(&mut session_layer.tool_session_objects, 6);
            for object in &mut session_layer.tool_session_objects {
                object.tool_name = cap_text(std::mem::take(&mut object.tool_name), 64);
                object.result_summary = cap_optional_text(object.result_summary.take(), 160);
                object.route = cap_optional_text(object.route.take(), 80);
                object.source_ref = cap_optional_text(object.source_ref.take(), 140);
            }
            cap_vec_len(&mut session_layer.multimodal_session_objects, 6);
            for object in &mut session_layer.multimodal_session_objects {
                object.locator = cap_text(std::mem::take(&mut object.locator), 140);
                object.route = cap_optional_text(object.route.take(), 80);
                object.modality = cap_optional_text(object.modality.take(), 32);
                object.collection = cap_optional_text(object.collection.take(), 80);
                object.source_url = cap_optional_text(object.source_url.take(), 140);
                object.title = cap_optional_text(object.title.take(), 120);
                object.task_goal = cap_optional_text(object.task_goal.take(), 140);
            }
            cap_vec_len(&mut session_layer.open_loops, 5);
            cap_vec_text(&mut session_layer.open_loops, 140);
            cap_vec_len(&mut session_layer.ongoing_goals, 4);
            cap_vec_text(&mut session_layer.ongoing_goals, 120);
            cap_vec_len(&mut session_layer.pending_followups, 5);
            cap_vec_text(&mut session_layer.pending_followups, 140);
            session_layer.apply_compression_slot_budget_caps();
        }

        if let Some(recent_window_summary) = self.recent_window_summary.as_mut() {
            recent_window_summary.summary =
                cap_text(std::mem::take(&mut recent_window_summary.summary), 360);
        }

        if self.source_refs.len() > 8 {
            let drain_count = self.source_refs.len() - 8;
            self.source_refs.drain(0..drain_count);
        }
    }
}

fn push_opt(parts: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("{key}={value}"));
    }
}

fn metadata_lines(metadata: &std::collections::HashMap<String, String>, key: &str) -> Vec<String> {
    metadata
        .get(key)
        .map(|value| {
            value
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn metadata_optional_text(
    metadata: &std::collections::HashMap<String, String>,
    key: &str,
) -> Option<String> {
    metadata
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn set_metadata_lines(
    metadata: &mut std::collections::HashMap<String, String>,
    key: &str,
    values: &[String],
) {
    let value = values
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if value.is_empty() {
        metadata.remove(key);
    } else {
        metadata.insert(key.to_string(), value);
    }
}

fn set_metadata_optional_text(
    metadata: &mut std::collections::HashMap<String, String>,
    key: &str,
    value: Option<&str>,
) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        metadata.remove(key);
        return;
    };
    metadata.insert(key.to_string(), value.to_string());
}

fn cap_optional_text(value: Option<String>, max_chars: usize) -> Option<String> {
    value
        .map(|text| cap_text(text, max_chars))
        .filter(|text| !text.trim().is_empty())
}

fn cap_text(text: String, max_chars: usize) -> String {
    let trimmed = text.trim();
    preview_text(trimmed, max_chars)
}

fn cap_vec_len<T>(items: &mut Vec<T>, max_items: usize) {
    if items.len() > max_items {
        let drain_count = items.len() - max_items;
        items.drain(0..drain_count);
    }
}

fn cap_vec_text(items: &mut [String], max_chars: usize) {
    for item in items.iter_mut() {
        *item = cap_text(std::mem::take(item), max_chars);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_context_record_roundtrips_legacy_label() {
        let record = BackendContextRecord::from_legacy_text("Web context: https://example.com");
        assert_eq!(record.kind, Some(BackendContextKind::Web));
        assert_eq!(record.render(), "Web context: https://example.com");
    }

    #[test]
    fn background_budget_caps_keep_recent_items() {
        let mut envelope = BackgroundEnvelope {
            session_layer: Some(SessionBackgroundState {
                active_topics: vec![
                    "old".to_string(),
                    "middle".to_string(),
                    "new".repeat(80),
                    "newer".to_string(),
                    "latest".to_string(),
                    "kept".to_string(),
                ],
                ..SessionBackgroundState::default()
            }),
            ..BackgroundEnvelope::default()
        };

        envelope.apply_budget_caps();

        let topics = &envelope.session_layer.as_ref().unwrap().active_topics;
        assert_eq!(topics.len(), 5);
        assert!(!topics.iter().any(|topic| topic == "old"));
        assert!(topics.iter().all(|topic| topic.chars().count() <= 140));
    }
}
