use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NluIntent {
    pub name: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NluSlot {
    pub name: String,
    pub value: String,
    pub confidence: f32,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetabolicMode {
    Optimal,   // GPU FP16/32
    Efficient, // INT8/U8
    Cold,      // INT4 (CPU Accelerated)
    Survival,  // Heuristic/Cloud Only
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityReference {
    pub surface_form: String,       // "he", "that file", "it"
    pub resolved_entity_id: String, // "person:mark_zuckerberg", "file:/path/to/log"
    pub confidence: f32,
    pub reference_type: ReferenceType,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReferenceType {
    Pronoun,      // he, she, it
    DefiniteNoun, // the file, that report
    Deictic,      // here, there (spatial reference)
    Temporal,     // then, at that time
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DialogueContext {
    pub recent_entities: Vec<String>, // Last N entities mentioned
    pub current_topic: Option<String>,
    pub previous_intent: Option<String>,
    pub turn_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NluResult {
    pub intent: NluIntent,
    pub slots: Vec<NluSlot>,
    pub references: Vec<EntityReference>,
    pub mode: MetabolicMode,
    pub metadata: serde_json::Value,
}

#[async_trait]
pub trait NluEngine: Send + Sync {
    /// Identify intent and extract slots from text
    async fn analyze(&self, text: &str) -> NluResult;

    /// Stateful analysis with Coreference Resolution
    async fn analyze_with_context(&self, text: &str, context: &DialogueContext) -> NluResult {
        // Default implementation delegates to basic analyze
        let mut res = self.analyze(text).await;
        let _ = context;
        res
    }

    /// Get model info for UI display
    fn model_info(&self) -> String;

    /// Current health/load status
    fn status(&self) -> crate::traits::HealthStatus;
}
