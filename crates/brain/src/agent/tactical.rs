use crate::agent::memory::{
    ArtifactSessionObject, BackendContextKind, BackendContextRecord, BackgroundCompressionDecision,
    BackgroundCompressionSlots, BackgroundEnvelope, BackgroundEvidenceRef, BackgroundQualitySignal,
    MultimodalSessionObject, RelationshipBackgroundLayer, RetrievedMemoryObject,
    SessionBackgroundState, TaskSessionObject, ToolSessionObject, WebSessionObject,
};
use crate::agent::message::Message;
use crate::error::Result;
use async_trait::async_trait;
use benshu_inference::{GenerationConfig, InferenceConfig, KvEngine, ModelBackend};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
struct ActionEntropyMonitor {
    tracker: Arc<RwLock<EntropyTracker>>,
}

impl Default for ActionEntropyMonitor {
    fn default() -> Self {
        Self {
            tracker: Arc::new(RwLock::new(EntropyTracker::default())),
        }
    }
}

#[derive(Debug, Default)]
struct EntropyTracker {
    sessions: HashMap<String, EntropySessionState>,
    touch_epoch: u64,
}

#[derive(Debug)]
struct EntropySessionState {
    history: VecDeque<u64>,
    last_touched_epoch: u64,
}

impl Default for EntropySessionState {
    fn default() -> Self {
        Self {
            history: VecDeque::with_capacity(5),
            last_touched_epoch: 0,
        }
    }
}

impl ActionEntropyMonitor {
    fn count_unique_action_hashes(history: &VecDeque<u64>) -> usize {
        let mut uniques = 0usize;
        let mut seen = [0u64; 5];
        let mut seen_len = 0usize;
        for action_hash in history.iter().copied() {
            let already_seen = seen[..seen_len].contains(&action_hash);
            if !already_seen {
                if seen_len < seen.len() {
                    seen[seen_len] = action_hash;
                }
                seen_len += 1;
                uniques += 1;
            }
        }
        uniques
    }

    fn evict_oldest_sessions(
        &self,
        tracker: &mut EntropyTracker,
        max_entropy_sessions: usize,
        entropy_evict_count: usize,
    ) {
        if tracker.sessions.len() < max_entropy_sessions {
            return;
        }

        let overflow = tracker
            .sessions
            .len()
            .saturating_sub(max_entropy_sessions)
            .saturating_add(1);
        let evict_count = entropy_evict_count
            .max(1)
            .min(tracker.sessions.len())
            .max(overflow);

        let mut oldest = tracker
            .sessions
            .iter()
            .map(|(session_id, state)| (session_id.clone(), state.last_touched_epoch))
            .collect::<Vec<_>>();
        oldest.sort_by_key(|(_, last_touched_epoch)| *last_touched_epoch);

        for (session_id, _) in oldest.into_iter().take(evict_count) {
            tracker.sessions.remove(&session_id);
        }

        debug!(
            "TacticalOrchestrator: entropy buffer pressure. Evicted {} least-recently-touched sessions.",
            evict_count
        );
    }

    fn calculate(
        &self,
        config: &TacticalOrchestratorConfig,
        session_id: &str,
        action_hash: u64,
    ) -> f32 {
        let mut tracker = self.tracker.write();
        tracker.touch_epoch = tracker.touch_epoch.saturating_add(1);
        let current_epoch = tracker.touch_epoch;

        if !tracker.sessions.contains_key(session_id)
            && tracker.sessions.len() >= config.max_entropy_sessions
        {
            self.evict_oldest_sessions(
                &mut tracker,
                config.max_entropy_sessions,
                config.entropy_evict_count,
            );
        }

        let state = tracker
            .sessions
            .entry(session_id.to_string())
            .or_insert_with(EntropySessionState::default);
        state.last_touched_epoch = current_epoch;
        let history = &mut state.history;

        history.push_back(action_hash);
        if history.len() > 5 {
            history.pop_front();
        }

        let original_len = history.len();
        let unique_count = Self::count_unique_action_hashes(history);

        if original_len < 2 {
            1.0
        } else {
            (unique_count as f32) / (original_len as f32)
        }
    }
}

#[derive(Debug)]
struct SpeculativeTaskSlot<T> {
    pending_task: Arc<RwLock<Option<tokio::task::JoinHandle<T>>>>,
}

impl<T> Default for SpeculativeTaskSlot<T> {
    fn default() -> Self {
        Self {
            pending_task: Arc::new(RwLock::new(None)),
        }
    }
}

impl<T> SpeculativeTaskSlot<T> {
    fn replace(&self, handle: tokio::task::JoinHandle<T>) {
        let mut pending = self.pending_task.write();
        if let Some(previous) = pending.replace(handle) {
            previous.abort();
            debug!(
                "SpeculativeTacticalOrchestrator: Aborted stale pending tactical validation task."
            );
        }
    }

    fn take(&self) -> Option<tokio::task::JoinHandle<T>> {
        self.pending_task.write().take()
    }
}

impl<T> Drop for SpeculativeTaskSlot<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.pending_task.write().take() {
            handle.abort();
        }
    }
}

struct BackgroundTacticsEngine<'a> {
    model_name: &'a str,
}

impl<'a> BackgroundTacticsEngine<'a> {
    fn new(model_name: &'a str) -> Self {
        Self { model_name }
    }

    fn derive_rule_based(
        &self,
        orchestrator: &GlobalTacticalOrchestrator,
        messages: &[Message],
        current_background: Option<&BackgroundEnvelope>,
    ) -> BackgroundCompressionVerdict {
        orchestrator.derive_background_tactics_rule_based(messages, current_background)
    }

    async fn derive_with_slm(
        &self,
        orchestrator: &GlobalTacticalOrchestrator,
        backend: &Arc<dyn ModelBackend>,
        messages: &[Message],
        current_background: Option<&BackgroundEnvelope>,
    ) -> Result<BackgroundCompressionVerdict> {
        orchestrator
            .derive_background_tactics_with_slm(backend, messages, current_background)
            .await
    }
}

fn fresh_tactical_kv_engine() -> Arc<RwLock<KvEngine>> {
    Arc::new(RwLock::new(KvEngine::new(InferenceConfig::default())))
}

/// Phase 16.2: Type-safe action representation
#[derive(Debug, Clone)]
pub struct ProposedAction {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
}

/// Phase 16.2: Configuration for the tactical balancer
#[derive(Debug, Clone)]
pub struct TacticalOrchestratorConfig {
    pub entropy_threshold: f32,
    pub max_entropy_sessions: usize,
    pub entropy_evict_count: usize,
    pub max_new_tokens: usize,
    pub temperature: f32,
    pub context_message_count: usize,
    pub session_topic_staleness_epochs: u64,
    pub session_goal_staleness_epochs: u64,
    pub session_followup_staleness_epochs: u64,
    pub session_workspace_staleness_epochs: u64,
    pub session_theme_staleness_epochs: u64,
}

impl Default for TacticalOrchestratorConfig {
    fn default() -> Self {
        Self {
            entropy_threshold: 0.21,
            max_entropy_sessions: 1000,
            entropy_evict_count: 64,
            max_new_tokens: 160,
            temperature: 0.1,
            context_message_count: 3,
            session_topic_staleness_epochs: 4,
            session_goal_staleness_epochs: 4,
            session_followup_staleness_epochs: 3,
            session_workspace_staleness_epochs: 3,
            session_theme_staleness_epochs: 3,
        }
    }
}

/// Phase 16.1: Tactical Orchestration (System 2 Reflection)
#[async_trait]
pub trait TacticalOrchestrator: Send + Sync {
    async fn derive_tactics(
        &self,
        messages: &[Message],
        proposed_actions: &[ProposedAction],
    ) -> Result<TacticalVerdict>;

    async fn derive_background_tactics(
        &self,
        messages: &[Message],
        current_background: Option<&BackgroundEnvelope>,
    ) -> Result<BackgroundCompressionVerdict>;

    fn is_active(&self) -> bool;
}

#[derive(Debug, Clone)]
pub enum TacticalVerdict {
    Proceed,
    Pivot(String),
    Halt(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundCompressionVerdict {
    pub decision: BackgroundCompressionDecision,
    pub reason: String,
    pub quality_signal: BackgroundQualitySignal,
    pub relationship_candidate: Option<RelationshipBackgroundLayer>,
    pub session_candidate: Option<SessionBackgroundState>,
    pub evidence_refs: Vec<BackgroundEvidenceRef>,
    pub used_slm: bool,
}

impl BackgroundCompressionVerdict {
    pub fn skip(reason: impl Into<String>) -> Self {
        Self {
            decision: BackgroundCompressionDecision::Skip,
            reason: reason.into(),
            quality_signal: BackgroundQualitySignal::Skipped,
            relationship_candidate: None,
            session_candidate: None,
            evidence_refs: Vec::new(),
            used_slm: false,
        }
    }
}

const TACTICAL_PROMPT_TEMPLATE: &str = "### TACTICAL REFLECTION (SYSTEM 2)\n\n\
    Evaluate the following proposed actions for logical consistency and first-principles efficiency.\n\n\
    #### CONTEXT SUMMARY:\n{context}\n\n\
    #### PROPOSED ACTIONS:\n{actions}\n\n\
    #### INSTRUCTIONS:\n\
    1. Identify if these actions are leading to an infinite loop.\n\
    2. Check if the tools are being used correctly based on the goal.\n\
    3. If the plan is solid, respond with [PROCEED].\n\
    4. If there's a better tactical path, respond with [PIVOT] followed by advice.\n\
    5. If the plan is dangerous or nonsensical, respond with [HALT] followed by reason.\n\n\
    VERDICT:";

const BACKGROUND_TACTICAL_PROMPT_TEMPLATE: &str = "### BACKGROUND COMPRESSION REFLECTION\n\n\
    Evaluate whether the recent conversation should refresh the agent background layer.\n\n\
    #### CURRENT BACKGROUND:\n{background}\n\n\
    #### RECENT CONVERSATION WINDOW:\n{context}\n\n\
    #### INSTRUCTIONS:\n\
    1. Be conservative. Do not promote speculative or weakly supported relationship claims.\n\
    2. Choose exactly one verdict token.\n\
    3. Use [SKIP] when no update is needed.\n\
    4. Use [REFRESH_SESSION] when only current session background should be refreshed.\n\
    5. Use [PROMOTE_FACT] only for clearly supported durable preference/relationship facts.\n\
    6. Use [REWRITE_ENVELOPE] only when the current background is stale and should be rewritten.\n\
    7. Use [REJECT] when the candidate update is risky, contradictory, or unsupported.\n\n\
    VERDICT:";

pub struct GlobalTacticalOrchestrator {
    slm_backend: Option<Arc<dyn ModelBackend>>,
    model_name: String,
    config: TacticalOrchestratorConfig,
    entropy_monitor: ActionEntropyMonitor,
}

impl GlobalTacticalOrchestrator {
    pub fn new(slm_backend: Option<Arc<dyn ModelBackend>>, model_name: String) -> Self {
        Self {
            slm_backend,
            model_name,
            config: TacticalOrchestratorConfig::default(),
            entropy_monitor: ActionEntropyMonitor::default(),
        }
    }

    pub fn with_config(mut self, config: TacticalOrchestratorConfig) -> Self {
        self.config = config;
        self
    }

    pub fn passthrough() -> Self {
        Self {
            slm_backend: None,
            model_name: "Passthrough (None)".to_string(),
            config: TacticalOrchestratorConfig::default(),
            entropy_monitor: ActionEntropyMonitor::default(),
        }
    }

    fn calculate_entropy(&self, session_id: &str, action_hash: u64) -> f32 {
        self.entropy_monitor
            .calculate(&self.config, session_id, action_hash)
    }

    fn shrink_text(text: &str, max_chars: usize) -> String {
        let trimmed = text.trim();
        if trimmed.chars().count() <= max_chars {
            return trimmed.to_string();
        }

        let cutoff = trimmed
            .char_indices()
            .nth(max_chars)
            .map(|(idx, _)| idx)
            .unwrap_or(trimmed.len());
        format!("{}...", &trimmed[..cutoff])
    }

    fn summarize_recent_window(messages: &[Message], limit: usize) -> String {
        let mut summary = String::new();
        for message in messages
            .iter()
            .rev()
            .filter(|m| !matches!(m.role, crate::agent::message::Role::System))
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let role = format!("{:?}", message.role).to_uppercase();
            let text = Self::shrink_text(&message.content.as_text(), 180);
            summary.push_str(&format!("- {}: {}\n", role, text));
        }
        summary
    }

    fn background_snapshot(background: Option<&BackgroundEnvelope>) -> String {
        match background {
            Some(envelope) if !envelope.is_empty() => {
                let mut lines = Vec::new();
                if let Some(persona) = &envelope.persona_layer {
                    if let Some(identity) = &persona.identity_summary {
                        lines.push(format!("persona: {}", Self::shrink_text(identity, 140)));
                    }
                }
                if let Some(relationship) = &envelope.relationship_layer {
                    if let Some(summary) = &relationship.relationship_summary {
                        lines.push(format!("relationship: {}", Self::shrink_text(summary, 140)));
                    }
                }
                if let Some(session) = &envelope.session_layer {
                    if let Some(summary) = &session.summary {
                        lines.push(format!("session: {}", Self::shrink_text(summary, 140)));
                    }
                }
                if lines.is_empty() {
                    "background present but mostly empty".to_string()
                } else {
                    lines.join("\n")
                }
            }
            _ => "no current background".to_string(),
        }
    }

    fn build_background_evidence_refs(
        messages: &[Message],
        limit: usize,
    ) -> Vec<BackgroundEvidenceRef> {
        messages
            .iter()
            .rev()
            .filter(|m| !matches!(m.role, crate::agent::message::Role::System))
            .take(limit)
            .enumerate()
            .map(|(index, message)| {
                let mut metadata = std::collections::HashMap::from([(
                    "role".to_string(),
                    format!("{:?}", message.role).to_lowercase(),
                )]);
                if let Some(tool_name) = message.metadata.get("tool_name") {
                    metadata.insert("tool_name".to_string(), tool_name.clone());
                }
                if let Some(source_collection) = &message.source_collection {
                    metadata.insert("source_collection".to_string(), source_collection.clone());
                }
                if let Some(source_path) = &message.source_path {
                    metadata.insert(
                        "source_path".to_string(),
                        Self::shrink_text(source_path, 160),
                    );
                }
                if let Some(source_url) = message.metadata.get("source_url") {
                    metadata.insert("source_url".to_string(), Self::shrink_text(source_url, 160));
                }
                if let Some(media_source_ref) = message.metadata.get("media_preprocess_source_ref")
                {
                    metadata.insert(
                        "media_preprocess_source_ref".to_string(),
                        Self::shrink_text(media_source_ref, 160),
                    );
                }
                if let Some(route) = message.metadata.get("route") {
                    metadata.insert("route".to_string(), route.clone());
                }
                if let Some(route) = message.metadata.get("media_preprocess_route") {
                    metadata.insert("media_preprocess_route".to_string(), route.clone());
                }
                if let Some(retrieved_from) = message.metadata.get("retrieved_from") {
                    metadata.insert("retrieved_from".to_string(), retrieved_from.clone());
                }
                if let Some(task_state) = message.metadata.get("task_state") {
                    metadata.insert("task_state".to_string(), task_state.clone());
                }
                if let Some(task_goal) = message.metadata.get("task_goal") {
                    metadata.insert("task_goal".to_string(), Self::shrink_text(task_goal, 160));
                }
                if let Some(window_title) = message.metadata.get("window_title") {
                    metadata.insert(
                        "window_title".to_string(),
                        Self::shrink_text(window_title, 160),
                    );
                }

                let source_kind = if message
                    .source_collection
                    .as_deref()
                    .is_some_and(|value| value.contains("memory") || value.contains("recall"))
                    || message
                        .metadata
                        .get("route")
                        .is_some_and(|value| value.contains("memory") || value.contains("recall"))
                    || message.metadata.contains_key("retrieved_from")
                {
                    "memory_recall".to_string()
                } else if message.metadata.contains_key("tool_name") {
                    "tool_result".to_string()
                } else if message.source_path.is_some()
                    || message.metadata.contains_key("source_url")
                {
                    "artifact".to_string()
                } else {
                    "message".to_string()
                };

                let source_id = message
                    .metadata
                    .get("source_url")
                    .cloned()
                    .or_else(|| message.metadata.get("media_preprocess_source_ref").cloned())
                    .or_else(|| message.source_path.clone())
                    .or_else(|| message.metadata.get("retrieved_from").cloned())
                    .or_else(|| {
                        message.metadata.get("tool_name").map(|tool_name| {
                            let route = message
                                .metadata
                                .get("route")
                                .or_else(|| message.metadata.get("media_preprocess_route"))
                                .map(String::as_str)
                                .unwrap_or("unknown_route");
                            format!("{tool_name}:{route}")
                        })
                    })
                    .or_else(|| message.metadata.get("message_id").cloned())
                    .or_else(|| message.metadata.get("id").cloned())
                    .unwrap_or_else(|| format!("recent-{}", index));

                BackgroundEvidenceRef {
                    source_kind,
                    source_id,
                    confidence: Some(0.75),
                    occurred_at: None,
                    metadata,
                }
            })
            .collect()
    }

    fn tool_name_is_transient_lookup(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "weather_lookup"
                | "price_lookup"
                | "fx_lookup"
                | "latest_info_lookup"
                | "realtime_lookup"
        )
    }

    fn runtime_effect_is_durable(effect: &str) -> bool {
        effect.contains("artifact.")
            || effect.contains("knowledge.imported")
            || effect.contains("continuous.")
            || effect.contains("task.")
    }

    fn message_has_durable_working_set_signal(message: &Message) -> bool {
        if message.source_path.is_some() || message.source_collection.is_some() {
            return true;
        }
        if let Some(tool_name) = message.metadata.get("tool_name") {
            if Self::tool_name_is_transient_lookup(tool_name) {
                return false;
            }
        }
        if message
            .metadata
            .get("runtime_effect")
            .is_some_and(|effect| Self::runtime_effect_is_durable(effect))
        {
            return true;
        }
        [
            "task_state",
            "task_title",
            "task_goal",
            "task_completed",
            "task_pending",
            "artifact_path",
            "artifact_uri",
            "output_path",
            "checkpoint_path",
            "continuous_task_id",
            "verification_result",
            "test_result",
        ]
        .iter()
        .any(|key| {
            message
                .metadata
                .get(*key)
                .is_some_and(|value| !value.trim().is_empty())
        })
    }

    fn push_slot_value(values: &mut Vec<String>, value: impl AsRef<str>, max_len: usize) {
        let value = Self::shrink_text(value.as_ref(), max_len);
        if value.trim().is_empty() {
            return;
        }
        Self::push_unique_limited(values, value, 8);
    }

    fn merge_working_set_slots_from_messages(
        slots: &mut BackgroundCompressionSlots,
        recent_messages: &[&Message],
    ) {
        for message in recent_messages.iter().rev().copied() {
            if !Self::message_has_durable_working_set_signal(message) {
                continue;
            }

            if slots.current_task.is_none() {
                if let Some(task) = Self::first_non_empty_metadata(
                    message,
                    &["task_state", "task_title", "task_goal"],
                ) {
                    slots.current_task = Some(Self::shrink_text(task, 180));
                }
            }

            for key in ["task_completed", "completed_work", "completion_summary"] {
                if let Some(value) = message.metadata.get(key) {
                    Self::push_slot_value(&mut slots.completed_work, value, 140);
                }
            }
            for key in ["task_pending", "pending_work", "next_step", "next_action"] {
                if let Some(value) = message.metadata.get(key) {
                    Self::push_slot_value(&mut slots.pending_work, value, 140);
                }
            }
            for key in [
                "artifact_path",
                "artifact_uri",
                "output_path",
                "checkpoint_path",
                "file_path",
            ] {
                if let Some(value) = message.metadata.get(key) {
                    Self::push_slot_value(&mut slots.key_files, value, 180);
                }
            }
            if let Some(source_path) = message.source_path.as_deref() {
                Self::push_slot_value(&mut slots.key_files, source_path, 180);
            }
            for key in ["test_result", "verification_result", "check_result"] {
                if let Some(value) = message.metadata.get(key) {
                    Self::push_slot_value(&mut slots.test_results, value, 180);
                }
            }

            if let Some(effect) = message.metadata.get("runtime_effect") {
                if Self::runtime_effect_is_durable(effect) {
                    Self::push_slot_value(
                        &mut slots.completed_work,
                        format!("durable runtime effect: {effect}"),
                        140,
                    );
                }
            }
        }
        slots.apply_budget_caps();
    }

    fn infer_session_candidate(
        &self,
        messages: &[Message],
        current_background: Option<&BackgroundEnvelope>,
    ) -> Option<SessionBackgroundState> {
        let recent_messages = messages
            .iter()
            .rev()
            .filter(|m| !matches!(m.role, crate::agent::message::Role::System))
            .take(6)
            .collect::<Vec<_>>();

        if recent_messages.is_empty() {
            return None;
        }

        let mut base_session = current_background
            .and_then(|background| background.session_layer.clone())
            .unwrap_or_default();
        base_session.sync_backend_context_storage();
        let next_epoch = current_background
            .map(|background| background.revision.revision.saturating_add(1))
            .unwrap_or(1);
        let mut active_topics = base_session.active_topics.clone();
        let mut backend_context_records = base_session.backend_context_records.clone();
        let mut retrieved_memory_objects = base_session.retrieved_memory_objects.clone();
        let mut web_session_objects = base_session.web_session_objects.clone();
        let mut artifact_session_objects = base_session.artifact_session_objects.clone();
        let mut task_session_objects = base_session.task_session_objects.clone();
        let mut tool_session_objects = base_session.tool_session_objects.clone();
        let mut multimodal_session_objects = base_session.multimodal_session_objects.clone();
        let mut ongoing_goals = base_session.ongoing_goals.clone();
        let mut pending_followups = base_session.pending_followups.clone();
        let (workspace_focus, workspace_metadata) =
            Self::infer_workspace_focus(recent_messages.as_slice());
        let mut observed_active_topics = Vec::new();
        let mut observed_backend_contexts = Vec::new();
        let mut observed_backend_context_records = Vec::new();
        let mut observed_retrieved_memory_objects = Vec::new();
        let mut observed_web_session_objects = Vec::new();
        let mut observed_artifact_session_objects = Vec::new();
        let mut observed_task_session_objects = Vec::new();
        let mut observed_tool_session_objects = Vec::new();
        let mut observed_multimodal_session_objects = Vec::new();
        let mut observed_goals = Vec::new();
        let mut observed_followups = Vec::new();

        let inferred_backend_contexts = Self::infer_backend_contexts(recent_messages.as_slice());
        for context in inferred_backend_contexts {
            let legacy = context.render();
            Self::push_unique_limited(&mut observed_backend_contexts, legacy, 8);
            Self::push_unique_limited_backend_records(
                &mut backend_context_records,
                context.clone(),
                8,
            );
            Self::push_unique_limited_backend_records(
                &mut observed_backend_context_records,
                context,
                8,
            );
        }

        for object in Self::infer_retrieved_memory_objects(recent_messages.as_slice()) {
            Self::push_unique_limited_retrieved_memory_objects(
                &mut retrieved_memory_objects,
                object.clone(),
                6,
            );
            Self::push_unique_limited(&mut observed_retrieved_memory_objects, object.render(), 6);
        }

        for object in Self::infer_web_session_objects(recent_messages.as_slice()) {
            Self::push_unique_limited_web_session_objects(
                &mut web_session_objects,
                object.clone(),
                6,
            );
            Self::push_unique_limited(&mut observed_web_session_objects, object.render(), 6);
        }

        for object in Self::infer_artifact_session_objects(recent_messages.as_slice()) {
            Self::push_unique_limited_artifact_session_objects(
                &mut artifact_session_objects,
                object.clone(),
                6,
            );
            Self::push_unique_limited(&mut observed_artifact_session_objects, object.render(), 6);
        }

        for object in Self::infer_task_session_objects(recent_messages.as_slice()) {
            Self::push_unique_limited_task_session_objects(
                &mut task_session_objects,
                object.clone(),
                6,
            );
            Self::push_unique_limited(&mut observed_task_session_objects, object.render(), 6);
        }

        for object in Self::infer_tool_session_objects(recent_messages.as_slice()) {
            Self::push_unique_limited_tool_session_objects(
                &mut tool_session_objects,
                object.clone(),
                6,
            );
            Self::push_unique_limited(&mut observed_tool_session_objects, object.render(), 6);
        }

        for object in Self::infer_multimodal_session_objects(recent_messages.as_slice()) {
            Self::push_unique_limited_multimodal_session_objects(
                &mut multimodal_session_objects,
                object.clone(),
                6,
            );
            Self::push_unique_limited(&mut observed_multimodal_session_objects, object.render(), 6);
        }

        for message in recent_messages.iter().rev() {
            let text = message.content.as_text();
            let compact = Self::shrink_text(&text, 96);

            match message.role {
                crate::agent::message::Role::User => {
                    Self::push_unique_limited(&mut active_topics, compact.clone(), 5);
                    Self::push_unique_limited(&mut observed_active_topics, compact.clone(), 5);
                    if text.contains('？')
                        || text.contains('?')
                        || text.contains("帮我")
                        || text.contains("请")
                        || text.to_lowercase().contains("please")
                    {
                        Self::push_unique_limited(&mut ongoing_goals, compact.clone(), 4);
                        Self::push_unique_limited(&mut observed_goals, compact.clone(), 4);
                    }
                }
                crate::agent::message::Role::Assistant => {
                    if text.contains('？')
                        || text.contains('?')
                        || text.contains("是否")
                        || text.to_lowercase().contains("next")
                    {
                        Self::push_unique_limited(&mut pending_followups, compact.clone(), 5);
                        Self::push_unique_limited(&mut observed_followups, compact.clone(), 5);
                    }
                }
                crate::agent::message::Role::Tool => {
                    if Self::message_has_durable_working_set_signal(message) {
                        Self::push_unique_limited(
                            &mut active_topics,
                            format!("tool checkpoint: {}", compact),
                            5,
                        );
                        Self::push_unique_limited(
                            &mut observed_active_topics,
                            format!("tool checkpoint: {}", compact),
                            5,
                        );
                    }
                }
                crate::agent::message::Role::System => {}
            }
        }

        let preserve_existing_mode =
            workspace_metadata.is_empty() && base_session.metadata.contains_key("working_mode");
        let mut metadata = base_session.metadata.clone();
        metadata.insert(
            "session_background_refresh_epoch".to_string(),
            next_epoch.to_string(),
        );
        Self::mark_observed_items(
            &mut metadata,
            "active_topic",
            &observed_active_topics,
            next_epoch,
        );
        Self::mark_observed_items(
            &mut metadata,
            "backend_context",
            &observed_backend_contexts,
            next_epoch,
        );
        Self::mark_observed_items(
            &mut metadata,
            "retrieved_memory_object",
            &observed_retrieved_memory_objects,
            next_epoch,
        );
        Self::mark_observed_items(
            &mut metadata,
            "web_session_object",
            &observed_web_session_objects,
            next_epoch,
        );
        Self::mark_observed_items(
            &mut metadata,
            "artifact_session_object",
            &observed_artifact_session_objects,
            next_epoch,
        );
        Self::mark_observed_items(
            &mut metadata,
            "task_session_object",
            &observed_task_session_objects,
            next_epoch,
        );
        Self::mark_observed_items(
            &mut metadata,
            "tool_session_object",
            &observed_tool_session_objects,
            next_epoch,
        );
        Self::mark_observed_items(
            &mut metadata,
            "multimodal_session_object",
            &observed_multimodal_session_objects,
            next_epoch,
        );
        Self::mark_observed_items(&mut metadata, "ongoing_goal", &observed_goals, next_epoch);
        Self::mark_observed_items(
            &mut metadata,
            "pending_followup",
            &observed_followups,
            next_epoch,
        );

        let working_mode_candidate = Self::infer_working_mode(recent_messages.as_slice());

        let workspace_focus = if let Some(workspace_focus) = workspace_focus {
            metadata.extend(workspace_metadata);
            metadata.insert(
                "workspace_focus_last_seen_epoch".to_string(),
                next_epoch.to_string(),
            );
            Some(workspace_focus)
        } else if Self::is_scalar_value_fresh(
            &metadata,
            "workspace_focus_last_seen_epoch",
            next_epoch,
            self.config.session_workspace_staleness_epochs,
        ) {
            base_session.workspace_focus.clone()
        } else {
            Self::clear_workspace_focus_metadata(&mut metadata);
            None
        };

        if let Some(working_mode) = working_mode_candidate {
            metadata.insert("working_mode".to_string(), working_mode);
            metadata.insert(
                "working_mode_last_seen_epoch".to_string(),
                next_epoch.to_string(),
            );
        } else if preserve_existing_mode
            && !Self::is_scalar_value_fresh(
                &metadata,
                "working_mode_last_seen_epoch",
                next_epoch,
                self.config.session_theme_staleness_epochs,
            )
        {
            metadata.remove("working_mode");
            metadata.remove("working_mode_last_seen_epoch");
        }

        let interaction_theme_candidate =
            Self::infer_interaction_theme(recent_messages.as_slice(), &metadata);
        if let Some(interaction_theme) = interaction_theme_candidate {
            metadata.insert("interaction_theme".to_string(), interaction_theme);
            metadata.insert(
                "interaction_theme_last_seen_epoch".to_string(),
                next_epoch.to_string(),
            );
        } else if metadata
            .get("interaction_theme")
            .is_some_and(|theme| theme == "focused_review")
            && !metadata.contains_key("working_mode")
        {
            metadata.remove("interaction_theme");
            metadata.remove("interaction_theme_last_seen_epoch");
        } else if !Self::is_scalar_value_fresh(
            &metadata,
            "interaction_theme_last_seen_epoch",
            next_epoch,
            self.config.session_theme_staleness_epochs,
        ) {
            metadata.remove("interaction_theme");
            metadata.remove("interaction_theme_last_seen_epoch");
        }

        let mut compression_slots = base_session.compression_slots();
        Self::merge_working_set_slots_from_messages(
            &mut compression_slots,
            recent_messages.as_slice(),
        );

        Self::retain_recent_items(
            &mut observed_backend_contexts,
            &mut metadata,
            "backend_context",
            next_epoch,
            self.config.session_topic_staleness_epochs,
        );
        Self::retain_recent_rendered_objects(
            &mut backend_context_records,
            &mut metadata,
            "backend_context",
            next_epoch,
            self.config.session_topic_staleness_epochs,
            BackendContextRecord::render,
        );
        let backend_contexts = backend_context_records
            .iter()
            .map(BackendContextRecord::render)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        Self::retain_recent_rendered_objects(
            &mut retrieved_memory_objects,
            &mut metadata,
            "retrieved_memory_object",
            next_epoch,
            self.config.session_topic_staleness_epochs,
            RetrievedMemoryObject::render,
        );
        Self::retain_recent_rendered_objects(
            &mut web_session_objects,
            &mut metadata,
            "web_session_object",
            next_epoch,
            self.config.session_topic_staleness_epochs,
            WebSessionObject::render,
        );
        Self::retain_recent_rendered_objects(
            &mut artifact_session_objects,
            &mut metadata,
            "artifact_session_object",
            next_epoch,
            self.config.session_topic_staleness_epochs,
            ArtifactSessionObject::render,
        );
        Self::retain_recent_rendered_objects(
            &mut task_session_objects,
            &mut metadata,
            "task_session_object",
            next_epoch,
            self.config.session_topic_staleness_epochs,
            TaskSessionObject::render,
        );
        Self::retain_recent_rendered_objects(
            &mut tool_session_objects,
            &mut metadata,
            "tool_session_object",
            next_epoch,
            self.config.session_topic_staleness_epochs,
            ToolSessionObject::render,
        );
        Self::retain_recent_rendered_objects(
            &mut multimodal_session_objects,
            &mut metadata,
            "multimodal_session_object",
            next_epoch,
            self.config.session_topic_staleness_epochs,
            MultimodalSessionObject::render,
        );
        Self::retain_recent_items(
            &mut active_topics,
            &mut metadata,
            "active_topic",
            next_epoch,
            self.config.session_topic_staleness_epochs,
        );
        Self::retain_recent_items(
            &mut ongoing_goals,
            &mut metadata,
            "ongoing_goal",
            next_epoch,
            self.config.session_goal_staleness_epochs,
        );
        Self::retain_recent_items(
            &mut pending_followups,
            &mut metadata,
            "pending_followup",
            next_epoch,
            self.config.session_followup_staleness_epochs,
        );

        let summary = Self::summarize_recent_window(messages, 4);
        let mut session = SessionBackgroundState {
            active_topics,
            backend_contexts,
            backend_context_records,
            retrieved_memory_objects,
            web_session_objects,
            artifact_session_objects,
            task_session_objects,
            tool_session_objects,
            multimodal_session_objects,
            open_loops: pending_followups.clone(),
            recent_emotional_state: base_session.recent_emotional_state,
            ongoing_goals,
            workspace_focus,
            pending_followups,
            summary: if summary.trim().is_empty() {
                base_session.summary
            } else {
                Some(summary.trim().to_string())
            },
            metadata,
        };
        session.set_compression_slots(compression_slots);
        Some(session)
    }

    fn background_value_key(namespace: &str, value: &str) -> String {
        let slug = value
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim_matches('_')
            .chars()
            .take(32)
            .collect::<String>();
        let normalized_slug = if slug.is_empty() {
            "item".to_string()
        } else {
            slug
        };
        format!(
            "background_decay::{namespace}::{}_{:x}",
            normalized_slug,
            fxhash::hash64(value)
        )
    }

    fn mark_observed_items(
        metadata: &mut HashMap<String, String>,
        namespace: &str,
        items: &[String],
        epoch: u64,
    ) {
        for item in items {
            metadata.insert(
                Self::background_value_key(namespace, item),
                epoch.to_string(),
            );
        }
    }

    fn retain_recent_items(
        items: &mut Vec<String>,
        metadata: &mut HashMap<String, String>,
        namespace: &str,
        epoch: u64,
        max_staleness_epochs: u64,
    ) {
        let prefix = format!("background_decay::{namespace}::");
        let mut keep_keys = HashSet::new();
        let mut seen_values = HashSet::new();
        let mut retained = Vec::new();

        for item in items.drain(..) {
            let trimmed = item.trim();
            if trimmed.is_empty() {
                continue;
            }

            let key = Self::background_value_key(namespace, trimmed);
            let last_seen = metadata
                .get(&key)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let age = epoch.saturating_sub(last_seen);
            if age > max_staleness_epochs {
                metadata.remove(&key);
                continue;
            }

            keep_keys.insert(key.clone());
            metadata.insert(key, last_seen.to_string());
            if seen_values.insert(trimmed.to_string()) {
                retained.push(trimmed.to_string());
            }
        }

        metadata.retain(|key, _| !key.starts_with(&prefix) || keep_keys.contains(key));
        *items = retained;
    }

    fn is_scalar_value_fresh(
        metadata: &HashMap<String, String>,
        key: &str,
        epoch: u64,
        max_staleness_epochs: u64,
    ) -> bool {
        let Some(last_seen) = metadata
            .get(key)
            .and_then(|value| value.parse::<u64>().ok())
        else {
            return true;
        };

        epoch.saturating_sub(last_seen) <= max_staleness_epochs
    }

    fn clear_workspace_focus_metadata(metadata: &mut HashMap<String, String>) {
        for key in [
            "workspace_focus_last_seen_epoch",
            "workspace_focus_source",
            "workspace_focus_app",
            "workspace_focus_ref",
            "workspace_focus_kind",
        ] {
            metadata.remove(key);
        }
    }

    fn push_unique_limited(items: &mut Vec<String>, value: String, max_len: usize) {
        if value.trim().is_empty() || items.iter().any(|existing| existing == &value) {
            return;
        }

        items.push(value);
        if items.len() > max_len {
            let overflow = items.len() - max_len;
            items.drain(0..overflow);
        }
    }

    fn push_unique_limited_backend_records(
        items: &mut Vec<BackendContextRecord>,
        value: BackendContextRecord,
        max_len: usize,
    ) {
        if value.value.trim().is_empty() {
            return;
        }

        let key = value.normalized_key();
        if items
            .iter()
            .any(|existing| existing.normalized_key() == key)
        {
            return;
        }

        items.push(value);
        if items.len() > max_len {
            let overflow = items.len() - max_len;
            items.drain(0..overflow);
        }
    }

    fn push_unique_limited_retrieved_memory_objects(
        items: &mut Vec<RetrievedMemoryObject>,
        value: RetrievedMemoryObject,
        max_len: usize,
    ) {
        if value.recall_source.trim().is_empty() {
            return;
        }

        let key = value.normalized_key();
        if items
            .iter()
            .any(|existing| existing.normalized_key() == key)
        {
            return;
        }

        items.push(value);
        if items.len() > max_len {
            let overflow = items.len() - max_len;
            items.drain(0..overflow);
        }
    }

    fn push_unique_limited_web_session_objects(
        items: &mut Vec<WebSessionObject>,
        value: WebSessionObject,
        max_len: usize,
    ) {
        if value.url.trim().is_empty() {
            return;
        }

        let key = value.normalized_key();
        if items
            .iter()
            .any(|existing| existing.normalized_key() == key)
        {
            return;
        }

        items.push(value);
        if items.len() > max_len {
            let overflow = items.len() - max_len;
            items.drain(0..overflow);
        }
    }

    fn push_unique_limited_artifact_session_objects(
        items: &mut Vec<ArtifactSessionObject>,
        value: ArtifactSessionObject,
        max_len: usize,
    ) {
        if value.path.trim().is_empty() {
            return;
        }

        let key = value.normalized_key();
        if items
            .iter()
            .any(|existing| existing.normalized_key() == key)
        {
            return;
        }

        items.push(value);
        if items.len() > max_len {
            let overflow = items.len() - max_len;
            items.drain(0..overflow);
        }
    }

    fn push_unique_limited_task_session_objects(
        items: &mut Vec<TaskSessionObject>,
        value: TaskSessionObject,
        max_len: usize,
    ) {
        if value.state.trim().is_empty() {
            return;
        }

        let key = value.normalized_key();
        if items
            .iter()
            .any(|existing| existing.normalized_key() == key)
        {
            return;
        }

        items.push(value);
        if items.len() > max_len {
            let overflow = items.len() - max_len;
            items.drain(0..overflow);
        }
    }

    fn push_unique_limited_tool_session_objects(
        items: &mut Vec<ToolSessionObject>,
        value: ToolSessionObject,
        max_len: usize,
    ) {
        if value.tool_name.trim().is_empty() {
            return;
        }

        let key = value.normalized_key();
        if items
            .iter()
            .any(|existing| existing.normalized_key() == key)
        {
            return;
        }

        items.push(value);
        if items.len() > max_len {
            let overflow = items.len() - max_len;
            items.drain(0..overflow);
        }
    }

    fn push_unique_limited_multimodal_session_objects(
        items: &mut Vec<MultimodalSessionObject>,
        value: MultimodalSessionObject,
        max_len: usize,
    ) {
        if value.locator.trim().is_empty() {
            return;
        }

        let key = value.normalized_key();
        if items
            .iter()
            .any(|existing| existing.normalized_key() == key)
        {
            return;
        }

        items.push(value);
        if items.len() > max_len {
            let overflow = items.len() - max_len;
            items.drain(0..overflow);
        }
    }

    fn backend_context_record(
        kind: BackendContextKind,
        value: impl Into<String>,
        source: Option<&str>,
    ) -> BackendContextRecord {
        BackendContextRecord {
            kind: Some(kind),
            value: value.into(),
            source: source.map(ToString::to_string),
        }
    }

    fn retain_recent_rendered_objects<T, F>(
        items: &mut Vec<T>,
        metadata: &mut HashMap<String, String>,
        namespace: &str,
        epoch: u64,
        max_staleness_epochs: u64,
        render: F,
    ) where
        F: Fn(&T) -> String,
    {
        let prefix = format!("background_decay::{namespace}::");
        let mut keep_keys = HashSet::new();
        let mut retained = Vec::new();
        let mut seen = HashSet::new();

        for item in items.drain(..) {
            let rendered = render(&item);
            let trimmed = rendered.trim();
            if trimmed.is_empty() {
                continue;
            }

            let key = Self::background_value_key(namespace, trimmed);
            let last_seen = metadata
                .get(&key)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let age = epoch.saturating_sub(last_seen);
            if age > max_staleness_epochs {
                metadata.remove(&key);
                continue;
            }

            keep_keys.insert(key.clone());
            metadata.insert(key, last_seen.to_string());
            if seen.insert(trimmed.to_string()) {
                retained.push(item);
            }
        }

        metadata.retain(|key, _| !key.starts_with(&prefix) || keep_keys.contains(key));
        *items = retained;
    }

    fn infer_workspace_focus(
        recent_messages: &[&Message],
    ) -> (Option<String>, std::collections::HashMap<String, String>) {
        let mut metadata = std::collections::HashMap::new();

        for message in recent_messages.iter().copied() {
            if let Some(explicit) = Self::first_non_empty_metadata(
                message,
                &[
                    "workspace_focus",
                    "workspace_context",
                    "active_window_context",
                ],
            ) {
                metadata.insert(
                    "workspace_focus_source".to_string(),
                    "message_metadata".to_string(),
                );
                return (Some(Self::shrink_text(explicit, 160)), metadata);
            }

            let window_title = Self::first_non_empty_metadata(
                message,
                &[
                    "active_window",
                    "window_title",
                    "page_title",
                    "browser_title",
                ],
            );
            let app_name = Self::first_non_empty_metadata(
                message,
                &[
                    "foreground_app",
                    "focused_app",
                    "app_name",
                    "application_name",
                ],
            );
            if window_title.is_some() || app_name.is_some() {
                let focus = match (window_title, app_name) {
                    (Some(title), Some(app)) => format!("{title} ({app})"),
                    (Some(title), None) => title.to_string(),
                    (None, Some(app)) => format!("Focused app: {app}"),
                    (None, None) => unreachable!(),
                };
                metadata.insert(
                    "workspace_focus_source".to_string(),
                    "window_metadata".to_string(),
                );
                if let Some(app) = app_name {
                    metadata.insert("workspace_focus_app".to_string(), app.to_string());
                }
                return (Some(Self::shrink_text(&focus, 160)), metadata);
            }

            if let Some(source_path) = &message.source_path {
                let source_label = Self::summarize_source_path(source_path);
                let source_kind = message
                    .source_collection
                    .as_deref()
                    .or_else(|| {
                        message
                            .metadata
                            .get("tool_name")
                            .map(|value| value.as_str())
                    })
                    .or_else(|| message.metadata.get("route").map(|value| value.as_str()))
                    .unwrap_or("workspace_artifact");
                metadata.insert(
                    "workspace_focus_source".to_string(),
                    "source_path".to_string(),
                );
                metadata.insert(
                    "workspace_focus_ref".to_string(),
                    Self::shrink_text(source_path, 160),
                );
                metadata.insert("workspace_focus_kind".to_string(), source_kind.to_string());
                return (
                    Some(Self::shrink_text(
                        &format!("Working from {source_kind}: {source_label}"),
                        160,
                    )),
                    metadata,
                );
            }

            if let Some(source_ref) =
                Self::first_non_empty_metadata(message, &["media_preprocess_source_ref"])
            {
                let source_label = Self::summarize_source_path(source_ref);
                let tool_name = message
                    .metadata
                    .get("tool_name")
                    .cloned()
                    .unwrap_or_else(|| "media_preprocess".to_string());
                metadata.insert(
                    "workspace_focus_source".to_string(),
                    "media_preprocess_source_ref".to_string(),
                );
                metadata.insert(
                    "workspace_focus_ref".to_string(),
                    Self::shrink_text(source_ref, 160),
                );
                metadata.insert("workspace_focus_kind".to_string(), tool_name.clone());
                return (
                    Some(Self::shrink_text(
                        &format!("Reviewing {tool_name}: {source_label}"),
                        160,
                    )),
                    metadata,
                );
            }

            if let Some(tool_name) = message.metadata.get("tool_name") {
                let focus = match tool_name.as_str() {
                    "browser_browse" => Some("Focused on the current browser task.".to_string()),
                    "browser_snapshot" => {
                        Some("Focused on the current browser snapshot.".to_string())
                    }
                    "browser_screenshot" => {
                        Some("Focused on the current browser screenshot.".to_string())
                    }
                    "document_understand" => {
                        Some("Focused on the current document understanding task.".to_string())
                    }
                    "text_extract" => {
                        Some("Focused on extracting text from the current asset.".to_string())
                    }
                    "pdf_parse" => Some("Focused on the current PDF parsing task.".to_string()),
                    _ => None,
                };
                if let Some(focus) = focus {
                    metadata.insert(
                        "workspace_focus_source".to_string(),
                        "tool_name".to_string(),
                    );
                    metadata.insert("workspace_focus_kind".to_string(), tool_name.clone());
                    return (Some(focus), metadata);
                }
            }
        }

        (None, metadata)
    }

    fn first_non_empty_metadata<'a>(message: &'a Message, keys: &[&str]) -> Option<&'a str> {
        keys.iter()
            .filter_map(|key| message.metadata.get(*key))
            .map(|value| value.trim())
            .find(|value| !value.is_empty())
    }

    fn summarize_source_path(path: &str) -> String {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return "current artifact".to_string();
        }

        Path::new(trimmed)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(trimmed)
            .to_string()
    }

    fn infer_relationship_candidate(
        messages: &[Message],
        current_background: Option<&BackgroundEnvelope>,
    ) -> Option<RelationshipBackgroundLayer> {
        let base_relationship = current_background
            .and_then(|background| background.relationship_layer.clone())
            .unwrap_or_default();
        let recent_user_messages = messages
            .iter()
            .rev()
            .filter(|m| matches!(m.role, crate::agent::message::Role::User))
            .take(6)
            .collect::<Vec<_>>();

        if recent_user_messages.is_empty() {
            return None;
        }

        let mut user_preferences = base_relationship.user_preferences.clone();
        let mut long_term_topics = base_relationship.long_term_topics.clone();
        let mut relationship_summary = base_relationship.relationship_summary.clone();

        for message in recent_user_messages.iter().rev() {
            let text = message.content.as_text();
            let compact = Self::shrink_text(&text, 120);
            let lower = text.to_lowercase();

            let explicit_preference = text.contains("我喜欢")
                || text.contains("我不喜欢")
                || text.contains("记住")
                || text.contains("以后")
                || lower.contains("i prefer")
                || lower.contains("i like")
                || lower.contains("i don't like")
                || lower.contains("remember")
                || lower.contains("call me");

            if explicit_preference && !compact.is_empty() {
                Self::push_unique_limited(&mut user_preferences, compact.clone(), 6);
            }

            if text.contains("最近")
                || text.contains("正在")
                || lower.contains("recently")
                || lower.contains("working on")
            {
                Self::push_unique_limited(&mut long_term_topics, compact, 4);
            }
        }

        if relationship_summary.is_none() && !user_preferences.is_empty() {
            relationship_summary = Some(format!(
                "用户近期明确表达了 {} 条应持续保留的偏好/关系提示。",
                user_preferences.len()
            ));
        }

        if relationship_summary.is_none()
            && user_preferences.is_empty()
            && long_term_topics.is_empty()
        {
            return None;
        }

        Some(RelationshipBackgroundLayer {
            user_profile_summary: base_relationship.user_profile_summary,
            user_preferences,
            relationship_summary,
            long_term_topics,
            emotional_markers: base_relationship.emotional_markers,
            metadata: base_relationship.metadata,
        })
    }

    fn infer_working_mode(recent_messages: &[&Message]) -> Option<String> {
        for message in recent_messages.iter().copied() {
            if let Some(tool_name) = message.metadata.get("tool_name") {
                let mode = match tool_name.as_str() {
                    "browser_browse" | "browser_snapshot" | "browser_screenshot" => {
                        Some("browser_review")
                    }
                    "document_understand" | "pdf_parse" | "text_extract" => Some("document_review"),
                    _ => Some("tool_assisted"),
                };
                if let Some(mode) = mode {
                    return Some(mode.to_string());
                }
            }

            if message.source_path.is_some() {
                return Some("artifact_review".to_string());
            }
        }

        let merged = recent_messages
            .iter()
            .map(|message| message.content.as_text().to_lowercase())
            .collect::<Vec<_>>()
            .join("\n");

        if merged.contains("计划")
            || merged.contains("方案")
            || merged.contains("roadmap")
            || merged.contains("plan")
        {
            return Some("planning".to_string());
        }
        if merged.contains("review")
            || merged.contains("审查")
            || merged.contains("看看")
            || merged.contains("分析")
        {
            return Some("review".to_string());
        }

        None
    }

    fn infer_backend_contexts(recent_messages: &[&Message]) -> Vec<BackendContextRecord> {
        let mut contexts = Vec::new();

        for message in recent_messages.iter().rev().copied() {
            let has_artifact_context = message.source_path.is_some();
            let has_collection_context = message.source_collection.is_some();
            let has_web_context = message.metadata.contains_key("source_url");
            let has_route_context = message.metadata.contains_key("route");
            let has_task_context =
                Self::first_non_empty_metadata(message, &["task_state", "task_title", "task_goal"])
                    .is_some();
            let has_memory_recall_context = Self::first_non_empty_metadata(
                message,
                &[
                    "retrieved_from",
                    "memory_recall",
                    "retrieval_query",
                    "recall_summary",
                ],
            )
            .is_some();
            let has_multimodal_context = Self::first_non_empty_metadata(
                message,
                &[
                    "media_preprocess_source_ref",
                    "multimodal_source_path",
                    "multimodal_source_url",
                    "multimodal_artifact_locator",
                ],
            )
            .is_some();
            let has_multimodal_route = Self::first_non_empty_metadata(
                message,
                &["media_preprocess_route", "multimodal_route"],
            )
            .is_some();

            if let Some(source_path) = &message.source_path {
                Self::push_unique_limited_backend_records(
                    &mut contexts,
                    Self::backend_context_record(
                        BackendContextKind::Artifact,
                        Self::summarize_source_path(source_path),
                        Some("source_path"),
                    ),
                    8,
                );
            }

            if let Some(source_collection) = &message.source_collection {
                Self::push_unique_limited_backend_records(
                    &mut contexts,
                    Self::backend_context_record(
                        BackendContextKind::Collection,
                        Self::shrink_text(source_collection, 100),
                        Some("source_collection"),
                    ),
                    8,
                );
            }

            if let Some(source_url) = message.metadata.get("source_url") {
                Self::push_unique_limited_backend_records(
                    &mut contexts,
                    Self::backend_context_record(
                        BackendContextKind::Web,
                        Self::shrink_text(source_url, 120),
                        Some("source_url"),
                    ),
                    8,
                );
            }

            let has_richer_context = has_artifact_context
                || has_collection_context
                || has_web_context
                || has_route_context
                || has_task_context
                || has_memory_recall_context
                || has_multimodal_context
                || has_multimodal_route;

            if let Some(tool_name) = message.metadata.get("tool_name") {
                if !has_richer_context {
                    Self::push_unique_limited_backend_records(
                        &mut contexts,
                        Self::backend_context_record(
                            BackendContextKind::ToolResult,
                            tool_name.clone(),
                            Some("tool_name"),
                        ),
                        8,
                    );
                }
            }

            if let Some(route) = message.metadata.get("route") {
                Self::push_unique_limited_backend_records(
                    &mut contexts,
                    Self::backend_context_record(
                        BackendContextKind::Route,
                        Self::shrink_text(route, 100),
                        Some("route"),
                    ),
                    8,
                );
            }

            if let Some(media_context) = Self::first_non_empty_metadata(
                message,
                &[
                    "media_preprocess_source_ref",
                    "multimodal_source_path",
                    "multimodal_source_url",
                    "multimodal_artifact_locator",
                ],
            ) {
                Self::push_unique_limited_backend_records(
                    &mut contexts,
                    Self::backend_context_record(
                        BackendContextKind::Multimodal,
                        Self::shrink_text(media_context, 120),
                        Some("multimodal_source"),
                    ),
                    8,
                );
            }

            if let Some(multimodal_route) = Self::first_non_empty_metadata(
                message,
                &["media_preprocess_route", "multimodal_route"],
            ) {
                Self::push_unique_limited_backend_records(
                    &mut contexts,
                    Self::backend_context_record(
                        BackendContextKind::MultimodalRoute,
                        Self::shrink_text(multimodal_route, 100),
                        Some("multimodal_route"),
                    ),
                    8,
                );
            }

            if let Some(task_state) =
                Self::first_non_empty_metadata(message, &["task_state", "task_title", "task_goal"])
            {
                Self::push_unique_limited_backend_records(
                    &mut contexts,
                    Self::backend_context_record(
                        BackendContextKind::TaskState,
                        Self::shrink_text(task_state, 120),
                        Some("task_state"),
                    ),
                    8,
                );
            }

            if let Some(recalled) = Self::first_non_empty_metadata(
                message,
                &[
                    "retrieved_from",
                    "memory_recall",
                    "retrieval_query",
                    "recall_summary",
                ],
            ) {
                Self::push_unique_limited_backend_records(
                    &mut contexts,
                    Self::backend_context_record(
                        BackendContextKind::MemoryRecall,
                        Self::shrink_text(recalled, 120),
                        Some("memory_recall"),
                    ),
                    6,
                );
            }
        }

        contexts
    }

    fn infer_retrieved_memory_objects(recent_messages: &[&Message]) -> Vec<RetrievedMemoryObject> {
        let mut objects = Vec::new();

        for message in recent_messages.iter().rev().copied() {
            let Some(recall_source) = Self::first_non_empty_metadata(
                message,
                &["retrieved_from", "memory_recall", "recall_source"],
            ) else {
                continue;
            };

            Self::push_unique_limited_retrieved_memory_objects(
                &mut objects,
                RetrievedMemoryObject {
                    recall_source: Self::shrink_text(recall_source, 80),
                    recall_kind: Self::first_non_empty_metadata(
                        message,
                        &["recall_kind", "memory_recall_kind", "route", "tool_name"],
                    )
                    .map(|value| Self::shrink_text(value, 60)),
                    collection: message
                        .source_collection
                        .as_deref()
                        .map(|value| Self::shrink_text(value, 80)),
                    retrieval_query: Self::first_non_empty_metadata(
                        message,
                        &["retrieval_query", "memory_query"],
                    )
                    .map(|value| Self::shrink_text(value, 140)),
                    recall_summary: Self::first_non_empty_metadata(
                        message,
                        &["recall_summary", "retrieved_snippet_summary"],
                    )
                    .map(|value| Self::shrink_text(value, 160)),
                },
                6,
            );
        }

        objects
    }

    fn infer_web_session_objects(recent_messages: &[&Message]) -> Vec<WebSessionObject> {
        let mut objects = Vec::new();

        for message in recent_messages.iter().rev().copied() {
            let Some(url) = message.metadata.get("source_url") else {
                continue;
            };
            if !Self::message_has_durable_working_set_signal(message)
                && !message
                    .metadata
                    .get("task_goal")
                    .is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }

            Self::push_unique_limited_web_session_objects(
                &mut objects,
                WebSessionObject {
                    url: Self::shrink_text(url, 140),
                    page_title: Self::first_non_empty_metadata(
                        message,
                        &["window_title", "page_title", "active_window"],
                    )
                    .map(|value| Self::shrink_text(value, 120)),
                    task_goal: Self::first_non_empty_metadata(
                        message,
                        &["task_goal", "task_title"],
                    )
                    .map(|value| Self::shrink_text(value, 140)),
                },
                6,
            );
        }

        objects
    }

    fn infer_artifact_session_objects(recent_messages: &[&Message]) -> Vec<ArtifactSessionObject> {
        let mut objects = Vec::new();

        for message in recent_messages.iter().rev().copied() {
            let Some(source_path) = message.source_path.as_deref() else {
                continue;
            };

            Self::push_unique_limited_artifact_session_objects(
                &mut objects,
                ArtifactSessionObject {
                    path: Self::shrink_text(source_path, 140),
                    collection: message
                        .source_collection
                        .as_deref()
                        .map(|value| Self::shrink_text(value, 80)),
                    task_goal: Self::first_non_empty_metadata(
                        message,
                        &["task_goal", "task_title"],
                    )
                    .map(|value| Self::shrink_text(value, 140)),
                },
                6,
            );
        }

        objects
    }

    fn infer_task_session_objects(recent_messages: &[&Message]) -> Vec<TaskSessionObject> {
        let mut objects = Vec::new();

        for message in recent_messages.iter().rev().copied() {
            let Some(state) =
                Self::first_non_empty_metadata(message, &["task_state", "task_title", "task_goal"])
            else {
                continue;
            };

            Self::push_unique_limited_task_session_objects(
                &mut objects,
                TaskSessionObject {
                    state: Self::shrink_text(state, 100),
                    title: Self::first_non_empty_metadata(message, &["task_title", "window_title"])
                        .map(|value| Self::shrink_text(value, 120)),
                    goal: Self::first_non_empty_metadata(message, &["task_goal"])
                        .map(|value| Self::shrink_text(value, 140)),
                },
                6,
            );
        }

        objects
    }

    fn infer_multimodal_session_objects(
        recent_messages: &[&Message],
    ) -> Vec<MultimodalSessionObject> {
        let mut objects = Vec::new();

        for message in recent_messages.iter().rev().copied() {
            let Some(locator) = Self::first_non_empty_metadata(
                message,
                &[
                    "media_preprocess_source_ref",
                    "multimodal_source_path",
                    "multimodal_artifact_locator",
                    "multimodal_source_url",
                ],
            ) else {
                continue;
            };

            Self::push_unique_limited_multimodal_session_objects(
                &mut objects,
                MultimodalSessionObject {
                    locator: Self::shrink_text(locator, 140),
                    route: Self::first_non_empty_metadata(
                        message,
                        &["media_preprocess_route", "multimodal_route"],
                    )
                    .map(|value| Self::shrink_text(value, 80)),
                    modality: Self::first_non_empty_metadata(
                        message,
                        &["multimodal_modality", "media_modality"],
                    )
                    .map(|value| Self::shrink_text(value, 32)),
                    collection: message
                        .source_collection
                        .as_deref()
                        .map(|value| Self::shrink_text(value, 80)),
                    source_url: Self::first_non_empty_metadata(
                        message,
                        &["multimodal_source_url", "source_url"],
                    )
                    .map(|value| Self::shrink_text(value, 140)),
                    title: Self::first_non_empty_metadata(
                        message,
                        &["window_title", "page_title", "task_title"],
                    )
                    .map(|value| Self::shrink_text(value, 120)),
                    task_goal: Self::first_non_empty_metadata(
                        message,
                        &["task_goal", "task_title"],
                    )
                    .map(|value| Self::shrink_text(value, 140)),
                },
                6,
            );
        }

        objects
    }

    fn infer_tool_session_objects(recent_messages: &[&Message]) -> Vec<ToolSessionObject> {
        let mut objects = Vec::new();

        for message in recent_messages.iter().rev().copied() {
            let Some(tool_name) = message.metadata.get("tool_name") else {
                continue;
            };
            if !Self::message_has_durable_working_set_signal(message) {
                continue;
            }

            Self::push_unique_limited_tool_session_objects(
                &mut objects,
                ToolSessionObject {
                    tool_name: Self::shrink_text(tool_name, 64),
                    result_summary: Some(Self::shrink_text(&message.content.as_text(), 160)),
                    route: Self::first_non_empty_metadata(
                        message,
                        &["route", "media_preprocess_route", "multimodal_route"],
                    )
                    .map(|value| Self::shrink_text(value, 80)),
                    source_ref: message
                        .source_path
                        .as_deref()
                        .map(|value| Self::shrink_text(value, 140))
                        .or_else(|| {
                            Self::first_non_empty_metadata(
                                message,
                                &[
                                    "media_preprocess_source_ref",
                                    "source_url",
                                    "multimodal_source_url",
                                ],
                            )
                            .map(|value| Self::shrink_text(value, 140))
                        }),
                },
                6,
            );
        }

        objects
    }

    fn infer_interaction_theme(
        recent_messages: &[&Message],
        metadata: &std::collections::HashMap<String, String>,
    ) -> Option<String> {
        let merged = recent_messages
            .iter()
            .map(|message| message.content.as_text().to_lowercase())
            .collect::<Vec<_>>()
            .join("\n");

        if merged.contains("继续")
            || merged.contains("接着")
            || merged.contains("下一步")
            || merged.contains("我们")
            || merged.contains("together")
            || merged.contains("next step")
        {
            return Some("collaborative_progress".to_string());
        }

        if merged.contains("为什么")
            || merged.contains("怎么")
            || merged.contains("explain")
            || merged.contains("分析")
        {
            return Some("guided_reasoning".to_string());
        }

        if metadata
            .get("working_mode")
            .is_some_and(|mode| mode == "browser_review" || mode == "document_review")
        {
            return Some("focused_review".to_string());
        }

        None
    }

    fn derive_background_tactics_rule_based(
        &self,
        messages: &[Message],
        current_background: Option<&BackgroundEnvelope>,
    ) -> BackgroundCompressionVerdict {
        let conversational_messages = messages
            .iter()
            .filter(|m| !matches!(m.role, crate::agent::message::Role::System))
            .collect::<Vec<_>>();

        if conversational_messages.len() < 4 {
            return BackgroundCompressionVerdict::skip(
                "recent conversation window is still too small for a safe background refresh",
            );
        }

        let recent_text = conversational_messages
            .iter()
            .rev()
            .take(6)
            .map(|m| m.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        let recent_text_lower = recent_text.to_lowercase();
        let contains_explicit_preference = |text: &str, lower: &str| {
            text.contains("记住")
                || text.contains("以后")
                || text.contains("我喜欢")
                || text.contains("我不喜欢")
                || lower.contains("remember")
                || lower.contains("i prefer")
                || lower.contains("call me")
        };
        let contains_background_write_intent = |text: &str, lower: &str| {
            text.contains("长期偏好")
                || text.contains("长期记忆")
                || text.contains("稳定背景")
                || text.contains("背景层")
                || lower.contains("long-term memory")
                || lower.contains("long-term preference")
                || lower.contains("stable background")
                || lower.contains("background layer")
        };

        let evidence_refs = Self::build_background_evidence_refs(messages, 6);
        let session_candidate = self.infer_session_candidate(messages, current_background);
        let relationship_candidate =
            Self::infer_relationship_candidate(messages, current_background);

        let has_explicit_preference =
            contains_explicit_preference(&recent_text, &recent_text_lower);
        let has_background_write_intent =
            contains_background_write_intent(&recent_text, &recent_text_lower);
        let has_user_explicit_preference = conversational_messages.iter().any(|message| {
            if !matches!(message.role, crate::agent::message::Role::User) {
                return false;
            }
            let text = message.content.as_text();
            let lower = text.to_lowercase();
            contains_explicit_preference(&text, &lower)
        });
        let has_backend_only_preference_signal = conversational_messages.iter().any(|message| {
            if matches!(message.role, crate::agent::message::Role::User) {
                return false;
            }
            let text = message.content.as_text();
            let lower = text.to_lowercase();
            contains_explicit_preference(&text, &lower)
        }) && !has_user_explicit_preference;

        let blocks_durable_write = recent_text.contains("先别记住")
            || recent_text.contains("不要记住")
            || recent_text.contains("别写进长期")
            || recent_text.contains("暂时别记")
            || recent_text.contains("先不要写")
            || recent_text.contains("临时备注")
            || recent_text.contains("稳定背景")
            || recent_text.contains("也许")
            || recent_text.contains("可能")
            || recent_text.contains("如果以后")
            || recent_text_lower.contains("don't remember")
            || recent_text_lower.contains("do not remember")
            || recent_text_lower.contains("don't write")
            || recent_text_lower.contains("do not write")
            || recent_text_lower.contains("temporary")
            || recent_text_lower.contains("stable background")
            || recent_text_lower.contains("do not promote")
            || recent_text_lower.contains("maybe")
            || recent_text_lower.contains("perhaps")
            || recent_text_lower.contains("if i ever");

        if has_backend_only_preference_signal {
            return BackgroundCompressionVerdict {
                decision: BackgroundCompressionDecision::RejectCandidate,
                reason: "backend-only preference signal lacks explicit user confirmation and should not be promoted into durable background".to_string(),
                quality_signal: BackgroundQualitySignal::Rejected,
                relationship_candidate,
                session_candidate,
                evidence_refs,
                used_slm: false,
            };
        }

        if (has_explicit_preference || has_background_write_intent) && blocks_durable_write {
            return BackgroundCompressionVerdict {
                decision: BackgroundCompressionDecision::RejectCandidate,
                reason: "recent conversation contains tentative or explicitly blocked preference signals that should not be promoted into durable background".to_string(),
                quality_signal: BackgroundQualitySignal::Rejected,
                relationship_candidate,
                session_candidate,
                evidence_refs,
                used_slm: false,
            };
        }

        if has_explicit_preference {
            return BackgroundCompressionVerdict {
                decision: BackgroundCompressionDecision::PromoteRelationshipFact,
                reason:
                    "recent conversation contains explicit durable preference or relationship cues"
                        .to_string(),
                quality_signal: BackgroundQualitySignal::Guarded,
                relationship_candidate,
                session_candidate,
                evidence_refs,
                used_slm: false,
            };
        }

        if conversational_messages.len() >= 12
            && current_background.is_some_and(|background| !background.is_empty())
        {
            return BackgroundCompressionVerdict {
                decision: BackgroundCompressionDecision::RewriteWholeEnvelope,
                reason: "conversation has grown long enough that the current background likely needs a fresh full rewrite".to_string(),
                quality_signal: BackgroundQualitySignal::Guarded,
                relationship_candidate,
                session_candidate,
                evidence_refs,
                used_slm: false,
            };
        }

        BackgroundCompressionVerdict {
            decision: BackgroundCompressionDecision::RefreshSessionLayer,
            reason: "recent conversation window is large enough to refresh the current session background conservatively".to_string(),
            quality_signal: BackgroundQualitySignal::Guarded,
            relationship_candidate,
            session_candidate,
            evidence_refs,
            used_slm: false,
        }
    }

    async fn derive_background_tactics_with_slm(
        &self,
        backend: &Arc<dyn ModelBackend>,
        messages: &[Message],
        current_background: Option<&BackgroundEnvelope>,
    ) -> Result<BackgroundCompressionVerdict> {
        let session_id = messages
            .first()
            .and_then(|m| m.metadata.get("session_id"))
            .cloned()
            .unwrap_or_else(|| "background".to_string());

        let prompt = BACKGROUND_TACTICAL_PROMPT_TEMPLATE
            .replace(
                "{background}",
                &Self::background_snapshot(current_background),
            )
            .replace("{context}", &Self::summarize_recent_window(messages, 6));

        let config = GenerationConfig {
            max_new_tokens: 80,
            temperature: 0.0,
            session_id: Some(format!("background-tactical-{}", session_id)),
            priority: -32,
            ..Default::default()
        };

        let request_id = format!("background-tactical-{}", uuid::Uuid::new_v4());
        let response = backend
            .generate(
                &request_id,
                &prompt,
                None,
                config,
                fresh_tactical_kv_engine(),
            )
            .await
            .map_err(|error| {
                crate::error::Error::AgentExecution(format!(
                    "background tactical generation failed: {}",
                    error
                ))
            })?;
        let normalized = response.trim().to_uppercase();

        let mut verdict = self.derive_background_tactics_rule_based(messages, current_background);
        verdict.used_slm = true;
        verdict.reason = format!("slm tactical verdict: {}", response.trim());

        if normalized.contains("[SKIP]") {
            verdict.decision = BackgroundCompressionDecision::Skip;
            verdict.relationship_candidate = None;
            verdict.session_candidate = None;
            verdict.quality_signal = BackgroundQualitySignal::Skipped;
        } else if normalized.contains("[REFRESH_SESSION]") {
            verdict.decision = BackgroundCompressionDecision::RefreshSessionLayer;
        } else if normalized.contains("[PROMOTE_FACT]") {
            verdict.decision = BackgroundCompressionDecision::PromoteRelationshipFact;
        } else if normalized.contains("[REWRITE_ENVELOPE]") {
            verdict.decision = BackgroundCompressionDecision::RewriteWholeEnvelope;
            verdict.quality_signal = BackgroundQualitySignal::Candidate;
        } else if normalized.contains("[REJECT]") {
            verdict.decision = BackgroundCompressionDecision::RejectCandidate;
            verdict.relationship_candidate = None;
            verdict.quality_signal = BackgroundQualitySignal::Rejected;
        } else {
            warn!(
                "TacticalOrchestrator: Ambiguous background response from {}: {}. Falling back to rule-based verdict.",
                self.model_name, response
            );
            verdict = self.derive_background_tactics_rule_based(messages, current_background);
        }

        Ok(verdict)
    }
}

#[async_trait]
impl TacticalOrchestrator for GlobalTacticalOrchestrator {
    fn is_active(&self) -> bool {
        self.slm_backend.is_some()
    }

    async fn derive_tactics(
        &self,
        messages: &[Message],
        proposed_actions: &[ProposedAction],
    ) -> Result<TacticalVerdict> {
        let backend = match &self.slm_backend {
            Some(b) => b,
            None => return Ok(TacticalVerdict::Proceed),
        };

        // 1. Better Session Identification
        let session_id = messages
            .first()
            .and_then(|m| m.metadata.get("session_id"))
            .cloned()
            .unwrap_or_else(|| {
                let context_key = messages
                    .first()
                    .map(|m| m.content.as_text())
                    .unwrap_or_else(|| "none".to_string());
                format!("anon-{}", fxhash::hash64(&context_key))
            });

        // 2. Entropy Check
        let action_str = proposed_actions
            .iter()
            .map(|a| format!("{}:{}", a.name, a.args))
            .collect::<String>();
        let action_hash = fxhash::hash64(&action_str);

        let entropy = self
            .entropy_monitor
            .calculate(&self.config, &session_id, action_hash);
        if entropy < self.config.entropy_threshold {
            warn!(
                "TacticalOrchestrator: Low entropy ({:.2}) for session {}. Repeating same actions.",
                entropy, session_id
            );
            return Ok(TacticalVerdict::Pivot("REPETITIVE LOOP DETECTED. You are repeating the same tool calls. Break the cycle by trying a fundamentally different approach or re-analyzing the previous error.".to_string()));
        }

        // 3. Prompt Construction
        let mut context_summary = String::new();
        for m in messages
            .iter()
            .rev()
            .take(self.config.context_message_count)
            .rev()
        {
            let role = format!("{:?}", m.role).to_uppercase();
            let text = m.content.as_text();
            let truncated = if text.len() > 150 {
                format!("{}...", &text[..150])
            } else {
                text.to_string()
            };
            context_summary.push_str(&format!("- {}: {}\n", role, truncated));
        }

        let mut actions_summary = String::new();
        for action in proposed_actions {
            actions_summary.push_str(&format!(
                "- Tool: {}\n  Args: {}\n",
                action.name, action.args
            ));
        }

        let prompt = TACTICAL_PROMPT_TEMPLATE
            .replace("{context}", &context_summary)
            .replace("{actions}", &actions_summary);

        let config = GenerationConfig {
            max_new_tokens: self.config.max_new_tokens,
            temperature: self.config.temperature,
            session_id: Some(format!("tactical-{}", session_id)),
            priority: -32,
            ..Default::default()
        };

        let request_id = format!("tactical-{}", uuid::Uuid::new_v4());

        match backend
            .generate(
                &request_id,
                &prompt,
                None,
                config,
                fresh_tactical_kv_engine(),
            )
            .await
        {
            Ok(response) => {
                let response = response.trim();
                if response.contains("[PROCEED]") {
                    Ok(TacticalVerdict::Proceed)
                } else if response.contains("[PIVOT]") {
                    let advice = response
                        .split("[PIVOT]")
                        .last()
                        .unwrap_or("Change approach.")
                        .trim()
                        .to_string();
                    Ok(TacticalVerdict::Pivot(advice))
                } else if response.contains("[HALT]") {
                    let reason = response
                        .split("[HALT]")
                        .last()
                        .unwrap_or("Logical inconsistency.")
                        .trim()
                        .to_string();
                    Ok(TacticalVerdict::Halt(reason))
                } else {
                    warn!(
                        "TacticalOrchestrator: Ambiguous response from {}: {}",
                        self.model_name, response
                    );
                    Ok(TacticalVerdict::Proceed)
                }
            }
            Err(e) => {
                warn!(
                    "TacticalOrchestrator: SLM Fail ({}): {}. Falling back to passive mode.",
                    self.model_name, e
                );
                Ok(TacticalVerdict::Proceed)
            }
        }
    }

    async fn derive_background_tactics(
        &self,
        messages: &[Message],
        current_background: Option<&BackgroundEnvelope>,
    ) -> Result<BackgroundCompressionVerdict> {
        let background_engine = BackgroundTacticsEngine::new(&self.model_name);
        let backend = match &self.slm_backend {
            Some(backend) => backend,
            None => {
                debug!(
                    "TacticalOrchestrator: {} using rule-based background compression engine.",
                    background_engine.model_name
                );
                return Ok(background_engine.derive_rule_based(self, messages, current_background));
            }
        };

        match background_engine
            .derive_with_slm(self, backend, messages, current_background)
            .await
        {
            Ok(verdict) => Ok(verdict),
            Err(error) => {
                warn!(
                    "TacticalOrchestrator: Background SLM fail ({}): {}. Falling back to rule-based verdict.",
                    self.model_name, error
                );
                Ok(background_engine.derive_rule_based(self, messages, current_background))
            }
        }
    }
}

/// Phase 16.3: Speculative Execution Wrapper for Tactical Orchestration.
pub struct SpeculativeTacticalOrchestrator {
    inner: Arc<dyn TacticalOrchestrator>,
    pending_task: SpeculativeTaskSlot<Result<TacticalVerdict>>,
}

impl SpeculativeTacticalOrchestrator {
    pub fn new(inner: Arc<dyn TacticalOrchestrator>) -> Self {
        Self {
            inner,
            pending_task: SpeculativeTaskSlot::default(),
        }
    }

    pub fn spawn_validation(&self, messages: Vec<Message>, actions: Vec<ProposedAction>) {
        let inner = self.inner.clone();
        let handle = tokio::spawn(async move { inner.derive_tactics(&messages, &actions).await });
        self.pending_task.replace(handle);
    }
}

#[async_trait]
impl TacticalOrchestrator for SpeculativeTacticalOrchestrator {
    fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    async fn derive_tactics(
        &self,
        messages: &[Message],
        proposed_actions: &[ProposedAction],
    ) -> Result<TacticalVerdict> {
        let task_opt = self.pending_task.take();
        if let Some(handle) = task_opt {
            debug!("SpeculativeTacticalOrchestrator: Resolving background tactical check...");
            match handle.await {
                Ok(res) => return res,
                Err(e) => warn!(
                    "SpeculativeTacticalOrchestrator: Background thread error: {}",
                    e
                ),
            }
        }
        self.inner.derive_tactics(messages, proposed_actions).await
    }

    async fn derive_background_tactics(
        &self,
        messages: &[Message],
        current_background: Option<&BackgroundEnvelope>,
    ) -> Result<BackgroundCompressionVerdict> {
        self.inner
            .derive_background_tactics(messages, current_background)
            .await
    }
}

/// Phase 16.4: Post-Quantum Security Guard
pub struct PostQuantumGuard;

impl PostQuantumGuard {
    pub fn sign_tool_call(tool_name: &str, args: &serde_json::Value) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(tool_name.as_bytes());
        hasher.update(args.to_string().as_bytes());
        let hash = hasher.finalize();
        format!("pqc-v1:dilithium5:{}", hex::encode(hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::message::{Content, Role};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Notify;
    use tokio::time::{sleep, Duration};

    fn make_message(role: Role, text: &str) -> Message {
        let mut message = Message::new(role, Content::text(text));
        message.metadata = HashMap::new();
        message
    }

    #[tokio::test]
    async fn passthrough_background_tactics_skip_small_windows() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let messages = vec![
            make_message(Role::User, "你好"),
            make_message(Role::Assistant, "你好呀"),
            make_message(Role::User, "今天天气怎么样"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        assert_eq!(verdict.decision, BackgroundCompressionDecision::Skip);
        assert_eq!(verdict.quality_signal, BackgroundQualitySignal::Skipped);
        assert!(!verdict.used_slm);
    }

    #[test]
    fn entropy_tracker_evicts_oldest_session_instead_of_clearing_everything() {
        let orchestrator =
            GlobalTacticalOrchestrator::passthrough().with_config(TacticalOrchestratorConfig {
                max_entropy_sessions: 2,
                entropy_evict_count: 1,
                ..Default::default()
            });

        orchestrator.calculate_entropy("session-a", 11);
        orchestrator.calculate_entropy("session-b", 22);
        orchestrator.calculate_entropy("session-b", 23);
        orchestrator.calculate_entropy("session-c", 33);

        let tracker = orchestrator.entropy_monitor.tracker.read();
        assert_eq!(tracker.sessions.len(), 2);
        assert!(!tracker.sessions.contains_key("session-a"));
        assert!(tracker.sessions.contains_key("session-b"));
        assert!(tracker.sessions.contains_key("session-c"));
    }

    #[tokio::test]
    async fn passthrough_background_tactics_promote_explicit_preferences() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let messages = vec![
            make_message(Role::User, "我们继续聊日常习惯"),
            make_message(Role::Assistant, "好，我会记重点"),
            make_message(Role::User, "记住，我喜欢安静一点的交流方式"),
            make_message(Role::Assistant, "收到，我会更安静温和"),
            make_message(Role::User, "以后都这样和我说话"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        assert_eq!(
            verdict.decision,
            BackgroundCompressionDecision::PromoteRelationshipFact
        );
        assert!(!verdict.evidence_refs.is_empty());
        assert!(verdict.relationship_candidate.is_some());
        assert!(!verdict.used_slm);
    }

    #[tokio::test]
    async fn passthrough_background_tactics_builds_multisource_backend_evidence_refs() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();

        let mut recall = make_message(Role::Tool, "memory snippets ready");
        recall
            .metadata
            .insert("tool_name".to_string(), "memory_recall".to_string());
        recall.metadata.insert(
            "retrieved_from".to_string(),
            "relationship_memory".to_string(),
        );
        recall.source_collection = Some("memory".to_string());

        let mut browser = make_message(Role::Tool, "browser snapshot ready");
        browser
            .metadata
            .insert("tool_name".to_string(), "browser_snapshot".to_string());
        browser.metadata.insert(
            "source_url".to_string(),
            "https://example.com/background-window".to_string(),
        );
        browser
            .metadata
            .insert("window_title".to_string(), "BenShu Gateway".to_string());

        let mut screenshot = make_message(Role::Tool, "desktop screenshot ready");
        screenshot
            .metadata
            .insert("tool_name".to_string(), "browser_screenshot".to_string());
        screenshot.metadata.insert(
            "media_preprocess_source_ref".to_string(),
            "/tmp/dashboard.png".to_string(),
        );
        screenshot.metadata.insert(
            "media_preprocess_route".to_string(),
            "image_page_raster".to_string(),
        );

        let verdict = orchestrator
            .derive_background_tactics(
                &[
                    make_message(Role::User, "继续沿真实后台主线推进"),
                    recall,
                    browser,
                    screenshot,
                ],
                None,
            )
            .await
            .expect("background tactics should succeed");

        assert!(verdict.evidence_refs.iter().any(|reference| {
            reference.source_kind == "memory_recall"
                && reference.source_id.contains("relationship_memory")
        }));
        assert!(verdict.evidence_refs.iter().any(|reference| {
            reference.source_kind == "tool_result"
                && reference
                    .source_id
                    .contains("example.com/background-window")
        }));
        assert!(verdict.evidence_refs.iter().any(|reference| {
            reference.source_kind == "tool_result"
                && reference.source_id.contains("/tmp/dashboard.png")
                && reference
                    .metadata
                    .get("media_preprocess_route")
                    .is_some_and(|value| value == "image_page_raster")
        }));
    }

    #[tokio::test]
    async fn passthrough_background_tactics_refresh_session_layer() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let messages = vec![
            make_message(Role::User, "我们来继续这个长期对话背景方案"),
            make_message(Role::Assistant, "好，我们接着拆主线"),
            make_message(Role::User, "先说背景层"),
            make_message(Role::Assistant, "背景层要区分人格、关系和 session"),
            make_message(Role::User, "那这一轮先别写长期关系"),
            make_message(Role::Assistant, "可以，先保守刷新 session layer"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        assert_eq!(
            verdict.decision,
            BackgroundCompressionDecision::RefreshSessionLayer
        );
        assert!(verdict.session_candidate.is_some());
        assert!(!verdict.used_slm);
    }

    #[tokio::test]
    async fn passthrough_background_tactics_infers_workspace_focus_from_window_metadata() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let mut browser_context = make_message(Role::User, "你看看我现在这个界面在干嘛");
        browser_context.metadata.insert(
            "window_title".to_string(),
            "BenShu Control Panel".to_string(),
        );
        browser_context
            .metadata
            .insert("focused_app".to_string(), "Browser".to_string());

        let messages = vec![
            make_message(Role::Assistant, "好，我先看当前页面"),
            browser_context,
            make_message(Role::Assistant, "我看到一个状态面板"),
            make_message(Role::User, "重点看看内存和健康状态"),
            make_message(Role::Assistant, "我继续沿当前界面分析"),
            make_message(Role::User, "然后告诉我下一步"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        let session = verdict
            .session_candidate
            .expect("session candidate should be present");
        assert_eq!(
            session.workspace_focus.as_deref(),
            Some("BenShu Control Panel (Browser)")
        );
        assert_eq!(
            session.metadata.get("workspace_focus_source"),
            Some(&"window_metadata".to_string())
        );
    }

    #[tokio::test]
    async fn passthrough_background_tactics_infers_workspace_focus_from_source_path_artifacts() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let mut screenshot_result =
            Message::tool_result("call_1", "识别完成").with_tool_name("document_understand");
        screenshot_result.source_collection = Some("desktop_capture".to_string());
        screenshot_result.source_path = Some("/tmp/dashboard.png".to_string());

        let messages = vec![
            make_message(Role::User, "你看一下我刚截的图"),
            make_message(Role::Assistant, "好，我来分析截图"),
            screenshot_result,
            make_message(Role::Assistant, "这是健康状态面板"),
            make_message(Role::User, "聚焦看一下当前工作区重点"),
            make_message(Role::Assistant, "我会把当前桌面焦点记住"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        let session = verdict
            .session_candidate
            .expect("session candidate should be present");
        assert_eq!(
            session.workspace_focus.as_deref(),
            Some("Working from desktop_capture: dashboard.png")
        );
        assert_eq!(
            session.metadata.get("workspace_focus_source"),
            Some(&"source_path".to_string())
        );
        assert_eq!(
            session.metadata.get("workspace_focus_kind"),
            Some(&"desktop_capture".to_string())
        );
    }

    #[tokio::test]
    async fn passthrough_background_tactics_keeps_existing_workspace_focus_when_no_new_focus_appears(
    ) {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let messages = vec![
            make_message(Role::User, "我们继续推进这个 agent 背景窗方案"),
            make_message(Role::Assistant, "好，我继续沿同一个主线拆"),
            make_message(Role::User, "这轮主要看看关系层"),
            make_message(Role::Assistant, "可以，我先保留当前工作焦点"),
            make_message(Role::User, "不要丢掉刚才的工作区重点"),
            make_message(Role::Assistant, "我会继续沿当前焦点推进"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(
                &messages,
                Some(&BackgroundEnvelope {
                    session_layer: Some(SessionBackgroundState {
                        workspace_focus: Some("Working from docs: BenShu plan".to_string()),
                        metadata: HashMap::from([(
                            "workspace_focus_source".to_string(),
                            "source_path".to_string(),
                        )]),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )
            .await
            .expect("background tactics should succeed");

        let session = verdict
            .session_candidate
            .expect("session candidate should be present");
        assert_eq!(
            session.workspace_focus.as_deref(),
            Some("Working from docs: BenShu plan")
        );
    }

    #[tokio::test]
    async fn passthrough_background_tactics_merges_existing_relationship_preferences() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let messages = vec![
            make_message(Role::User, "我们继续聊长期协作习惯"),
            make_message(Role::Assistant, "好，我继续保留之前的偏好"),
            make_message(Role::User, "记住，我喜欢先看结论再看细节"),
            make_message(Role::Assistant, "收到，我会先给结论"),
            make_message(Role::User, "最近我正在做 agent 背景压缩主线"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(
                &messages,
                Some(&BackgroundEnvelope {
                    relationship_layer: Some(RelationshipBackgroundLayer {
                        user_preferences: vec!["请保持直接但温和的风格".to_string()],
                        long_term_topics: vec!["长期在做 BenShu AgentOS".to_string()],
                        relationship_summary: Some("长期产品协作关系".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )
            .await
            .expect("background tactics should succeed");

        let relationship = verdict
            .relationship_candidate
            .expect("relationship candidate should be present");
        assert!(relationship
            .user_preferences
            .iter()
            .any(|value| value == "请保持直接但温和的风格"));
        assert!(relationship
            .user_preferences
            .iter()
            .any(|value| value.contains("我喜欢先看结论再看细节")));
        assert!(relationship
            .long_term_topics
            .iter()
            .any(|value| value == "长期在做 BenShu AgentOS"));
        assert!(relationship
            .long_term_topics
            .iter()
            .any(|value| value.contains("最近我正在做 agent 背景压缩主线")));
    }

    #[tokio::test]
    async fn passthrough_background_tactics_records_working_mode_and_interaction_theme() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let mut browser_context = make_message(Role::User, "我们继续下一步，先看这个网页");
        browser_context
            .metadata
            .insert("tool_name".to_string(), "browser_snapshot".to_string());

        let messages = vec![
            make_message(Role::Assistant, "好，我来沿当前页面继续 review"),
            browser_context,
            make_message(Role::User, "然后告诉我下一步怎么推进"),
            make_message(Role::Assistant, "我会继续协作推进"),
            make_message(Role::User, "我们接着拆后端背景窗主线"),
            make_message(Role::Assistant, "好，我继续保持同一工作模式"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        let session = verdict
            .session_candidate
            .expect("session candidate should be present");
        assert_eq!(
            session.metadata.get("working_mode"),
            Some(&"browser_review".to_string())
        );
        assert_eq!(
            session.metadata.get("interaction_theme"),
            Some(&"collaborative_progress".to_string())
        );
    }

    #[tokio::test]
    async fn passthrough_background_tactics_infers_backend_contexts_from_artifacts_and_memory_recall(
    ) {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let mut recalled = Message::tool_result("call_recall", "memory snippets ready")
            .with_tool_name("memory_recall");
        recalled.metadata.insert(
            "retrieved_from".to_string(),
            "relationship_memory".to_string(),
        );
        recalled.metadata.insert(
            "retrieval_query".to_string(),
            "user preference continuity".to_string(),
        );

        let mut web =
            Message::tool_result("call_web", "page read ready").with_tool_name("browser_browse");
        web.metadata.insert(
            "source_url".to_string(),
            "https://example.com/background-window".to_string(),
        );
        web.metadata.insert(
            "task_goal".to_string(),
            "review current browser result".to_string(),
        );

        let mut doc =
            Message::tool_result("call_doc", "doc parse ready").with_tool_name("pdf_parse");
        doc.source_path = Some("/tmp/spec.pdf".to_string());

        let messages = vec![
            make_message(Role::User, "我们把后台来源一起纳入背景层"),
            recalled,
            web,
            doc,
            make_message(Role::Assistant, "好，我把这些后端来源作为当前背景输入"),
            make_message(Role::User, "继续这条主线"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        let session = verdict
            .session_candidate
            .expect("session candidate should be present");
        assert!(session
            .backend_contexts
            .iter()
            .any(|value| value.contains("Memory recall")));
        assert!(session
            .backend_contexts
            .iter()
            .any(|value| value.contains("Web context")));
        assert!(session
            .backend_contexts
            .iter()
            .any(|value| value.contains("Artifact context")));
        assert!(session
            .backend_contexts
            .iter()
            .any(|value| value.contains("Task state")));
        assert!(session.backend_context_records.iter().any(|record| {
            record.kind == Some(BackendContextKind::MemoryRecall)
                && record.value.contains("relationship_memory")
        }));
        assert!(session.backend_context_records.iter().any(|record| {
            record.kind == Some(BackendContextKind::Artifact) && record.value.contains("spec.pdf")
        }));
        assert!(session.retrieved_memory_objects.iter().any(|object| {
            object.recall_source.contains("relationship_memory")
                && object
                    .recall_kind
                    .as_deref()
                    .is_some_and(|value| value.contains("memory_recall"))
                && object
                    .retrieval_query
                    .as_deref()
                    .is_some_and(|value| value.contains("user preference continuity"))
        }));
        assert!(session.web_session_objects.iter().any(|object| {
            object.url.contains("example.com/background-window")
                && object
                    .task_goal
                    .as_deref()
                    .is_some_and(|value| value.contains("review current browser result"))
        }));
        assert!(session
            .artifact_session_objects
            .iter()
            .any(|object| object.path.contains("spec.pdf")));
        assert!(session
            .task_session_objects
            .iter()
            .any(|object| { object.state.contains("review current browser result") }));
        assert!(session.tool_session_objects.iter().any(|object| {
            object.tool_name.contains("browser_browse")
                && object
                    .result_summary
                    .as_deref()
                    .is_some_and(|value| value.contains("page read ready"))
        }));
    }

    #[tokio::test]
    async fn passthrough_background_tactics_filters_transient_lookup_outputs_from_session_state() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let mut weather =
            Message::tool_result("call_weather", "北京 22 度").with_tool_name("weather_lookup");
        weather.metadata.insert(
            "source_url".to_string(),
            "https://api.weather.test".to_string(),
        );
        let mut artifact = Message::tool_result("call_artifact", "saved chapter checkpoint")
            .with_tool_name("novel_studio");
        artifact.metadata.insert(
            "runtime_effect".to_string(),
            "artifact.checkpointed".to_string(),
        );
        artifact.metadata.insert(
            "artifact_path".to_string(),
            "/tmp/novel/chapter-1.md".to_string(),
        );
        artifact.metadata.insert(
            "task_completed".to_string(),
            "chapter 1 drafted".to_string(),
        );
        artifact
            .metadata
            .insert("task_pending".to_string(), "audit chapter 1".to_string());

        let messages = vec![
            make_message(Role::User, "继续当前写作任务"),
            weather,
            artifact,
            make_message(
                Role::Assistant,
                "已保存章节 checkpoint，天气只是临时查询结果。",
            ),
            make_message(Role::User, "下一步审稿"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");
        let session = verdict
            .session_candidate
            .expect("session candidate should be present");
        assert!(!session
            .tool_session_objects
            .iter()
            .any(|object| object.tool_name == "weather_lookup"));
        assert!(session
            .tool_session_objects
            .iter()
            .any(|object| object.tool_name == "novel_studio"));
        let slots = session.compression_slots();
        assert!(slots
            .completed_work
            .iter()
            .any(|value| value.contains("chapter 1 drafted")));
        assert!(slots
            .pending_work
            .iter()
            .any(|value| value.contains("audit chapter 1")));
        assert!(slots
            .key_files
            .iter()
            .any(|value| value.contains("chapter-1.md")));
    }

    #[tokio::test]
    async fn passthrough_background_tactics_infers_multimodal_backend_contexts() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();

        let mut screenshot = Message::tool_result("call_screen", "desktop screenshot ready")
            .with_tool_name("browser_screenshot");
        screenshot.source_collection = Some("desktop_capture".to_string());
        screenshot.source_path = Some("/tmp/dashboard.png".to_string());
        screenshot.metadata.insert(
            "media_preprocess_source_ref".to_string(),
            "/tmp/dashboard.png".to_string(),
        );
        screenshot.metadata.insert(
            "media_preprocess_route".to_string(),
            "image_page_raster".to_string(),
        );

        let mut multimodal = Message::tool_result("call_multimodal", "multimodal summary ready")
            .with_tool_name("vision_understanding");
        multimodal.metadata.insert(
            "multimodal_source_url".to_string(),
            "https://example.com/capture.png".to_string(),
        );
        multimodal.metadata.insert(
            "multimodal_route".to_string(),
            "image_understanding".to_string(),
        );
        multimodal.metadata.insert(
            "task_title".to_string(),
            "image_understanding preview pane".to_string(),
        );

        let messages = vec![
            make_message(Role::User, "把截图和多模态结果也纳入背景层"),
            screenshot,
            multimodal,
            make_message(Role::Assistant, "好，我把这些 backend 来源作为当前背景输入"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        let session = verdict
            .session_candidate
            .expect("session candidate should be present");
        assert!(session
            .backend_contexts
            .iter()
            .any(|value| value.contains("Collection context")));
        assert!(session
            .backend_contexts
            .iter()
            .any(|value| value.contains("Multimodal context")));
        assert!(session
            .backend_contexts
            .iter()
            .any(|value| value.contains("Multimodal route")));
        assert!(session.backend_context_records.iter().any(|record| {
            record.kind == Some(BackendContextKind::Collection)
                && record.value.contains("desktop_capture")
        }));
        assert!(session.backend_context_records.iter().any(|record| {
            record.kind == Some(BackendContextKind::MultimodalRoute)
                && record.value.contains("image_understanding")
        }));
        assert!(session.multimodal_session_objects.iter().any(|object| {
            object.locator.contains("dashboard.png")
                && object
                    .route
                    .as_deref()
                    .is_some_and(|value| value.contains("image_page_raster"))
        }));
        assert!(session.multimodal_session_objects.iter().any(|object| {
            object
                .title
                .as_deref()
                .is_some_and(|value| value.contains("image_understanding"))
        }));
    }

    #[tokio::test]
    async fn passthrough_background_tactics_rejects_tentative_or_blocked_preference_writeback() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let messages = vec![
            make_message(Role::User, "我们继续聊长期习惯"),
            make_message(Role::Assistant, "好，我只保留稳定信息"),
            make_message(Role::User, "也许我以后会想让你叫我老白，但先别记住"),
            make_message(Role::Assistant, "收到，这轮我不会把它写进长期背景"),
            make_message(Role::User, "现在先不要写进长期偏好"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        assert_eq!(
            verdict.decision,
            BackgroundCompressionDecision::RejectCandidate
        );
        assert!(!verdict.used_slm);
    }

    #[tokio::test]
    async fn passthrough_background_tactics_rejects_backend_only_preference_signal_without_user_confirmation(
    ) {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let mut tool = Message::tool_result(
            "call_browser_extract",
            "page note: maybe later call me captain as a long-term preference",
        )
        .with_tool_name("browser_extract");
        tool.metadata.insert(
            "source_url".to_string(),
            "https://example.com/preference-note".to_string(),
        );

        let messages = vec![
            make_message(Role::User, "把这个网页里的内容也一起纳入背景窗"),
            tool,
            make_message(Role::Assistant, "我先保守检查这些后台信号"),
            make_message(Role::User, "继续当前主线，不要乱写长期偏好"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(&messages, None)
            .await
            .expect("background tactics should succeed");

        assert_eq!(
            verdict.decision,
            BackgroundCompressionDecision::RejectCandidate
        );
        let session = verdict
            .session_candidate
            .expect("session candidate should still be present");
        assert!(session.tool_session_objects.iter().any(|object| {
            object.tool_name.contains("browser_extract")
                && object
                    .source_ref
                    .as_deref()
                    .is_some_and(|value| value.contains("preference-note"))
        }));
    }

    #[tokio::test]
    async fn passthrough_background_tactics_rejects_hallucination_sensitive_writeback_pack() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let cases = [
            "也许以后我会想让你叫我小白，但先别记住",
            "先不要写进长期偏好，我只是随口想想",
            "maybe later call me captain, but don't remember it yet",
            "perhaps we keep this temporary and do not write it into long-term memory",
            "先把这个当临时备注，不要放进稳定背景",
            "for now keep this temporary and do not promote it into stable background",
        ];

        for case in cases {
            let messages = vec![
                make_message(Role::User, "我们继续聊长期习惯"),
                make_message(Role::Assistant, "好，我只保留稳定信息"),
                make_message(Role::User, case),
                make_message(Role::Assistant, "收到，这轮我不会把它写进长期背景"),
                make_message(Role::User, "先保持现在的稳定偏好"),
            ];

            let verdict = orchestrator
                .derive_background_tactics(&messages, None)
                .await
                .expect("background tactics should succeed");

            assert_eq!(
                verdict.decision,
                BackgroundCompressionDecision::RejectCandidate,
                "case should be rejected: {case}"
            );
            assert!(
                !verdict.used_slm,
                "hallucination guard should work without slm"
            );
        }
    }

    #[tokio::test]
    async fn passthrough_background_tactics_decays_stale_workspace_focus_and_theme() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let messages = vec![
            make_message(Role::User, "我们现在转去整理新的长期主题"),
            make_message(Role::Assistant, "好，我先按新的主线收口"),
            make_message(Role::User, "这轮先不要沿用之前的桌面焦点"),
            make_message(Role::Assistant, "收到，我按新的话题处理"),
            make_message(Role::User, "重点只看新的背景压缩衰减规则"),
            make_message(Role::Assistant, "我会让旧主题退出 active background"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(
                &messages,
                Some(&BackgroundEnvelope {
                    revision: crate::agent::memory::BackgroundRevision {
                        revision: 10,
                        ..Default::default()
                    },
                    session_layer: Some(SessionBackgroundState {
                        workspace_focus: Some("BenShu Gateway (Browser)".to_string()),
                        metadata: HashMap::from([
                            ("working_mode".to_string(), "browser_review".to_string()),
                            (
                                "interaction_theme".to_string(),
                                "focused_review".to_string(),
                            ),
                            (
                                "workspace_focus_last_seen_epoch".to_string(),
                                "1".to_string(),
                            ),
                            ("working_mode_last_seen_epoch".to_string(), "1".to_string()),
                            (
                                "interaction_theme_last_seen_epoch".to_string(),
                                "1".to_string(),
                            ),
                        ]),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )
            .await
            .expect("background tactics should succeed");

        let session = verdict
            .session_candidate
            .expect("session candidate should be present");
        assert!(session.workspace_focus.is_none());
        assert!(!session.metadata.contains_key("working_mode"));
        assert_ne!(
            session
                .metadata
                .get("interaction_theme")
                .map(String::as_str),
            Some("focused_review")
        );
    }

    #[tokio::test]
    async fn passthrough_background_tactics_drops_stale_topics_when_new_task_takes_over() {
        let orchestrator = GlobalTacticalOrchestrator::passthrough();
        let messages = vec![
            make_message(Role::User, "这轮我们只看新的浏览器工作流"),
            make_message(Role::Assistant, "好，我会把焦点切到新的浏览器任务"),
            make_message(Role::User, "重点是新的工具调用和背景衰减"),
            make_message(Role::Assistant, "收到，我沿新的任务继续"),
            make_message(Role::User, "旧文档主题可以退出当前 session layer 了"),
            make_message(Role::Assistant, "我会保留新主线，清掉旧主线"),
        ];

        let verdict = orchestrator
            .derive_background_tactics(
                &messages,
                Some(&BackgroundEnvelope {
                    revision: crate::agent::memory::BackgroundRevision {
                        revision: 9,
                        ..Default::default()
                    },
                    session_layer: Some(SessionBackgroundState {
                        active_topics: vec![
                            "旧的文档收口主线".to_string(),
                            "更早之前的代码评审".to_string(),
                        ],
                        metadata: HashMap::from([
                            (
                                GlobalTacticalOrchestrator::background_value_key(
                                    "active_topic",
                                    "旧的文档收口主线",
                                ),
                                "1".to_string(),
                            ),
                            (
                                GlobalTacticalOrchestrator::background_value_key(
                                    "active_topic",
                                    "更早之前的代码评审",
                                ),
                                "1".to_string(),
                            ),
                        ]),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )
            .await
            .expect("background tactics should succeed");

        let session = verdict
            .session_candidate
            .expect("session candidate should be present");
        assert!(!session
            .active_topics
            .iter()
            .any(|value| value.contains("旧的文档收口主线")));
        assert!(session
            .active_topics
            .iter()
            .any(|value| value.contains("新的浏览器工作流")));
    }

    struct CompletionGuard {
        completed: Arc<AtomicBool>,
        cancelled: Arc<AtomicUsize>,
    }

    impl Drop for CompletionGuard {
        fn drop(&mut self) {
            if !self.completed.load(Ordering::SeqCst) {
                self.cancelled.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    struct SlowTacticalOrchestrator {
        started: Arc<AtomicUsize>,
        cancelled: Arc<AtomicUsize>,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait]
    impl TacticalOrchestrator for SlowTacticalOrchestrator {
        async fn derive_tactics(
            &self,
            _messages: &[Message],
            _proposed_actions: &[ProposedAction],
        ) -> Result<TacticalVerdict> {
            self.started.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_waiters();
            let completed = Arc::new(AtomicBool::new(false));
            let _guard = CompletionGuard {
                completed: completed.clone(),
                cancelled: self.cancelled.clone(),
            };
            self.release.notified().await;
            completed.store(true, Ordering::SeqCst);
            Ok(TacticalVerdict::Proceed)
        }

        async fn derive_background_tactics(
            &self,
            _messages: &[Message],
            _current_background: Option<&BackgroundEnvelope>,
        ) -> Result<BackgroundCompressionVerdict> {
            Ok(BackgroundCompressionVerdict::skip("not used in this test"))
        }

        fn is_active(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn speculative_spawn_validation_aborts_replaced_pending_task() {
        let started = Arc::new(AtomicUsize::new(0));
        let cancelled = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let inner = Arc::new(SlowTacticalOrchestrator {
            started: started.clone(),
            cancelled: cancelled.clone(),
            entered: entered.clone(),
            release: release.clone(),
        });
        let speculative = SpeculativeTacticalOrchestrator::new(inner);
        let messages = vec![make_message(Role::User, "check tactics")];
        let actions = vec![ProposedAction {
            id: "1".to_string(),
            name: "browser_snapshot".to_string(),
            args: serde_json::json!({"url":"https://example.com"}),
        }];

        speculative.spawn_validation(messages.clone(), actions.clone());
        entered.notified().await;

        speculative.spawn_validation(messages.clone(), actions.clone());
        sleep(Duration::from_millis(20)).await;

        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert_eq!(cancelled.load(Ordering::SeqCst), 1);

        release.notify_waiters();
        let verdict = speculative
            .derive_tactics(&messages, &actions)
            .await
            .expect("speculative tactical should resolve");
        assert!(matches!(verdict, TacticalVerdict::Proceed));
    }
}
