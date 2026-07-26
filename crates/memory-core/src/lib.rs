pub mod background;
pub mod capabilities;
pub mod knowledge;
pub mod multimodal;
pub mod review;

pub use background::{
    ArtifactSessionObject, BackendContextKind, BackendContextRecord, BackgroundCompressionDecision,
    BackgroundCompressionSlots, BackgroundEnvelope, BackgroundEvidenceRef, BackgroundQualitySignal,
    BackgroundRevision, MultimodalSessionObject, PersonaBackgroundLayer, RecentWindowSummary,
    RelationshipBackgroundLayer, RetrievedMemoryObject, SessionBackgroundState, TaskSessionObject,
    ToolSessionObject, WebSessionObject,
};
pub use capabilities::MemoryCapabilities;
pub use knowledge::{
    traverse_related_facts, traverse_related_facts_with_report, Document, Fact, FactProtection,
    FactStatus, Relation, RelationQueryBudget, RelationTraversalReport, RelationTraversalResult,
    RELATION_QUERY_DEFAULT_MAX_DEPTH, RELATION_QUERY_DEFAULT_MAX_RETURNED_EDGES,
    RELATION_QUERY_DEFAULT_MAX_VISITED_NODES, RELATION_QUERY_HARD_CAP_DEPTH,
};
pub use multimodal::{MultimodalDerivedFact, MultimodalMemoryKind, MultimodalMemoryRecord};
pub use review::{FactReviewPayload, FactReviewResolution, FactReviewResolutionOutcome};
