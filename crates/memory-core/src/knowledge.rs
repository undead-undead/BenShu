use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub const RELATION_QUERY_DEFAULT_MAX_DEPTH: usize = 2;
pub const RELATION_QUERY_HARD_CAP_DEPTH: usize = 3;
pub const RELATION_QUERY_DEFAULT_MAX_VISITED_NODES: usize = 64;
pub const RELATION_QUERY_DEFAULT_MAX_RETURNED_EDGES: usize = 128;

/// Discovered vs verified status for human-in-the-loop memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    /// Newly distilled or extracted, awaiting confirmation or conflict check.
    Pending,
    /// Confirmed by user or system auditor.
    Verified,
    /// Conflicting with existing facts, awaiting human resolution.
    PendingReview,
    /// Pragmatically archived or decayed.
    Archived,
}

impl Default for FactStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FactProtection {
    #[default]
    Normal,
    Pinned,
    Protected,
    CoreIdentity,
}

/// A distilled piece of knowledge with graph support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Unique ID for the fact.
    pub id: String,
    /// The category, for example Personal, Work, or Preference.
    pub category: String,
    /// The distilled content.
    pub content: String,
    /// Importance score from 0.0 to 1.0.
    pub importance: f32,
    /// When it was first discovered.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// User verification status, deprecated in favor of status.
    pub verified: bool,
    /// Source of the fact, for example session ID.
    pub source: Option<String>,
    /// Confidence level from 0.0 to 1.0.
    pub confidence: f32,
    /// Graph relations: this fact -[predicate]-> target fact ID.
    #[serde(default)]
    pub relations: Vec<Relation>,
    /// Semantic hash for deduplication.
    pub semantic_hash: Option<String>,
    /// Human-in-the-loop status.
    #[serde(default)]
    pub status: FactStatus,
    /// Protected memory semantics against automatic decay/prune.
    #[serde(default)]
    pub protection: FactProtection,
}

/// A directed edge in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    /// The predicate, for example works_at, likes, or related_to.
    pub predicate: String,
    /// The target fact ID.
    pub target_id: String,
    /// Strength of the relationship from 0.0 to 1.0.
    pub strength: f32,
}

/// A document retrieved from or stored in durable knowledge memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Unique identifier.
    pub id: String,
    /// The title or mnemonic for the document.
    pub title: String,
    /// The full text content.
    pub content: String,
    /// A shorter summary of the content.
    pub summary: Option<String>,
    /// The collection it belongs to.
    pub collection: Option<String>,
    /// The virtual path/source.
    pub path: Option<String>,
    /// Metadata associated with the document.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Similarity score.
    #[serde(default)]
    pub score: f32,
}

impl Fact {
    pub fn new(content: impl Into<String>, category: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            category: category.into(),
            content: content.into(),
            importance: 0.5,
            created_at: now,
            updated_at: now,
            verified: false,
            source: None,
            confidence: 1.0,
            relations: Vec::new(),
            semantic_hash: None,
            status: FactStatus::Pending,
            protection: FactProtection::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RelationQueryBudget {
    pub max_depth: usize,
    pub max_visited_nodes: usize,
    pub max_returned_edges: usize,
}

impl RelationQueryBudget {
    pub fn for_requested_depth(depth: usize) -> Self {
        let max_depth = if depth == 0 {
            0
        } else {
            depth.min(RELATION_QUERY_HARD_CAP_DEPTH)
        };
        Self {
            max_depth,
            max_visited_nodes: RELATION_QUERY_DEFAULT_MAX_VISITED_NODES,
            max_returned_edges: RELATION_QUERY_DEFAULT_MAX_RETURNED_EDGES,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelationTraversalReport {
    pub requested_depth: usize,
    pub effective_max_depth: usize,
    pub visited_nodes: usize,
    pub traversed_edges: usize,
    pub returned_facts: usize,
    pub cycle_safe: bool,
    pub depth_hard_capped: bool,
    pub visited_budget_exceeded: bool,
    pub edge_budget_exceeded: bool,
    pub truncated: bool,
    pub truncation_reason: Option<String>,
}

impl RelationTraversalReport {
    pub fn metadata_entries(&self, root_fact_id: &str) -> Vec<(String, String)> {
        vec![
            (
                "brain.memory.relation.last_root_fact_id".to_string(),
                root_fact_id.to_string(),
            ),
            (
                "brain.memory.relation.last_requested_depth".to_string(),
                self.requested_depth.to_string(),
            ),
            (
                "brain.memory.relation.last_effective_max_depth".to_string(),
                self.effective_max_depth.to_string(),
            ),
            (
                "brain.memory.relation.last_visited_nodes".to_string(),
                self.visited_nodes.to_string(),
            ),
            (
                "brain.memory.relation.last_traversed_edges".to_string(),
                self.traversed_edges.to_string(),
            ),
            (
                "brain.memory.relation.last_returned_facts".to_string(),
                self.returned_facts.to_string(),
            ),
            (
                "brain.memory.relation.last_cycle_safe".to_string(),
                self.cycle_safe.to_string(),
            ),
            (
                "brain.memory.relation.last_depth_hard_capped".to_string(),
                self.depth_hard_capped.to_string(),
            ),
            (
                "brain.memory.relation.last_visited_budget_exceeded".to_string(),
                self.visited_budget_exceeded.to_string(),
            ),
            (
                "brain.memory.relation.last_edge_budget_exceeded".to_string(),
                self.edge_budget_exceeded.to_string(),
            ),
            (
                "brain.memory.relation.last_budget_exceeded".to_string(),
                (self.visited_budget_exceeded
                    || self.edge_budget_exceeded
                    || self.depth_hard_capped)
                    .to_string(),
            ),
            (
                "brain.memory.relation.last_truncated".to_string(),
                self.truncated.to_string(),
            ),
            (
                "brain.memory.relation.last_truncation_reason".to_string(),
                self.truncation_reason.clone().unwrap_or_default(),
            ),
        ]
    }
}

#[derive(Debug, Clone)]
pub struct RelationTraversalResult {
    pub facts: Vec<Fact>,
    pub report: RelationTraversalReport,
}

pub fn traverse_related_facts_with_report(
    facts_by_id: &std::collections::HashMap<String, Fact>,
    fact_id: &str,
    depth: usize,
) -> RelationTraversalResult {
    let budget = RelationQueryBudget::for_requested_depth(depth);
    let mut results = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut to_visit = std::collections::VecDeque::new();
    let mut traversed_edges = 0usize;
    let mut visited_budget_exceeded = false;
    let mut edge_budget_exceeded = false;
    let depth_hard_capped = depth > budget.max_depth;

    if budget.max_depth == 0 {
        let report = RelationTraversalReport {
            requested_depth: depth,
            effective_max_depth: budget.max_depth,
            visited_nodes: 0,
            traversed_edges: 0,
            returned_facts: 0,
            cycle_safe: true,
            depth_hard_capped,
            visited_budget_exceeded,
            edge_budget_exceeded,
            truncated: depth_hard_capped,
            truncation_reason: depth_hard_capped.then(|| "depth_hard_cap".to_string()),
        };
        return RelationTraversalResult {
            facts: Vec::new(),
            report,
        };
    }

    visited.insert(fact_id.to_string());
    to_visit.push_back((fact_id.to_string(), 0usize));

    while let Some((current_id, current_depth)) = to_visit.pop_front() {
        if current_depth > budget.max_depth {
            continue;
        }
        if visited.len() > budget.max_visited_nodes {
            visited_budget_exceeded = true;
            break;
        }

        let Some(fact) = facts_by_id.get(&current_id) else {
            continue;
        };

        if current_depth > 0 {
            results.push(fact.clone());
        }

        if current_depth >= budget.max_depth {
            continue;
        }

        for rel in &fact.relations {
            if traversed_edges >= budget.max_returned_edges {
                edge_budget_exceeded = true;
                break;
            }
            if visited.len() >= budget.max_visited_nodes {
                visited_budget_exceeded = true;
                break;
            }

            traversed_edges += 1;
            if visited.insert(rel.target_id.clone()) {
                to_visit.push_back((rel.target_id.clone(), current_depth + 1));
            }
        }

        if visited_budget_exceeded || edge_budget_exceeded {
            break;
        }
    }

    let mut reasons = Vec::new();
    if depth_hard_capped {
        reasons.push("depth_hard_cap");
    }
    if visited_budget_exceeded {
        reasons.push("visited_nodes_budget");
    }
    if edge_budget_exceeded {
        reasons.push("returned_edges_budget");
    }

    let report = RelationTraversalReport {
        requested_depth: depth,
        effective_max_depth: budget.max_depth,
        visited_nodes: visited.len(),
        traversed_edges,
        returned_facts: results.len(),
        cycle_safe: true,
        depth_hard_capped,
        visited_budget_exceeded,
        edge_budget_exceeded,
        truncated: !reasons.is_empty(),
        truncation_reason: (!reasons.is_empty()).then(|| reasons.join(",")),
    };

    RelationTraversalResult {
        facts: results,
        report,
    }
}

pub fn traverse_related_facts(
    facts_by_id: &std::collections::HashMap<String, Fact>,
    fact_id: &str,
    depth: usize,
) -> Vec<Fact> {
    traverse_related_facts_with_report(facts_by_id, fact_id, depth).facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fact_defaults_to_pending_normal_memory() {
        let fact = Fact::new("likes Rust", "preference");

        assert_eq!(fact.category, "preference");
        assert!(matches!(fact.status, FactStatus::Pending));
        assert!(matches!(fact.protection, FactProtection::Normal));
    }

    #[test]
    fn relation_traversal_is_cycle_safe_and_budgeted() {
        let mut root = Fact::new("root", "test");
        let mut child = Fact::new("child", "test");
        root.id = "root".to_string();
        child.id = "child".to_string();
        root.relations.push(Relation {
            predicate: "links_to".to_string(),
            target_id: child.id.clone(),
            strength: 1.0,
        });
        child.relations.push(Relation {
            predicate: "links_to".to_string(),
            target_id: root.id.clone(),
            strength: 1.0,
        });

        let facts =
            std::collections::HashMap::from([(root.id.clone(), root), (child.id.clone(), child)]);
        let result = traverse_related_facts_with_report(&facts, "root", 2);

        assert_eq!(result.facts.len(), 1);
        assert!(result.report.cycle_safe);
        assert_eq!(result.report.returned_facts, 1);
    }
}
