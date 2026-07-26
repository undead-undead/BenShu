//! Context Management Module
//!
//! This module provides the `ContextManager` which is responsible for:
//! - Managing conversation history (short-term memory)
//! - Constructing the final prompt/messages for the LLM
//! - Handling token budgeting and windowing
//! - Injecting system prompts and dynamic context (RAG)

use crate::agent::memory::{
    BackgroundEnvelope, PersonaBackgroundLayer, RelationshipBackgroundLayer, SessionBackgroundState,
};
use crate::agent::message::{Message, Role};
use crate::error::Result;
pub use benshu_runtime_policy_core::{
    BackgroundPressureBand, ContextConfig, ContextOccupancyMetrics,
};
use std::collections::HashSet;
use std::path::Path;

/// Trait for injecting dynamic context
#[async_trait::async_trait]
pub trait ContextInjector: Send + Sync {
    /// Generate messages to inject into the context
    async fn inject(&self, history: &[Message]) -> Result<Vec<Message>>;
}

#[async_trait::async_trait]
impl<T: ContextInjector + ?Sized> ContextInjector for std::sync::Arc<T> {
    async fn inject(&self, history: &[Message]) -> Result<Vec<Message>> {
        self.as_ref().inject(history).await
    }
}

/// Manages the context window for an agent
#[derive(Clone)]
pub struct ContextManager {
    config: ContextConfig,
    system_prompt: Option<String>,
    background_envelope: Option<BackgroundEnvelope>,
    injectors: Vec<std::sync::Arc<dyn ContextInjector>>,
    last_context_metrics: std::sync::Arc<parking_lot::RwLock<Option<ContextOccupancyMetrics>>>,
}

#[derive(Default)]
struct SelectedHistorySignals {
    source_paths: HashSet<String>,
    source_urls: HashSet<String>,
    retrieved_from: HashSet<String>,
    tool_names: HashSet<String>,
    media_refs: HashSet<String>,
}

impl SelectedHistorySignals {
    fn from_messages(messages: &[Message]) -> Self {
        let mut signals = Self::default();

        for message in messages {
            if let Some(source_path) = message.source_path.as_deref() {
                Self::insert_trimmed(&mut signals.source_paths, source_path);
            }
            if let Some(source_url) = message.metadata.get("source_url") {
                Self::insert_trimmed(&mut signals.source_urls, source_url);
            }
            if let Some(source_url) = message.metadata.get("multimodal_source_url") {
                Self::insert_trimmed(&mut signals.source_urls, source_url);
            }
            if let Some(retrieved_from) = message.metadata.get("retrieved_from") {
                Self::insert_trimmed(&mut signals.retrieved_from, retrieved_from);
            }
            if let Some(recall_source) = message.metadata.get("recall_source") {
                Self::insert_trimmed(&mut signals.retrieved_from, recall_source);
            }
            if let Some(tool_name) = message.metadata.get("tool_name") {
                Self::insert_trimmed_lower(&mut signals.tool_names, tool_name);
            }
            if let Some(source_ref) = message.metadata.get("media_preprocess_source_ref") {
                Self::insert_trimmed(&mut signals.media_refs, source_ref);
            }
            if let Some(source_ref) = message.metadata.get("multimodal_source_path") {
                Self::insert_trimmed(&mut signals.media_refs, source_ref);
            }
        }

        signals
    }

    fn insert_trimmed(target: &mut HashSet<String>, value: &str) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            target.insert(trimmed.to_string());
        }
    }

    fn insert_trimmed_lower(target: &mut HashSet<String>, value: &str) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            target.insert(trimmed.to_ascii_lowercase());
        }
    }

    fn contains_any_ref(&self, value: &str) -> bool {
        let trimmed = value.trim();
        !trimmed.is_empty()
            && (self.source_paths.contains(trimmed)
                || self.source_urls.contains(trimmed)
                || self.retrieved_from.contains(trimmed)
                || self.media_refs.contains(trimmed)
                || self.tool_names.contains(&trimmed.to_ascii_lowercase()))
    }

    fn matches_backend_context_record(
        &self,
        record: &crate::agent::memory::BackendContextRecord,
    ) -> bool {
        let value = record.value.trim();
        if value.is_empty() {
            return false;
        }

        match record.kind.as_ref() {
            Some(crate::agent::memory::BackendContextKind::Artifact) => {
                self.source_paths.contains(value)
            }
            Some(crate::agent::memory::BackendContextKind::Web) => self.source_urls.contains(value),
            Some(crate::agent::memory::BackendContextKind::MemoryRecall) => {
                self.retrieved_from.contains(value)
            }
            Some(crate::agent::memory::BackendContextKind::Multimodal) => {
                self.media_refs.contains(value)
                    || self.source_paths.contains(value)
                    || self.source_urls.contains(value)
            }
            Some(crate::agent::memory::BackendContextKind::ToolResult) => {
                self.tool_names.contains(&value.to_ascii_lowercase())
            }
            _ => self.contains_any_ref(value),
        }
    }

    fn matches_retrieved_memory_object(
        &self,
        object: &crate::agent::memory::RetrievedMemoryObject,
    ) -> bool {
        self.retrieved_from.contains(object.recall_source.trim())
    }

    fn matches_web_session_object(&self, object: &crate::agent::memory::WebSessionObject) -> bool {
        self.source_urls.contains(object.url.trim())
    }

    fn matches_artifact_session_object(
        &self,
        object: &crate::agent::memory::ArtifactSessionObject,
    ) -> bool {
        self.source_paths.contains(object.path.trim())
    }

    fn matches_tool_session_object(
        &self,
        object: &crate::agent::memory::ToolSessionObject,
    ) -> bool {
        let tool_match = self
            .tool_names
            .contains(&object.tool_name.trim().to_ascii_lowercase());
        let source_match = object
            .source_ref
            .as_deref()
            .is_some_and(|source_ref| self.contains_any_ref(source_ref));
        tool_match || source_match
    }

    fn matches_multimodal_session_object(
        &self,
        object: &crate::agent::memory::MultimodalSessionObject,
    ) -> bool {
        self.media_refs.contains(object.locator.trim())
            || self.source_paths.contains(object.locator.trim())
            || object
                .source_url
                .as_deref()
                .is_some_and(|source_url| self.source_urls.contains(source_url.trim()))
    }
}

impl ContextManager {
    const LOCAL_RECENT_HISTORY_MESSAGES: usize = 12;
    const LOCAL_PROVIDER_TOKEN_INFLATION_NUMERATOR: usize = 9;
    const LOCAL_PROVIDER_TOKEN_INFLATION_DENOMINATOR: usize = 4;
    const CONTEXT_FIT_SAFETY_MARGIN_TOKENS: usize = 1000;

    fn normalize_overlap_text(text: &str) -> String {
        let mut normalized = String::with_capacity(text.len());
        let mut last_was_space = false;

        for ch in text.chars() {
            if ch.is_alphanumeric() {
                normalized.extend(ch.to_lowercase());
                last_was_space = false;
            } else if ch.is_whitespace() && !last_was_space {
                normalized.push(' ');
                last_was_space = true;
            }
        }

        normalized.trim().to_string()
    }

    fn recent_history_overlap_haystack(recent_history: &[Message]) -> String {
        recent_history
            .iter()
            .map(Message::text)
            .map(|text| Self::normalize_overlap_text(&text))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn history_semantically_mentions(history_haystack: &str, candidate: &str) -> bool {
        let candidate = Self::normalize_overlap_text(candidate);
        candidate.len() >= 10 && history_haystack.contains(&candidate)
    }

    /// Create a new ContextManager
    pub fn new(config: ContextConfig) -> Self {
        Self {
            config,
            system_prompt: None,
            background_envelope: None,
            injectors: Vec::new(),
            last_context_metrics: std::sync::Arc::new(parking_lot::RwLock::new(None)),
        }
    }

    /// Set the system prompt
    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    /// Set the persisted background envelope used to preserve persona, relationship,
    /// and session continuity across long-running conversations.
    pub fn set_background_envelope(&mut self, envelope: BackgroundEnvelope) {
        self.background_envelope = Some(envelope);
    }

    /// Clear any previously injected background envelope.
    pub fn clear_background_envelope(&mut self) {
        self.background_envelope = None;
    }

    /// Add a context injector
    pub fn add_injector(&mut self, injector: std::sync::Arc<dyn ContextInjector>) {
        self.injectors.push(injector);
    }

    pub fn latest_context_metrics(&self) -> Option<ContextOccupancyMetrics> {
        self.last_context_metrics.read().clone()
    }

    fn update_context_metrics(&self, metrics: ContextOccupancyMetrics) {
        tracing::debug!(
            max_window_tokens = metrics.max_window_tokens,
            history_budget_tokens = metrics.history_budget_tokens,
            static_prefix_tokens = metrics.static_prefix_tokens,
            provisional_background_tokens = metrics.provisional_background_tokens,
            effective_background_tokens = metrics.effective_background_tokens,
            dynamic_injection_tokens = metrics.dynamic_injection_tokens,
            selected_history_tokens = metrics.selected_history_tokens,
            pruned_history_tokens = metrics.pruned_history_tokens,
            estimated_final_prompt_tokens = metrics.estimated_final_prompt_tokens,
            background_occupancy_ratio = metrics.background_occupancy_ratio,
            prompt_occupancy_ratio = metrics.prompt_occupancy_ratio,
            pressure_band = metrics.pressure_band.as_str(),
            selected_history_messages = metrics.selected_history_messages,
            pruned_history_messages = metrics.pruned_history_messages,
            local_provider_mode = metrics.local_provider_mode,
            "Context occupancy metrics updated"
        );
        *self.last_context_metrics.write() = Some(metrics);
    }

    fn cap_vec_len_pressure<T>(items: &mut Vec<T>, max_items: usize) {
        if items.len() > max_items {
            let drain_count = items.len() - max_items;
            items.drain(0..drain_count);
        }
    }

    pub fn pressure_compact_envelope(
        envelope: &mut BackgroundEnvelope,
        pressure_band: BackgroundPressureBand,
    ) {
        match pressure_band {
            BackgroundPressureBand::Normal => {}
            BackgroundPressureBand::High => {
                envelope.recent_window_summary = None;
                if let Some(session_layer) = envelope.session_layer.as_mut() {
                    session_layer.summary = None;
                    Self::cap_vec_len_pressure(&mut session_layer.backend_contexts, 4);
                    Self::cap_vec_len_pressure(&mut session_layer.backend_context_records, 4);
                    Self::cap_vec_len_pressure(&mut session_layer.retrieved_memory_objects, 4);
                    Self::cap_vec_len_pressure(&mut session_layer.web_session_objects, 4);
                    Self::cap_vec_len_pressure(&mut session_layer.artifact_session_objects, 4);
                    Self::cap_vec_len_pressure(&mut session_layer.task_session_objects, 4);
                    Self::cap_vec_len_pressure(&mut session_layer.tool_session_objects, 4);
                    Self::cap_vec_len_pressure(&mut session_layer.multimodal_session_objects, 4);
                    Self::cap_vec_len_pressure(&mut session_layer.active_topics, 3);
                    Self::cap_vec_len_pressure(&mut session_layer.open_loops, 3);
                    Self::cap_vec_len_pressure(&mut session_layer.ongoing_goals, 3);
                    Self::cap_vec_len_pressure(&mut session_layer.pending_followups, 3);
                }
                if envelope.source_refs.len() > 6 {
                    let drain_count = envelope.source_refs.len() - 6;
                    envelope.source_refs.drain(0..drain_count);
                }
            }
            BackgroundPressureBand::Critical => {
                envelope.recent_window_summary = None;
                if let Some(session_layer) = envelope.session_layer.as_mut() {
                    session_layer.summary = None;
                    session_layer.recent_emotional_state = None;
                    Self::cap_vec_len_pressure(&mut session_layer.backend_contexts, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.backend_context_records, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.retrieved_memory_objects, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.web_session_objects, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.artifact_session_objects, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.task_session_objects, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.tool_session_objects, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.multimodal_session_objects, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.active_topics, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.open_loops, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.ongoing_goals, 2);
                    Self::cap_vec_len_pressure(&mut session_layer.pending_followups, 2);
                }
                if envelope.source_refs.len() > 4 {
                    let drain_count = envelope.source_refs.len() - 4;
                    envelope.source_refs.drain(0..drain_count);
                }
            }
        }
    }

    fn filtered_background_envelope(
        &self,
        recent_history: &[Message],
    ) -> Option<BackgroundEnvelope> {
        let Some(envelope) = self.background_envelope.as_ref() else {
            return None;
        };
        if envelope.is_empty() {
            return None;
        }

        let mut filtered = envelope.clone();
        filtered.apply_budget_caps();
        let signals = SelectedHistorySignals::from_messages(recent_history);

        if !recent_history.is_empty() {
            filtered.recent_window_summary = None;
            if let Some(session_layer) = filtered.session_layer.as_mut() {
                session_layer.summary = None;
            }
        }

        if let Some(session_layer) = filtered.session_layer.as_mut() {
            session_layer
                .backend_context_records
                .retain(|record| !signals.matches_backend_context_record(record));
            session_layer
                .retrieved_memory_objects
                .retain(|object| !signals.matches_retrieved_memory_object(object));
            session_layer
                .web_session_objects
                .retain(|object| !signals.matches_web_session_object(object));
            session_layer
                .artifact_session_objects
                .retain(|object| !signals.matches_artifact_session_object(object));
            session_layer
                .tool_session_objects
                .retain(|object| !signals.matches_tool_session_object(object));
            session_layer
                .multimodal_session_objects
                .retain(|object| !signals.matches_multimodal_session_object(object));
            session_layer.sync_backend_context_storage();
            Self::revalidate_session_workspace_refs(session_layer);

            let history_haystack = Self::recent_history_overlap_haystack(recent_history);
            if !history_haystack.is_empty() {
                session_layer
                    .active_topics
                    .retain(|topic| !Self::history_semantically_mentions(&history_haystack, topic));
                session_layer
                    .open_loops
                    .retain(|item| !Self::history_semantically_mentions(&history_haystack, item));
                session_layer
                    .ongoing_goals
                    .retain(|goal| !Self::history_semantically_mentions(&history_haystack, goal));
                session_layer.pending_followups.retain(|followup| {
                    !Self::history_semantically_mentions(&history_haystack, followup)
                });

                if session_layer.workspace_focus.as_ref().is_some_and(|value| {
                    Self::history_semantically_mentions(&history_haystack, value)
                }) {
                    session_layer.workspace_focus = None;
                }

                if session_layer
                    .recent_emotional_state
                    .as_ref()
                    .is_some_and(|value| {
                        Self::history_semantically_mentions(&history_haystack, value)
                    })
                {
                    session_layer.recent_emotional_state = None;
                }
            }
        }

        if filtered
            .session_layer
            .as_ref()
            .is_some_and(SessionBackgroundState::is_empty)
        {
            filtered.session_layer = None;
        }

        if filtered.is_empty() {
            return None;
        }

        Some(filtered)
    }

    fn path_string_requires_local_revalidation(value: &str) -> bool {
        let trimmed = value.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with("memory://")
            || trimmed.starts_with("artifact://")
            || trimmed.starts_with("knowledge://")
        {
            return false;
        }

        trimmed.starts_with('/')
            || trimmed.starts_with("./")
            || trimmed.starts_with("../")
            || trimmed.contains('\\')
            || trimmed.as_bytes().get(1).is_some_and(|byte| *byte == b':')
    }

    fn local_path_still_exists(value: &str) -> bool {
        if !Self::path_string_requires_local_revalidation(value) {
            return true;
        }
        Path::new(value.trim()).exists()
    }

    fn revalidate_session_workspace_refs(session_layer: &mut SessionBackgroundState) {
        let before_artifacts = session_layer.artifact_session_objects.len();
        session_layer
            .artifact_session_objects
            .retain(|object| Self::local_path_still_exists(&object.path));

        let mut slots = session_layer.compression_slots();
        let before_key_files = slots.key_files.len();
        let mut missing_key_files = Vec::new();
        slots.key_files.retain(|path| {
            let exists = Self::local_path_still_exists(path);
            if !exists {
                missing_key_files.push(path.clone());
            }
            exists
        });
        if !missing_key_files.is_empty() {
            slots.verification_needs.push(format!(
                "Re-check missing key files before use: {}",
                missing_key_files.join("; ")
            ));
        }
        session_layer.set_compression_slots(slots);

        let removed = before_artifacts
            .saturating_sub(session_layer.artifact_session_objects.len())
            .saturating_add(
                before_key_files.saturating_sub(session_layer.compression_slots().key_files.len()),
            );
        session_layer.metadata.insert(
            "background_workspace_refs_revalidated".to_string(),
            "true".to_string(),
        );
        session_layer.metadata.insert(
            "background_workspace_refs_removed_missing".to_string(),
            removed.to_string(),
        );
    }

    fn build_background_messages_for_history(&self, recent_history: &[Message]) -> Vec<Message> {
        self.build_background_messages_for_history_with_pressure(
            recent_history,
            BackgroundPressureBand::Normal,
        )
    }

    fn build_background_messages_for_history_with_pressure(
        &self,
        recent_history: &[Message],
        pressure_band: BackgroundPressureBand,
    ) -> Vec<Message> {
        let Some(mut envelope) = self.filtered_background_envelope(recent_history) else {
            return Vec::new();
        };

        Self::pressure_compact_envelope(&mut envelope, pressure_band);
        envelope.apply_budget_caps();

        let mut background = Vec::new();

        if let Some(layer) = envelope.persona_layer.as_ref() {
            if !layer.is_empty() {
                background.push(Message::system(Self::format_persona_layer(layer)));
            }
        }

        if let Some(layer) = envelope.relationship_layer.as_ref() {
            if !layer.is_empty() {
                background.push(Message::system(Self::format_relationship_layer(layer)));
            }
        }

        if let Some(layer) = envelope.session_layer.as_ref() {
            if !layer.is_empty() {
                background.push(Message::system(Self::format_session_layer(layer)));
            }
        }

        if let Some(summary) = envelope.recent_window_summary.as_ref() {
            if !summary.is_empty() {
                let mut text = String::from("### Recent Window Summary\n");
                text.push_str(&summary.summary);
                background.push(Message::system(text));
            }
        }

        background
    }

    fn message_token_cost_for_provider(message: &Message, is_local: bool) -> usize {
        Self::estimate_tokens_for_provider(std::slice::from_ref(message), is_local).max(1)
    }

    fn trim_message_to_token_budget(
        mut message: Message,
        token_budget: usize,
        is_local: bool,
    ) -> Message {
        let chars_per_token = if is_local { 3 } else { 4 };
        let char_limit = token_budget
            .saturating_mul(chars_per_token)
            .clamp(128, 16_000);
        message.soft_trim(char_limit);
        message
    }

    fn fit_messages_to_prompt_budget(
        &self,
        messages: Vec<Message>,
        prompt_budget: usize,
        is_local: bool,
    ) -> (Vec<Message>, usize) {
        if messages.is_empty()
            || Self::estimate_tokens_for_provider(&messages, is_local) <= prompt_budget
        {
            return (messages, 0);
        }

        let original_len = messages.len();
        let first_system = messages
            .first()
            .filter(|message| matches!(message.role, Role::System))
            .cloned();
        let latest_user_index = messages
            .iter()
            .rposition(|message| matches!(message.role, Role::User));

        let static_budget = if first_system.is_some() {
            let quarter = prompt_budget.saturating_div(4).max(1);
            if prompt_budget < 128 {
                quarter.min(prompt_budget.max(1))
            } else {
                quarter.clamp(128, prompt_budget)
            }
        } else {
            0
        };
        let mut fitted = Vec::new();
        let mut used = 0usize;

        if let Some(system) = first_system {
            let system = if Self::message_token_cost_for_provider(&system, is_local) > static_budget
            {
                Self::trim_message_to_token_budget(system, static_budget, is_local)
            } else {
                system
            };
            used = used.saturating_add(Self::message_token_cost_for_provider(&system, is_local));
            fitted.push(system);
        }

        let mut tail = Vec::new();
        let tail_budget = prompt_budget.saturating_sub(used).max(1);
        let mut tail_used = 0usize;

        for (index, message) in messages.iter().enumerate().rev() {
            if index == 0
                && fitted
                    .first()
                    .is_some_and(|m| matches!(m.role, Role::System))
            {
                continue;
            }

            let must_keep = Some(index) == latest_user_index || tail.is_empty();
            let mut candidate = message.clone();
            let mut cost = Self::message_token_cost_for_provider(&candidate, is_local);

            if tail_used.saturating_add(cost) > tail_budget {
                if !must_keep {
                    continue;
                }
                let remaining = tail_budget.saturating_sub(tail_used).max(1);
                candidate = Self::trim_message_to_token_budget(candidate, remaining, is_local);
                cost = Self::message_token_cost_for_provider(&candidate, is_local);
            }

            if tail_used.saturating_add(cost) <= tail_budget || must_keep {
                tail_used = tail_used.saturating_add(cost);
                tail.push(candidate);
            }
        }

        tail.reverse();
        fitted.extend(tail);

        while Self::estimate_tokens_for_provider(&fitted, is_local) > prompt_budget
            && fitted.len() > 1
        {
            let Some(remove_index) = fitted
                .iter()
                .position(|message| !matches!(message.role, Role::System | Role::User))
            else {
                break;
            };
            fitted.remove(remove_index);
        }

        if Self::estimate_tokens_for_provider(&fitted, is_local) > prompt_budget {
            let per_message_budget = prompt_budget
                .checked_div(fitted.len().max(1))
                .unwrap_or(prompt_budget)
                .max(1);
            fitted = fitted
                .into_iter()
                .map(|message| {
                    Self::trim_message_to_token_budget(message, per_message_budget, is_local)
                })
                .collect();
        }

        let dropped = original_len.saturating_sub(fitted.len());
        (fitted, dropped)
    }

    fn final_context_prompt_budget(&self) -> usize {
        let safety_margin =
            Self::CONTEXT_FIT_SAFETY_MARGIN_TOKENS.min(self.config.max_tokens.saturating_div(8));
        self.config
            .max_tokens
            .saturating_sub(self.config.response_reserve)
            .saturating_sub(safety_margin)
            .max(1)
    }

    fn format_persona_layer(layer: &PersonaBackgroundLayer) -> String {
        let mut lines = vec!["### Core Persona Layer".to_string()];

        if let Some(identity_summary) = &layer.identity_summary {
            lines.push(format!("- Identity: {}", identity_summary));
        }
        if let Some(speaking_style) = &layer.speaking_style {
            lines.push(format!("- Speaking Style: {}", speaking_style));
        }
        if let Some(relationship_frame) = &layer.relationship_frame {
            lines.push(format!("- Relationship Frame: {}", relationship_frame));
        }
        if !layer.safety_notes.is_empty() {
            lines.push("- Safety Notes:".to_string());
            for note in &layer.safety_notes {
                lines.push(format!("  - {}", note));
            }
        }

        lines.join("\n")
    }

    fn format_relationship_layer(layer: &RelationshipBackgroundLayer) -> String {
        let mut lines = vec!["### Relationship Layer".to_string()];

        if let Some(user_profile_summary) = &layer.user_profile_summary {
            lines.push(format!("- User Profile: {}", user_profile_summary));
        }
        if let Some(relationship_summary) = &layer.relationship_summary {
            lines.push(format!("- Relationship Summary: {}", relationship_summary));
        }
        if !layer.user_preferences.is_empty() {
            lines.push(format!(
                "- Preferences: {}",
                layer.user_preferences.join("; ")
            ));
        }
        if !layer.long_term_topics.is_empty() {
            lines.push(format!(
                "- Long-term Topics: {}",
                layer.long_term_topics.join("; ")
            ));
        }
        if !layer.emotional_markers.is_empty() {
            lines.push(format!(
                "- Emotional Markers: {}",
                layer.emotional_markers.join("; ")
            ));
        }

        lines.join("\n")
    }

    fn format_session_layer(layer: &SessionBackgroundState) -> String {
        let mut lines = vec!["### Ongoing Session Layer".to_string()];

        if !layer.active_topics.is_empty() {
            lines.push(format!(
                "- Active Topics: {}",
                layer.active_topics.join("; ")
            ));
        }
        let compression_slots = layer.compression_slots();
        if !compression_slots.project_facts.is_empty() {
            lines.push(format!(
                "- Project Facts: {}",
                compression_slots.project_facts.join("; ")
            ));
        }
        if let Some(current_task) = compression_slots.current_task.as_deref() {
            lines.push(format!("- Current Task: {}", current_task));
        }
        if !compression_slots.completed_work.is_empty() {
            lines.push(format!(
                "- Completed Work: {}",
                compression_slots.completed_work.join("; ")
            ));
        }
        if !compression_slots.pending_work.is_empty() {
            lines.push(format!(
                "- Pending Work: {}",
                compression_slots.pending_work.join("; ")
            ));
        }
        if !compression_slots.key_files.is_empty() {
            lines.push(format!(
                "- Key Files: {}",
                compression_slots.key_files.join("; ")
            ));
        }
        if !compression_slots.test_results.is_empty() {
            lines.push(format!(
                "- Test Results: {}",
                compression_slots.test_results.join("; ")
            ));
        }
        if !compression_slots.risks.is_empty() {
            lines.push(format!("- Risks: {}", compression_slots.risks.join("; ")));
        }
        if !compression_slots.verification_needs.is_empty() {
            lines.push(format!(
                "- Verification Needs: {}",
                compression_slots.verification_needs.join("; ")
            ));
        }
        if !compression_slots.is_empty() {
            lines.push(
                "- Compressed Claim Rule: verify file/repo/runtime/web facts with filesystem, git, gateway/panel, or tool evidence before treating them as current."
                    .to_string(),
            );
        }
        if !layer.backend_contexts.is_empty() && layer.backend_context_records.is_empty() {
            lines.push(format!(
                "- Backend Contexts: {}",
                layer.backend_contexts.join("; ")
            ));
        }
        let backend_context_records = layer.canonical_backend_context_records();
        if !backend_context_records.is_empty() {
            let rendered = backend_context_records
                .iter()
                .map(|record| record.render())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            if !rendered.is_empty() {
                lines.push(format!(
                    "- Backend Context Records: {}",
                    rendered.join("; ")
                ));
            }
        }
        if !layer.retrieved_memory_objects.is_empty() {
            let rendered = layer
                .retrieved_memory_objects
                .iter()
                .map(|object| object.render())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            if !rendered.is_empty() {
                lines.push(format!(
                    "- Retrieved Memory Objects: {}",
                    rendered.join("; ")
                ));
            }
        }
        if !layer.web_session_objects.is_empty() {
            let rendered = layer
                .web_session_objects
                .iter()
                .map(|object| object.render())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            if !rendered.is_empty() {
                lines.push(format!("- Web Session Objects: {}", rendered.join("; ")));
            }
        }
        if !layer.artifact_session_objects.is_empty() {
            let rendered = layer
                .artifact_session_objects
                .iter()
                .map(|object| object.render())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            if !rendered.is_empty() {
                lines.push(format!(
                    "- Artifact Session Objects: {}",
                    rendered.join("; ")
                ));
            }
        }
        if !layer.task_session_objects.is_empty() {
            let rendered = layer
                .task_session_objects
                .iter()
                .map(|object| object.render())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            if !rendered.is_empty() {
                lines.push(format!("- Task Session Objects: {}", rendered.join("; ")));
            }
        }
        if !layer.tool_session_objects.is_empty() {
            let rendered = layer
                .tool_session_objects
                .iter()
                .map(|object| object.render())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            if !rendered.is_empty() {
                lines.push(format!("- Tool Session Objects: {}", rendered.join("; ")));
            }
        }
        if !layer.multimodal_session_objects.is_empty() {
            let rendered = layer
                .multimodal_session_objects
                .iter()
                .map(|object| object.render())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            if !rendered.is_empty() {
                lines.push(format!(
                    "- Multimodal Session Objects: {}",
                    rendered.join("; ")
                ));
            }
        }
        if !layer.open_loops.is_empty() {
            lines.push(format!("- Open Loops: {}", layer.open_loops.join("; ")));
        }
        if let Some(recent_emotional_state) = &layer.recent_emotional_state {
            lines.push(format!(
                "- Recent Emotional State: {}",
                recent_emotional_state
            ));
        }
        if !layer.ongoing_goals.is_empty() {
            lines.push(format!(
                "- Ongoing Goals: {}",
                layer.ongoing_goals.join("; ")
            ));
        }
        if let Some(workspace_focus) = &layer.workspace_focus {
            lines.push(format!("- Workspace Focus: {}", workspace_focus));
        }
        if let Some(working_mode) = layer.metadata.get("working_mode") {
            lines.push(format!("- Working Mode: {}", working_mode));
        }
        if let Some(interaction_theme) = layer.metadata.get("interaction_theme") {
            lines.push(format!("- Interaction Theme: {}", interaction_theme));
        }
        if !layer.pending_followups.is_empty() {
            lines.push(format!(
                "- Pending Follow-ups: {}",
                layer.pending_followups.join("; ")
            ));
        }
        if let Some(summary) = &layer.summary {
            lines.push(format!("- Session Summary: {}", summary));
        }

        lines.join("\n")
    }

    /// Construct the final list of messages to send to the provider
    ///
    /// This method applies:
    /// 1. System prompt injection (Protected)
    /// 2. Dynamic Context Injection (RAG, etc.) (Protected)
    /// 3. Token budgeting using tiktoken (Soft Pruning)
    /// 4. Message windowing (based on strategy)
    /// Construct the final list of messages to send to the provider
    ///
    /// This method applies:
    /// 1. System prompt injection (Protected Prefix)
    /// 2. Dynamic Context Injection (Protected Prefix)
    /// 3. Progressive Pruning (Soft Trim & Hard Clear)
    /// 4. Observation Log Anchoring (Tail-end summary)
    pub async fn build_context(
        &self,
        history: &[Message],
        strategy: &crate::agent::attempt::Strategy,
        is_local: bool,
    ) -> Result<Vec<Message>> {
        // 1. Initialize Tokenizer
        let bpe = tiktoken_rs::cl100k_base().map_err(|e| {
            crate::error::Error::Internal(format!("Failed to load tokenizer: {}", e))
        })?;

        // --- SECTION A: Protected Static Prefix (P1) ---
        // Keep the static prefix stable for predictable prompt governance.
        let mut static_prefix = Vec::new();
        let provisional_background_prefix = self.build_background_messages_for_history(&[]);
        let mut dynamic_injections = Vec::new();

        let strategy_cfg = strategy.config();

        if let Some(prompt) = &self.system_prompt {
            let mut final_prompt = prompt.clone();
            if strategy_cfg.add_concise_directive {
                final_prompt.push_str(
                    "\n\nIMPERATIVE: Be extremely concise. Use minimal tokens to achieve the task.",
                );
            }
            static_prefix.push(Message::system(final_prompt));
        }

        // Feature: Fallback Strategy - Break early (Survival mode)
        let static_prefix_tokens = Self::estimate_tokens_for_provider(&static_prefix, is_local);
        let provisional_background_tokens =
            Self::estimate_tokens_for_provider(&provisional_background_prefix, is_local);

        if matches!(strategy, crate::agent::attempt::Strategy::Fallback) {
            let mut final_messages = static_prefix;
            let effective_background_messages = self.build_background_messages_for_history(history);
            final_messages.extend(effective_background_messages.clone());
            if let Some(last_user) = history
                .iter()
                .rev()
                .find(|message| matches!(message.role, Role::User))
            {
                final_messages.push(last_user.clone());
            }
            if let Some(last) = history.last() {
                let already_included = final_messages.last().is_some_and(|message| {
                    message.role == last.role && message.text() == last.text()
                });
                if !already_included {
                    final_messages.push(last.clone());
                }
            }
            let prompt_budget = self.final_context_prompt_budget();
            let (final_messages, final_fit_dropped_messages) =
                self.fit_messages_to_prompt_budget(final_messages, prompt_budget, is_local);
            let effective_background_tokens =
                Self::estimate_tokens_for_provider(&effective_background_messages, is_local);
            let selected_history_tokens = history
                .last()
                .map(|last| {
                    Self::estimate_tokens_for_provider(std::slice::from_ref(last), is_local)
                })
                .unwrap_or(0);
            let estimated_final_prompt_tokens =
                Self::estimate_tokens_for_provider(&final_messages, is_local);
            self.update_context_metrics(ContextOccupancyMetrics {
                max_window_tokens: self.config.max_tokens,
                reserved_response_tokens: 0,
                safety_margin_tokens: 0,
                history_budget_tokens: selected_history_tokens,
                static_prefix_tokens,
                provisional_background_tokens,
                effective_background_tokens,
                dynamic_injection_tokens: 0,
                selected_history_tokens,
                pruned_history_tokens: 0,
                estimated_prefix_tokens: static_prefix_tokens + provisional_background_tokens,
                estimated_final_prompt_tokens,
                effective_max_history_messages: 1,
                selected_history_messages: usize::from(history.last().is_some()),
                pruned_history_messages: history
                    .len()
                    .saturating_sub(usize::from(history.last().is_some())),
                dynamic_injection_messages: 0,
                background_message_count: effective_background_messages.len(),
                background_occupancy_ratio: if estimated_final_prompt_tokens == 0 {
                    0.0
                } else {
                    effective_background_tokens as f32 / estimated_final_prompt_tokens as f32
                },
                prompt_occupancy_ratio: if self.config.max_tokens == 0 {
                    0.0
                } else {
                    estimated_final_prompt_tokens as f32 / self.config.max_tokens as f32
                },
                pressure_band: BackgroundPressureBand::from_prompt_occupancy_ratio(
                    if self.config.max_tokens == 0 {
                        0.0
                    } else {
                        estimated_final_prompt_tokens as f32 / self.config.max_tokens as f32
                    },
                ),
                local_provider_mode: is_local,
            });
            if final_fit_dropped_messages > 0 {
                tracing::info!(
                    dropped_messages = final_fit_dropped_messages,
                    prompt_budget_tokens = prompt_budget,
                    "ContextManager: final context fit trimmed fallback prompt to runtime budget"
                );
            }
            return Ok(final_messages);
        }

        // Run Injectors (Static RAG, Skills indices)
        for injector in &self.injectors {
            match injector.inject(history).await {
                Ok(msgs) => dynamic_injections.extend(msgs),
                Err(e) => tracing::warn!("Context injector failed: {}", e),
            }
        }

        // P9: Context Metrics (Prefix Stability)
        // Calculate hash of static prefix to diagnose prompt-surface drift.
        let prefix_text = static_prefix
            .iter()
            .map(|m| m.content.as_text())
            .collect::<String>();
        let prefix_hash = fxhash::hash64(&prefix_text);
        tracing::debug!(hash = %prefix_hash, count = %static_prefix.len(), "Context Static Prefix Hash (P1)");

        if is_local {
            tracing::info!(
                local_recent_history_messages = Self::LOCAL_RECENT_HISTORY_MESSAGES,
                "ContextManager: Local Provider detected. Using background-first + recent-history window."
            );
        }

        // --- SECTION B: Budget Calculation ---
        const SAFETY_MARGIN: usize = 1000;
        let reserved_response = self.config.response_reserve;
        let max_window = self.config.max_tokens;

        let dynamic_injection_tokens =
            Self::estimate_tokens_for_provider(&dynamic_injections, is_local);
        let prefix_tokens =
            static_prefix_tokens + provisional_background_tokens + dynamic_injection_tokens;
        let total_reserved = reserved_response + SAFETY_MARGIN + prefix_tokens;
        let history_budget = max_window.saturating_sub(total_reserved);

        // --- SECTION C: Dynamic History Selection & Pruning (P2, P4) ---
        let mut selected_history = Vec::new();
        let mut history_usage = 0;
        let mut pruned_messages = Vec::new();

        // Stage 1: Determination of window size
        let effective_max_history = (self.config.max_history_messages as f32
            * strategy_cfg.max_history_ratio)
            .ceil() as usize;
        let effective_max_history = effective_max_history.max(1); // Ensure at least 1 for User message if not Fallback (ratio 0.0)

        let effective_max_history = if matches!(strategy, crate::agent::attempt::Strategy::Fallback)
        {
            1
        } else {
            effective_max_history
        };

        let effective_max_history = if is_local {
            effective_max_history.min(Self::LOCAL_RECENT_HISTORY_MESSAGES)
        } else {
            effective_max_history
        };

        let history_slice = if history.len() > effective_max_history {
            let (pruned, selected) = history.split_at(history.len() - effective_max_history);
            pruned_messages.extend(pruned.iter().cloned());
            selected
        } else {
            history
        };

        // Stage 2: Selection with Defensive Trimming (Soft Trim)
        // Iterate REVERSE (Latest first)
        for mut msg in history_slice.iter().rev().cloned() {
            let mut tokens = bpe.encode_with_special_tokens(&msg.content.as_text()).len();

            // P4: Stage 1 Pruning (Soft Trim) - If a single message is huge, trim it immediately
            // This prevents one giant tool output from flushing the whole history.
            if tokens > 2000 {
                msg.soft_trim(4000); // Keep approx 1000 tokens head/tail
                tokens = bpe.encode_with_special_tokens(&msg.content.as_text()).len();
            }

            let cost = Self::adjust_estimated_tokens(tokens + 4, is_local);

            if history_usage + cost <= history_budget {
                history_usage += cost;
                selected_history.push(msg);
            } else {
                // P4: Stage 2 Pruning (Hard Clear) - If selected_history already has enough,
                // we treat the rest as pruned.
                pruned_messages.push(msg);
            }
        }

        selected_history.reverse();
        if !selected_history
            .iter()
            .any(|message| matches!(message.role, Role::User))
        {
            if let Some(mut latest_user) = history
                .iter()
                .rev()
                .find(|message| matches!(message.role, Role::User))
                .cloned()
            {
                latest_user.soft_trim(2000);
                selected_history.push(latest_user);
            }
        }

        // --- SECTION D: Observation Log Anchoring (P5) ---
        // Instead of putting the log at the START (which breaks KV cache every turn),
        // we put it at the start of the SELECTED HISTORY or just before the latest messages.
        // Here we've opted for: [Static Prefix] -> [Observation Log] -> [History]
        // But wait, to keep prefix stable, it's better to append the log as a bridge.

        let selected_history_tokens =
            Self::estimate_tokens_for_provider(&selected_history, is_local);
        let pruned_history_tokens = Self::estimate_tokens_for_provider(&pruned_messages, is_local);
        let selected_history_messages = selected_history.len();
        let pruned_history_messages = pruned_messages.len();
        let dynamic_injection_messages = dynamic_injections.len();

        let assemble_final_messages =
            |pressure_band: BackgroundPressureBand| -> (Vec<Message>, Vec<Message>) {
                let effective_background_messages = self
                    .build_background_messages_for_history_with_pressure(
                        &selected_history,
                        pressure_band,
                    );
                let mut final_messages = static_prefix.clone();
                final_messages.extend(effective_background_messages.clone());
                final_messages.extend(dynamic_injections.clone());
                (final_messages, effective_background_messages)
            };

        let (mut final_messages, mut effective_background_messages) =
            assemble_final_messages(BackgroundPressureBand::Normal);

        let enable_smart_pruning = self.config.smart_pruning || strategy_cfg.enable_smart_pruning;

        let should_emit_pruned_history_summary = enable_smart_pruning
            && !pruned_messages.is_empty()
            && !(is_local && self.background_envelope.is_some());

        if should_emit_pruned_history_summary {
            let mut log = String::from("### Historical Context Summary (Pruned)\n");
            log.push_str("To save context space, early history was summarized below:\n");

            // Sort pruned messages back to chronological for summary
            pruned_messages.reverse();
            for msg in &pruned_messages {
                match msg.role {
                    crate::agent::message::Role::Assistant => {
                        let text = msg.content.as_text();
                        let snippet = if text.chars().count() > 64 {
                            let truncated: String = text.chars().take(60).collect();
                            format!("{}...", truncated.replace('\n', " "))
                        } else {
                            text.replace('\n', " ")
                        };
                        log.push_str(&format!("- Assistant decision: {}\n", snippet));
                    }
                    crate::agent::message::Role::Tool => {
                        let name = msg.name.as_deref().unwrap_or("unknown_tool");
                        log.push_str(&format!("- Result from: {}\n", name));
                    }
                    crate::agent::message::Role::User => {
                        let text = msg.content.as_text();
                        let user_snippet = if text.chars().count() > 40 {
                            let truncated: String = text.chars().take(40).collect();
                            format!("{}...", truncated)
                        } else {
                            text
                        };
                        log.push_str(&format!("- User requested: {}\n", user_snippet));
                    }
                    _ => {}
                }
            }
            final_messages.push(Message::system(log));
        }

        // Finally add the selected recent history
        final_messages.extend(selected_history.clone());

        let mut estimated_final_prompt_tokens =
            Self::estimate_tokens_for_provider(&final_messages, is_local);
        let mut prompt_occupancy_ratio = if max_window == 0 {
            0.0
        } else {
            estimated_final_prompt_tokens as f32 / max_window as f32
        };
        let mut pressure_band =
            BackgroundPressureBand::from_prompt_occupancy_ratio(prompt_occupancy_ratio);

        if pressure_band != BackgroundPressureBand::Normal {
            let (mut pressured_final_messages, pressured_background_messages) =
                assemble_final_messages(pressure_band);
            if should_emit_pruned_history_summary {
                let mut log = String::from("### Historical Context Summary (Pruned)\n");
                log.push_str("To save context space, early history was summarized below:\n");

                for msg in &pruned_messages {
                    match msg.role {
                        crate::agent::message::Role::Assistant => {
                            let text = msg.content.as_text();
                            let snippet = if text.chars().count() > 64 {
                                let truncated: String = text.chars().take(60).collect();
                                format!("{}...", truncated.replace('\n', " "))
                            } else {
                                text.replace('\n', " ")
                            };
                            log.push_str(&format!("- Assistant decision: {}\n", snippet));
                        }
                        crate::agent::message::Role::Tool => {
                            let name = msg.name.as_deref().unwrap_or("unknown_tool");
                            log.push_str(&format!("- Result from: {}\n", name));
                        }
                        crate::agent::message::Role::User => {
                            let text = msg.content.as_text();
                            let user_snippet = if text.chars().count() > 40 {
                                let truncated: String = text.chars().take(40).collect();
                                format!("{}...", truncated)
                            } else {
                                text
                            };
                            log.push_str(&format!("- User requested: {}\n", user_snippet));
                        }
                        _ => {}
                    }
                }
                pressured_final_messages.push(Message::system(log));
            }
            pressured_final_messages.extend(selected_history.clone());
            final_messages = pressured_final_messages;
            effective_background_messages = pressured_background_messages;
        }

        let prompt_budget = self.final_context_prompt_budget();
        let (fitted_final_messages, final_fit_dropped_messages) =
            self.fit_messages_to_prompt_budget(final_messages, prompt_budget, is_local);
        final_messages = fitted_final_messages;
        estimated_final_prompt_tokens =
            Self::estimate_tokens_for_provider(&final_messages, is_local);
        prompt_occupancy_ratio = if max_window == 0 {
            0.0
        } else {
            estimated_final_prompt_tokens as f32 / max_window as f32
        };
        pressure_band = BackgroundPressureBand::from_prompt_occupancy_ratio(prompt_occupancy_ratio);

        if final_fit_dropped_messages > 0 {
            tracing::info!(
                dropped_messages = final_fit_dropped_messages,
                prompt_budget_tokens = prompt_budget,
                "ContextManager: final context fit trimmed prompt to runtime budget"
            );
        }

        let effective_background_tokens =
            Self::estimate_tokens_for_provider(&effective_background_messages, is_local);
        self.update_context_metrics(ContextOccupancyMetrics {
            max_window_tokens: max_window,
            reserved_response_tokens: reserved_response,
            safety_margin_tokens: SAFETY_MARGIN,
            history_budget_tokens: history_budget,
            static_prefix_tokens,
            provisional_background_tokens,
            effective_background_tokens,
            dynamic_injection_tokens,
            selected_history_tokens,
            pruned_history_tokens,
            estimated_prefix_tokens: prefix_tokens,
            estimated_final_prompt_tokens,
            effective_max_history_messages: effective_max_history,
            selected_history_messages,
            pruned_history_messages: pruned_history_messages
                .saturating_add(final_fit_dropped_messages),
            dynamic_injection_messages,
            background_message_count: effective_background_messages.len(),
            background_occupancy_ratio: if estimated_final_prompt_tokens == 0 {
                0.0
            } else {
                effective_background_tokens as f32 / estimated_final_prompt_tokens as f32
            },
            prompt_occupancy_ratio,
            pressure_band,
            local_provider_mode: is_local,
        });

        Ok(final_messages)
    }

    /// Estimate token count for a list of messages using tiktoken
    pub fn estimate_tokens(messages: &[Message]) -> usize {
        if let Ok(bpe) = tiktoken_rs::cl100k_base() {
            messages
                .iter()
                .map(|m| bpe.encode_with_special_tokens(&m.content.as_text()).len() + 4)
                .sum()
        } else {
            // Fallback to heuristic if tokenizer fails
            messages
                .iter()
                .map(|m| m.content.as_text().len() / 4)
                .sum::<usize>()
        }
    }

    fn estimate_tokens_for_provider(messages: &[Message], is_local: bool) -> usize {
        Self::adjust_estimated_tokens(Self::estimate_tokens(messages), is_local)
    }

    fn adjust_estimated_tokens(tokens: usize, is_local: bool) -> usize {
        if !is_local || tokens == 0 {
            return tokens;
        }

        let numerator = tokens.saturating_mul(Self::LOCAL_PROVIDER_TOKEN_INFLATION_NUMERATOR);
        numerator.div_ceil(Self::LOCAL_PROVIDER_TOKEN_INFLATION_DENOMINATOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::memory::{
        BackgroundEnvelope, BackgroundRevision, PersonaBackgroundLayer,
        RelationshipBackgroundLayer, SessionBackgroundState,
    };
    // use crate::agent::message::Content;

    struct StaticInjector;

    #[async_trait::async_trait]
    impl ContextInjector for StaticInjector {
        async fn inject(&self, _history: &[Message]) -> Result<Vec<Message>> {
            Ok(vec![Message::system(
                "### Dynamic Injector\n- Retrieved Context: enabled",
            )])
        }
    }

    #[tokio::test]
    async fn fallback_context_keeps_latest_user_before_system_recovery_marker() {
        let config = ContextConfig {
            max_history_messages: 1,
            max_tokens: 4096,
            response_reserve: 512,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_system_prompt("System Prompt");

        let history = vec![
            Message::system("Current Session ID: test"),
            Message::user("你好，用一句中文回复。"),
            Message::system("### TOOL EXECUTION REQUIRED"),
        ];

        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Fallback, true)
            .await
            .unwrap();

        assert!(
            ctx.iter()
                .any(|message| message.role == Role::User
                    && message.content.as_text().contains("你好")),
            "fallback context must keep the latest real user turn"
        );
        assert!(
            ctx.iter().any(|message| message
                .content
                .as_text()
                .contains("TOOL EXECUTION REQUIRED")),
            "fallback context should still keep the recovery marker"
        );
    }

    #[tokio::test]
    async fn test_smart_pruning_generation() {
        let config = ContextConfig {
            max_history_messages: 2, // Only keep 2 latest messages
            max_tokens: 10000,
            response_reserve: 1000,
            smart_pruning: true,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_system_prompt("System Prompt");

        let history = vec![
            Message::assistant("I am thinking about the first task."),
            Message::user("What about the second one?"),
            Message::assistant("Executing the third part now."),
            Message::user("Final question."),
        ];

        // Should keep "Executing the third part now." and "Final question."
        // And summarize "I am thinking about the first task." and "What about the second one?"
        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, false)
            .await
            .unwrap();

        // System Prompt + Observation Log + 2 History Messages = 4 messages
        assert_eq!(
            ctx.len(),
            4,
            "Context should contain System, Log, and 2 history messages"
        );

        let log_msg = &ctx[1];
        assert!(
            log_msg
                .content
                .as_text()
                .contains("Historical Context Summary"),
            "Should contain Historical Context Summary"
        );
        assert!(
            log_msg.content.as_text().contains("Assistant"),
            "Should mention Assistant in log"
        );
    }

    #[tokio::test]
    async fn test_tail_anchored_log() {
        let config = ContextConfig {
            max_history_messages: 1,
            max_tokens: 10000,
            smart_pruning: true,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_system_prompt("SYSTEM_PREFIX");

        let history = vec![
            Message::assistant("Pruned message"),
            Message::user("Recent message"),
        ];

        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, false)
            .await
            .unwrap();

        // Expected order: [System] -> [Log] -> [Recent Message]
        assert_eq!(ctx.len(), 3);
        assert_eq!(ctx[0].content.as_text(), "SYSTEM_PREFIX");
        assert!(ctx[1]
            .content
            .as_text()
            .contains("Historical Context Summary"));
        assert_eq!(ctx[2].content.as_text(), "Recent message");
    }

    #[tokio::test]
    async fn test_background_layers_precede_injectors_and_history() {
        let config = ContextConfig {
            max_history_messages: 1,
            max_tokens: 10000,
            smart_pruning: false,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_system_prompt("SYSTEM_PREFIX");
        mgr.set_background_envelope(BackgroundEnvelope {
            persona_layer: Some(PersonaBackgroundLayer {
                identity_summary: Some("You are BenShu.".to_string()),
                speaking_style: Some("Warm and concise.".to_string()),
                ..Default::default()
            }),
            relationship_layer: Some(RelationshipBackgroundLayer {
                relationship_summary: Some("Trusted long-term dialogue context.".to_string()),
                ..Default::default()
            }),
            session_layer: Some(SessionBackgroundState {
                active_topics: vec!["long-running dialogue continuity".to_string()],
                summary: Some("We are planning a persistent background layer.".to_string()),
                ..Default::default()
            }),
            revision: BackgroundRevision {
                revision: 1,
                ..Default::default()
            },
            ..Default::default()
        });
        mgr.add_injector(std::sync::Arc::new(StaticInjector));

        let history = vec![
            Message::assistant("Older history"),
            Message::user("Current question"),
        ];

        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, false)
            .await
            .unwrap();

        assert_eq!(ctx[0].content.as_text(), "SYSTEM_PREFIX");
        assert!(ctx[1].content.as_text().contains("### Core Persona Layer"));
        assert!(ctx[2].content.as_text().contains("### Relationship Layer"));
        assert!(ctx[3]
            .content
            .as_text()
            .contains("### Ongoing Session Layer"));
        assert!(ctx[4].content.as_text().contains("### Dynamic Injector"));
        assert_eq!(ctx[5].content.as_text(), "Current question");
    }

    #[tokio::test]
    async fn test_local_context_keeps_background_layers() {
        let config = ContextConfig::default();
        let mut mgr = ContextManager::new(config);
        mgr.set_background_envelope(BackgroundEnvelope {
            persona_layer: Some(PersonaBackgroundLayer {
                identity_summary: Some("Persistent agent identity.".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });

        let history = vec![Message::user("Hello")];
        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, true)
            .await
            .unwrap();

        assert!(ctx[0].content.as_text().contains("### Core Persona Layer"));
        assert_eq!(ctx[1].content.as_text(), "Hello");
    }

    #[tokio::test]
    async fn test_local_context_uses_recent_history_window_instead_of_full_history() {
        let config = ContextConfig {
            max_history_messages: 50,
            max_tokens: 10000,
            smart_pruning: false,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_background_envelope(BackgroundEnvelope {
            persona_layer: Some(PersonaBackgroundLayer {
                identity_summary: Some("Persistent agent identity.".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });

        let history = (0..20)
            .map(|idx| Message::user(format!("message-{idx}")))
            .collect::<Vec<_>>();

        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, true)
            .await
            .unwrap();

        let joined = ctx
            .iter()
            .map(|msg| msg.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(joined.contains("### Core Persona Layer"));
        assert!(joined.contains("message-19"));
        assert!(!joined.contains("message-0"));
        assert_eq!(
            ctx.iter()
                .filter(|msg| msg.content.as_text().starts_with("message-"))
                .count(),
            ContextManager::LOCAL_RECENT_HISTORY_MESSAGES
        );
    }

    #[tokio::test]
    async fn test_local_context_prefers_background_over_pruned_history_summary() {
        let config = ContextConfig {
            max_history_messages: 2,
            max_tokens: 10000,
            smart_pruning: true,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_background_envelope(BackgroundEnvelope {
            persona_layer: Some(PersonaBackgroundLayer {
                identity_summary: Some("Persistent agent identity.".to_string()),
                ..Default::default()
            }),
            session_layer: Some(SessionBackgroundState {
                summary: Some(
                    "Long-running context has already been folded into background.".to_string(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        });

        let history = (0..20)
            .map(|idx| Message::user(format!("message-{idx}")))
            .collect::<Vec<_>>();

        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, true)
            .await
            .unwrap();

        let joined = ctx
            .iter()
            .map(|msg| msg.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(joined.contains("### Core Persona Layer"));
        assert!(
            !joined.contains("### Historical Context Summary (Pruned)"),
            "local context with background should let the background envelope carry older continuity instead of adding a second pruned-history summary layer",
        );
    }

    #[tokio::test]
    async fn test_session_layer_renders_working_mode_and_interaction_theme() {
        let config = ContextConfig::default();
        let mut mgr = ContextManager::new(config);
        mgr.set_background_envelope(BackgroundEnvelope {
            session_layer: Some(SessionBackgroundState {
                backend_context_records: vec![
                    crate::agent::memory::BackendContextRecord {
                        kind: Some(crate::agent::memory::BackendContextKind::Web),
                        value: "https://example.com/dashboard".to_string(),
                        source: Some("source_url".to_string()),
                    },
                    crate::agent::memory::BackendContextRecord {
                        kind: Some(crate::agent::memory::BackendContextKind::MemoryRecall),
                        value: "relationship_memory".to_string(),
                        source: Some("memory_recall".to_string()),
                    },
                ],
                retrieved_memory_objects: vec![crate::agent::memory::RetrievedMemoryObject {
                    recall_source: "relationship_memory".to_string(),
                    recall_kind: Some("memory_recall".to_string()),
                    collection: Some("memory".to_string()),
                    retrieval_query: Some("长期称呼偏好".to_string()),
                    recall_summary: Some("保留稳定称呼方式".to_string()),
                }],
                web_session_objects: vec![crate::agent::memory::WebSessionObject {
                    url: "https://example.com/background-window".to_string(),
                    page_title: Some("BenShu Gateway".to_string()),
                    task_goal: Some("review browser result".to_string()),
                }],
                artifact_session_objects: vec![crate::agent::memory::ArtifactSessionObject {
                    path: "/tmp/spec.pdf".to_string(),
                    collection: Some("docs".to_string()),
                    task_goal: Some("align current plan".to_string()),
                }],
                task_session_objects: vec![crate::agent::memory::TaskSessionObject {
                    state: "background_window_review".to_string(),
                    title: Some("背景压缩主线".to_string()),
                    goal: Some("keep current task stable".to_string()),
                }],
                tool_session_objects: vec![crate::agent::memory::ToolSessionObject {
                    tool_name: "browser_snapshot".to_string(),
                    result_summary: Some(
                        "current browser result enters active background".to_string(),
                    ),
                    route: Some("browser_snapshot".to_string()),
                    source_ref: Some("https://example.com/background-window".to_string()),
                }],
                multimodal_session_objects: vec![crate::agent::memory::MultimodalSessionObject {
                    locator: "/tmp/dashboard.png".to_string(),
                    route: Some("image_page_raster".to_string()),
                    modality: Some("image".to_string()),
                    collection: Some("desktop_capture".to_string()),
                    source_url: Some("https://example.com/dashboard.png".to_string()),
                    title: Some("dashboard screenshot".to_string()),
                    task_goal: Some("review browser result".to_string()),
                }],
                metadata: std::collections::HashMap::from([
                    ("working_mode".to_string(), "browser_review".to_string()),
                    (
                        "interaction_theme".to_string(),
                        "collaborative_progress".to_string(),
                    ),
                ]),
                ..Default::default()
            }),
            ..Default::default()
        });

        let history = vec![Message::user("继续")];
        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, false)
            .await
            .unwrap();

        assert!(ctx[0]
            .content
            .as_text()
            .contains("Working Mode: browser_review"));
        assert!(ctx[0]
            .content
            .as_text()
            .contains("Interaction Theme: collaborative_progress"));
        assert!(ctx[0]
            .content
            .as_text()
            .contains("Backend Context Records: Web context: https://example.com/dashboard"));
        assert!(ctx[0]
            .content
            .as_text()
            .contains("Memory recall: relationship_memory"));
        assert!(ctx[0].content.as_text().contains(
            "Retrieved Memory Objects: Memory Recall Object: source=relationship_memory"
        ));
        assert!(ctx[0].content.as_text().contains("kind=memory_recall"));
        assert!(ctx[0].content.as_text().contains(
            "Web Session Objects: Web Session Object: url=https://example.com/background-window"
        ));
        assert!(ctx[0]
            .content
            .as_text()
            .contains("Artifact Session Objects: Artifact Session Object: path=/tmp/spec.pdf"));
        assert!(ctx[0]
            .content
            .as_text()
            .contains("Task Session Objects: Task Session Object: state=background_window_review"));
        assert!(ctx[0]
            .content
            .as_text()
            .contains("Tool Session Objects: Tool Session Object: tool=browser_snapshot"));
        assert!(ctx[0].content.as_text().contains(
            "Multimodal Session Objects: Multimodal Session Object: locator=/tmp/dashboard.png"
        ));
    }

    #[tokio::test]
    async fn test_session_layer_renders_explicit_compression_slots() {
        let config = ContextConfig::default();
        let mut mgr = ContextManager::new(config);
        let mut session_layer = SessionBackgroundState::default();
        session_layer.set_compression_slots(crate::agent::memory::BackgroundCompressionSlots {
            project_facts: vec!["BenShu is Windows-first".to_string()],
            current_task: Some("Migrate generic compression helpers".to_string()),
            completed_work: vec!["Command output compression is wired".to_string()],
            pending_work: vec!["Run final targeted tests".to_string()],
            key_files: vec!["crates/brain/src/agent/context.rs".to_string()],
            test_results: vec!["cargo test -p benshu-brain context passed".to_string()],
            risks: vec!["Do not trust compressed code facts without re-checking files".to_string()],
            verification_needs: vec![
                "Use git and gateway evidence before final delivery".to_string()
            ],
        });
        mgr.set_background_envelope(BackgroundEnvelope {
            session_layer: Some(session_layer),
            ..Default::default()
        });

        let ctx = mgr
            .build_context(
                &[Message::user("继续")],
                &crate::agent::attempt::Strategy::Standard,
                false,
            )
            .await
            .unwrap();
        let text = ctx[0].content.as_text();

        assert!(text.contains("Project Facts: BenShu is Windows-first"));
        assert!(text.contains("Current Task: Migrate generic compression helpers"));
        assert!(text.contains("Completed Work: Command output compression is wired"));
        assert!(text.contains("Pending Work: Run final targeted tests"));
        assert!(text.contains("Key Files: crates/brain/src/agent/context.rs"));
        assert!(text.contains("Test Results: cargo test -p benshu-brain context passed"));
        assert!(text.contains("Risks: Do not trust compressed code facts"));
        assert!(text.contains("Verification Needs: Use git and gateway evidence"));
        assert!(text.contains("Compressed Claim Rule: verify file/repo/runtime/web facts"));
    }

    #[tokio::test]
    async fn test_recent_window_summary_is_suppressed_when_recent_history_is_present() {
        let config = ContextConfig {
            max_history_messages: 2,
            max_tokens: 10000,
            smart_pruning: false,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_background_envelope(BackgroundEnvelope {
            recent_window_summary: Some(crate::agent::memory::RecentWindowSummary {
                summary: "Recent work involved browser review and tool follow-ups.".to_string(),
                covered_message_count: 2,
                ..Default::default()
            }),
            ..Default::default()
        });

        let history = vec![
            Message::assistant("We just reviewed the browser state."),
            Message::user("继续看最近那两个结果。"),
        ];

        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, false)
            .await
            .unwrap();

        assert!(
            ctx.iter()
                .all(|msg| !msg.content.as_text().contains("### Recent Window Summary")),
            "recent window summary should be suppressed when raw recent history is already present",
        );
    }

    #[tokio::test]
    async fn test_recent_history_deduplicates_matching_backend_objects() {
        let config = ContextConfig {
            max_history_messages: 4,
            max_tokens: 10000,
            smart_pruning: false,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_background_envelope(BackgroundEnvelope {
            session_layer: Some(SessionBackgroundState {
                workspace_focus: Some("Reviewing browser snapshot output".to_string()),
                backend_context_records: vec![
                    crate::agent::memory::BackendContextRecord {
                        kind: Some(crate::agent::memory::BackendContextKind::Web),
                        value: "https://example.com/dashboard".to_string(),
                        source: Some("source_url".to_string()),
                    },
                    crate::agent::memory::BackendContextRecord {
                        kind: Some(crate::agent::memory::BackendContextKind::MemoryRecall),
                        value: "relationship_memory".to_string(),
                        source: Some("retrieved_from".to_string()),
                    },
                ],
                retrieved_memory_objects: vec![crate::agent::memory::RetrievedMemoryObject {
                    recall_source: "relationship_memory".to_string(),
                    recall_kind: Some("memory_recall".to_string()),
                    collection: Some("memory".to_string()),
                    retrieval_query: Some("稳定称呼偏好".to_string()),
                    recall_summary: Some("保留稳定称呼".to_string()),
                }],
                web_session_objects: vec![crate::agent::memory::WebSessionObject {
                    url: "https://example.com/dashboard".to_string(),
                    page_title: Some("Gateway".to_string()),
                    task_goal: Some("review browser".to_string()),
                }],
                artifact_session_objects: vec![crate::agent::memory::ArtifactSessionObject {
                    path: "/tmp/spec.pdf".to_string(),
                    collection: Some("docs".to_string()),
                    task_goal: Some("read spec".to_string()),
                }],
                tool_session_objects: vec![crate::agent::memory::ToolSessionObject {
                    tool_name: "browser_snapshot".to_string(),
                    result_summary: Some("dashboard captured".to_string()),
                    route: Some("browser_snapshot".to_string()),
                    source_ref: Some("https://example.com/dashboard".to_string()),
                }],
                multimodal_session_objects: vec![crate::agent::memory::MultimodalSessionObject {
                    locator: "/tmp/dashboard.png".to_string(),
                    route: Some("image_page_raster".to_string()),
                    modality: Some("image".to_string()),
                    collection: Some("desktop_capture".to_string()),
                    source_url: Some("https://example.com/dashboard.png".to_string()),
                    title: Some("dashboard screenshot".to_string()),
                    task_goal: Some("review dashboard".to_string()),
                }],
                ..Default::default()
            }),
            ..Default::default()
        });

        let mut recall = Message::tool_result("call_recall", "memory recall ready")
            .with_tool_name("memory_recall");
        recall.metadata.insert(
            "retrieved_from".to_string(),
            "relationship_memory".to_string(),
        );

        let mut browser = Message::tool_result("call_browser", "browser snapshot ready")
            .with_tool_name("browser_snapshot");
        browser.metadata.insert(
            "source_url".to_string(),
            "https://example.com/dashboard".to_string(),
        );

        let mut doc =
            Message::tool_result("call_doc", "pdf parse ready").with_tool_name("pdf_parse");
        doc.source_path = Some("/tmp/spec.pdf".to_string());

        let mut screenshot = Message::tool_result("call_screen", "desktop screenshot ready")
            .with_tool_name("browser_screenshot");
        screenshot.source_path = Some("/tmp/dashboard.png".to_string());
        screenshot.metadata.insert(
            "media_preprocess_source_ref".to_string(),
            "/tmp/dashboard.png".to_string(),
        );

        let history = vec![recall, browser, doc, screenshot];
        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, false)
            .await
            .unwrap();

        let joined = ctx
            .iter()
            .map(|msg| msg.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !joined.contains("Retrieved Memory Objects:"),
            "retrieved memory objects should not be repeated when recent history already carries the same recall",
        );
        assert!(
            !joined.contains("Web Session Objects:"),
            "web session objects should not be repeated when the same recent browser result is still present",
        );
        assert!(
            !joined.contains("Artifact Session Objects:"),
            "artifact session objects should not be repeated when the same recent document result is still present",
        );
        assert!(
            !joined.contains("Tool Session Objects:"),
            "tool session objects should not be repeated when the same recent tool result is still present",
        );
        assert!(
            !joined.contains("Multimodal Session Objects:"),
            "multimodal session objects should not be repeated when the same recent screenshot result is still present",
        );
        assert!(
            joined.contains("Workspace Focus: Reviewing browser snapshot output"),
            "higher-level workspace focus should still survive background deduplication",
        );
    }

    #[tokio::test]
    async fn test_recent_history_suppresses_duplicate_session_text_layers() {
        let config = ContextConfig {
            max_history_messages: 4,
            max_tokens: 10000,
            smart_pruning: false,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_background_envelope(BackgroundEnvelope {
            session_layer: Some(SessionBackgroundState {
                active_topics: vec!["windows runtime slimming migration".to_string()],
                open_loops: vec!["Finalize the OCR specialist rollout".to_string()],
                recent_emotional_state: Some("calm and focused".to_string()),
                ongoing_goals: vec!["Keep coordinator first routing stable".to_string()],
                workspace_focus: Some("Document the context slimming rollout".to_string()),
                pending_followups: vec!["Document the context slimming rollout".to_string()],
                metadata: std::collections::HashMap::from([(
                    "working_mode".to_string(),
                    "coordinator".to_string(),
                )]),
                ..Default::default()
            }),
            ..Default::default()
        });

        let history = vec![
            Message::assistant(
                "We are calm and focused while we finalize the OCR specialist rollout.",
            ),
            Message::user(
                "Continue the windows runtime slimming migration, keep coordinator first routing stable, and document the context slimming rollout.",
            ),
        ];

        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, false)
            .await
            .unwrap();

        let joined = ctx
            .iter()
            .map(|msg| msg.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !joined.contains("Active Topics:"),
            "session active topics should be suppressed when recent raw history already states the same topic",
        );
        assert!(
            !joined.contains("Open Loops:"),
            "session open loops should be suppressed when recent raw history already states the same loop",
        );
        assert!(
            !joined.contains("Recent Emotional State:"),
            "recent emotional state should be suppressed when recent raw history already states it",
        );
        assert!(
            !joined.contains("Ongoing Goals:"),
            "ongoing goals should be suppressed when recent raw history already states the same goal",
        );
        assert!(
            !joined.contains("Workspace Focus:"),
            "workspace focus should be suppressed when recent raw history already states the same focus",
        );
        assert!(
            !joined.contains("Pending Follow-ups:"),
            "pending follow-ups should be suppressed when recent raw history already states the same follow-up",
        );
        assert!(
            joined.contains("Working Mode: coordinator"),
            "higher-level session metadata should still survive text-layer deduplication",
        );
    }

    #[test]
    fn test_soft_trim_utility() {
        let mut msg = Message::user("A".repeat(10000));
        msg.soft_trim(2000);
        let text = msg.text();
        assert!(text.contains("context safety"));
        assert!(text.len() < 3000);
        assert!(text.starts_with(&"A".repeat(100)));
        assert!(text.ends_with(&"A".repeat(100)));
    }

    #[tokio::test]
    async fn test_context_manager_records_latest_occupancy_metrics() {
        let config = ContextConfig {
            max_history_messages: 6,
            max_tokens: 12000,
            response_reserve: 1024,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_system_prompt("You are a routing-first coordinator.");
        mgr.set_background_envelope(BackgroundEnvelope {
            recent_window_summary: Some(crate::agent::memory::RecentWindowSummary {
                summary: "The user is iterating on long-session background compression policy."
                    .to_string(),
                pruned_message_count: 8,
                covered_message_count: 8,
                metadata: std::collections::HashMap::new(),
            }),
            persona_layer: Some(PersonaBackgroundLayer {
                identity_summary: Some("BenShu frontstage coordinator".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });

        let history = vec![
            Message::user("帮我分析一下当前背景占用了多少 prompt"),
            Message::assistant("我会先测背景、历史和动态注入分别占多少。"),
            Message::user("然后把这些指标写进 trace 里。"),
        ];

        let _ = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, false)
            .await
            .unwrap();

        let metrics = mgr
            .latest_context_metrics()
            .expect("context metrics should be recorded after build_context");

        assert!(metrics.max_window_tokens >= 12000);
        assert!(metrics.static_prefix_tokens > 0);
        assert!(metrics.provisional_background_tokens > 0);
        assert!(metrics.estimated_final_prompt_tokens > 0);
        assert!(metrics.background_message_count > 0);
        assert!(metrics.selected_history_messages > 0);
    }

    #[tokio::test]
    async fn test_high_pressure_compacts_recent_window_and_session_summary() {
        let config = ContextConfig {
            max_history_messages: 8,
            max_tokens: 60,
            response_reserve: 16,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_system_prompt("You are a coordinator with a deliberately oversized prompt prefix for pressure testing. Repeat stable coordination posture and memory policy. Repeat stable coordination posture and memory policy. Repeat stable coordination posture and memory policy. Repeat stable coordination posture and memory policy.");
        mgr.set_background_envelope(BackgroundEnvelope {
            session_layer: Some(SessionBackgroundState {
                summary: Some(
                    "This summary should be removed when pressure gets high.".to_string(),
                ),
                active_topics: vec!["background occupancy rollout".to_string()],
                ..Default::default()
            }),
            recent_window_summary: Some(crate::agent::memory::RecentWindowSummary {
                summary: "This recent window summary should disappear under pressure.".to_string(),
                pruned_message_count: 4,
                covered_message_count: 4,
                metadata: std::collections::HashMap::new(),
            }),
            ..Default::default()
        });

        let history = vec![
            Message::user("继续聊当前实现细节，并把背景 occupancy、prompt occupancy、history budget、dynamic injection 都算出来。"),
            Message::assistant("我会优先保留人格和关系层，然后把 session 摘要和 recent window summary 作为高水位第一批压缩目标。"),
            Message::user("然后继续补充一批上下文文本，确保这一轮真的进入高水位压力状态。"),
        ];

        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, false)
            .await
            .unwrap();
        let joined = ctx
            .iter()
            .map(|msg| msg.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        let metrics = mgr
            .latest_context_metrics()
            .expect("context metrics should be available");

        assert_ne!(metrics.pressure_band, BackgroundPressureBand::Normal);
        assert!(
            !joined.contains("### Recent Window Summary"),
            "recent window summary should be removed under pressure"
        );
        assert!(
            !joined.contains("Session Summary:"),
            "session summary should be removed under pressure"
        );
    }

    #[tokio::test]
    async fn test_lowered_context_window_hard_fits_old_background_and_history() {
        let config = ContextConfig {
            max_history_messages: 20,
            max_tokens: 1024,
            response_reserve: 128,
            smart_pruning: true,
            ..Default::default()
        };
        let mut mgr = ContextManager::new(config);
        mgr.set_system_prompt(format!(
            "SYSTEM GOVERNANCE {}\nKeep the latest user request.",
            "fixed policy. ".repeat(400)
        ));
        mgr.set_background_envelope(BackgroundEnvelope {
            session_layer: Some(SessionBackgroundState {
                summary: Some("旧会话摘要 ".repeat(1000)),
                active_topics: vec!["上下文预算回归".to_string(); 20],
                open_loops: vec![
                    "确认降 ctx_size 后不会继续注入旧大摘要".to_string();
                    20
                ],
                ..Default::default()
            }),
            recent_window_summary: Some(crate::agent::memory::RecentWindowSummary {
                summary: "旧 recent window summary ".repeat(1000),
                pruned_message_count: 100,
                covered_message_count: 100,
                metadata: std::collections::HashMap::new(),
            }),
            ..Default::default()
        });

        let mut history = Vec::new();
        for idx in 0..12 {
            history.push(Message::assistant(format!(
                "旧助手长内容 {idx}: {}",
                "历史正文 ".repeat(500)
            )));
            history.push(Message::user(format!(
                "旧用户长内容 {idx}: {}",
                "历史问题 ".repeat(500)
            )));
        }
        history.push(Message::user("当前问题：请只回答这一轮。"));

        let ctx = mgr
            .build_context(&history, &crate::agent::attempt::Strategy::Standard, true)
            .await
            .unwrap();
        let joined = ctx
            .iter()
            .map(|message| message.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        let metrics = mgr
            .latest_context_metrics()
            .expect("context metrics should be recorded");

        assert_eq!(metrics.max_window_tokens, 1024);
        assert!(
            metrics.estimated_final_prompt_tokens <= mgr.final_context_prompt_budget(),
            "prompt should fit lowered runtime context budget: {} > {}",
            metrics.estimated_final_prompt_tokens,
            mgr.final_context_prompt_budget()
        );
        assert!(joined.contains("当前问题"));
        assert!(
            !joined.contains("旧 recent window summary 旧 recent window summary"),
            "old oversized background summary should not be injected verbatim after ctx shrink"
        );
    }

    #[test]
    fn test_pressure_compaction_critical_keeps_core_layers_and_trims_session_objects() {
        let mut envelope = BackgroundEnvelope {
            persona_layer: Some(PersonaBackgroundLayer {
                identity_summary: Some("Keep persona".to_string()),
                ..Default::default()
            }),
            relationship_layer: Some(RelationshipBackgroundLayer {
                relationship_summary: Some("Keep relationship".to_string()),
                ..Default::default()
            }),
            session_layer: Some(SessionBackgroundState {
                summary: Some("Drop me".to_string()),
                backend_context_records: vec![
                    crate::agent::memory::BackendContextRecord {
                        kind: Some(crate::agent::memory::BackendContextKind::Web),
                        value: "a".to_string(),
                        source: None,
                    },
                    crate::agent::memory::BackendContextRecord {
                        kind: Some(crate::agent::memory::BackendContextKind::Web),
                        value: "b".to_string(),
                        source: None,
                    },
                    crate::agent::memory::BackendContextRecord {
                        kind: Some(crate::agent::memory::BackendContextKind::Web),
                        value: "c".to_string(),
                        source: None,
                    },
                ],
                web_session_objects: vec![
                    crate::agent::memory::WebSessionObject {
                        url: "a".to_string(),
                        page_title: None,
                        task_goal: None,
                    },
                    crate::agent::memory::WebSessionObject {
                        url: "b".to_string(),
                        page_title: None,
                        task_goal: None,
                    },
                    crate::agent::memory::WebSessionObject {
                        url: "c".to_string(),
                        page_title: None,
                        task_goal: None,
                    },
                ],
                active_topics: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                open_loops: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                ..Default::default()
            }),
            recent_window_summary: Some(crate::agent::memory::RecentWindowSummary {
                summary: "Drop me".to_string(),
                pruned_message_count: 3,
                covered_message_count: 3,
                metadata: std::collections::HashMap::new(),
            }),
            ..Default::default()
        };

        ContextManager::pressure_compact_envelope(&mut envelope, BackgroundPressureBand::Critical);

        let session = envelope
            .session_layer
            .as_ref()
            .expect("session layer should remain");
        assert!(envelope.persona_layer.is_some());
        assert!(envelope.relationship_layer.is_some());
        assert!(envelope.recent_window_summary.is_none());
        assert!(session.summary.is_none());
        assert!(session.backend_context_records.len() <= 2);
        assert!(session.web_session_objects.len() <= 2);
        assert!(session.active_topics.len() <= 2);
        assert!(session.open_loops.len() <= 2);
    }
}
