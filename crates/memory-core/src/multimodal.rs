use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MultimodalMemoryKind {
    Understanding,
    GenerationProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultimodalDerivedFact {
    pub content: String,
    pub category: String,
    #[serde(default = "default_multimodal_fact_importance")]
    pub importance: f32,
    #[serde(default)]
    pub verified: bool,
}

fn default_multimodal_fact_importance() -> f32 {
    0.6
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultimodalMemoryRecord {
    pub kind: MultimodalMemoryKind,
    pub modality: String,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub collection: String,
    pub source_path: Option<String>,
    pub source_url: Option<String>,
    pub route: Option<String>,
    pub model: Option<String>,
    pub prompt: Option<String>,
    pub artifact_locator: Option<String>,
    #[serde(default)]
    pub transient: bool,
    pub derived_fact: Option<MultimodalDerivedFact>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

impl MultimodalMemoryRecord {
    pub fn kind_slug(&self) -> &'static str {
        match self.kind {
            MultimodalMemoryKind::Understanding => "understanding",
            MultimodalMemoryKind::GenerationProvenance => "generation_provenance",
        }
    }
}
