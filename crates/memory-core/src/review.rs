use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactReviewResolutionOutcome {
    Verified,
    Pruned,
    PendingReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactReviewResolution {
    pub outcome: FactReviewResolutionOutcome,
    pub resolution_reason: Option<String>,
    pub resolution_basis: Option<String>,
    pub resolved_by: Option<String>,
    pub resolved_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FactReviewPayload {
    pub review_reason: Option<String>,
    pub challenger_summary: Option<String>,
    pub challenger_source: Option<String>,
    pub review_requested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub resolution: Option<FactReviewResolution>,
}
