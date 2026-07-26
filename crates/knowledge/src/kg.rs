//! Phase 13: Lightweight Knowledge Graph (KG Lite).
//!
//! Provides entity-relation triple storage backed by an in-memory store.
//! Enables the agent to answer relationship-based queries like "A is B's mentor"
//! that require logical hops beyond flat document retrieval.

use std::collections::HashMap;
use std::sync::Arc;

use benshu_engram::storage::Storage;
use benshu_infra::error::{Error as BrainError, Result as BrainResult};

/// A single entity-relation triple: (subject, predicate, object)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Triple {
    pub subject: Arc<str>,
    pub predicate: Arc<str>,
    pub object: Arc<str>,
    /// Optional metadata (source, confidence, timestamp)
    pub metadata: HashMap<String, String>,
    /// Emotional weight of this relationship
    pub sentiment: Option<String>,
    /// Priority signal (0.0 to 1.0)
    pub urgency: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Entity {
    pub id: Arc<str>,
    pub entity_type: Arc<str>,
    pub properties: HashMap<String, String>,
}

/// A hyperedge: a relationship between N entities (N >= 1)
/// Useful for modeling events like "Meeting with A, B, and C"
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Hyperedge {
    pub id: Arc<str>,
    pub label: Arc<str>,
    pub participants: Vec<Arc<str>>,
    pub properties: HashMap<String, String>,
    pub created_at_ms: i64,
    /// Emotional context of the event
    pub sentiment: Option<String>,
    /// Importance signal
    pub urgency: f32,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KGConfig {
    pub max_triples: usize,
    pub auto_register_entities: bool,
    pub batch_write_size: usize,
    pub source_tag: String,
}

impl Default for KGConfig {
    fn default() -> Self {
        Self {
            max_triples: 500_000,
            auto_register_entities: true,
            batch_write_size: 100,
            source_tag: "jarvis-core".to_string(),
        }
    }
}

pub fn validate_triple(triple: &Triple) -> BrainResult<()> {
    if triple.subject.trim().is_empty()
        || triple.predicate.trim().is_empty()
        || triple.object.trim().is_empty()
    {
        return Err(BrainError::Validation(
            "Triple components cannot be empty".into(),
        ));
    }

    // 1. Component length limits (Prevention of oversized memory allocations)
    if triple.subject.len() > 1024 || triple.predicate.len() > 1024 || triple.object.len() > 1024 {
        return Err(BrainError::Validation(
            "Triple components must be < 1024 characters".into(),
        ));
    }

    // 2. Urgency range validation
    if !(0.0..=1.0).contains(&triple.urgency) {
        return Err(BrainError::Validation(format!(
            "Urgency must be between 0.0 and 1.0 (got {})",
            triple.urgency
        )));
    }

    // 3. Reserved Metadata Key protection
    for (k, _) in &triple.metadata {
        if k.starts_with('_') && !k.starts_with("_sentiment") && !k.starts_with("_urgency") {
            return Err(BrainError::Validation(format!(
                "Metadata keys starting with '_' are reserved (got {})",
                k
            )));
        }
    }

    // \u{1f} is our internal separator in Redb
    let invalid_chars = ['\0', '\n', '\r', '\u{1f}'];
    if triple.subject.contains(invalid_chars)
        || triple.predicate.contains(invalid_chars)
        || triple.object.contains(invalid_chars)
    {
        return Err(BrainError::Validation(
            "Triple contains restricted control characters".into(),
        ));
    }
    Ok(())
}

/// Result of a graph query
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphQueryResult {
    pub triples: Vec<Triple>,
    pub entities: Vec<Entity>,
}

/// Maximum number of triples allowed in memory for Lite KG to prevent OOM.
/// For larger datasets, use a disk-backed graph engine.
pub const MAX_TRIPLES: usize = 100_000;
pub const MAX_HYPEREDGES: usize = 50_000;

/// Lightweight in-memory knowledge graph index.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeGraph {
    /// All triples in the graph
    triples: Vec<Triple>,
    /// Entity registry: id -> Entity
    entities: HashMap<Arc<str>, Entity>,
    /// All hyperedges in the graph
    hyperedges: Vec<Hyperedge>,

    /// Subject index: subject -> [triple indices]
    #[serde(skip)]
    subject_index: HashMap<Arc<str>, Vec<usize>>,
    /// Object index: object -> [triple indices]
    #[serde(skip)]
    object_index: HashMap<Arc<str>, Vec<usize>>,
    /// Participant index: participant -> [hyperedge indices]
    #[serde(skip)]
    participant_index: HashMap<Arc<str>, Vec<usize>>,
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            triples: Vec::with_capacity(1024),
            entities: HashMap::new(),
            hyperedges: Vec::with_capacity(256),
            subject_index: HashMap::new(),
            object_index: HashMap::new(),
            participant_index: HashMap::new(),
        }
    }

    /// Add a triple (relationship) to the graph.
    pub fn add_triple(&mut self, triple: Triple) -> BrainResult<()> {
        if self.triples.len() >= MAX_TRIPLES {
            return Err(BrainError::Internal(format!(
                "KnowledgeGraph limit reached: {} triples",
                MAX_TRIPLES
            )));
        }

        let idx = self.triples.len();
        self.subject_index
            .entry(Arc::clone(&triple.subject))
            .or_default()
            .push(idx);
        self.object_index
            .entry(Arc::clone(&triple.object))
            .or_default()
            .push(idx);

        // Auto-register entities
        if !self.entities.contains_key(&triple.subject) {
            self.entities.insert(
                Arc::clone(&triple.subject),
                Entity {
                    id: Arc::clone(&triple.subject),
                    entity_type: "auto".into(),
                    properties: HashMap::new(),
                },
            );
        }
        if !self.entities.contains_key(&triple.object) {
            self.entities.insert(
                Arc::clone(&triple.object),
                Entity {
                    id: Arc::clone(&triple.object),
                    entity_type: "auto".into(),
                    properties: HashMap::new(),
                },
            );
        }

        self.triples.push(triple);
        Ok(())
    }

    /// Add multiple triples in a batch. More efficient for bulk imports.
    pub fn add_triples_batch(&mut self, triples: Vec<Triple>) -> BrainResult<usize> {
        if self.triples.len() + triples.len() > MAX_TRIPLES {
            return Err(BrainError::Internal(format!(
                "KnowledgeGraph limit exceeded ({} + {} > {})",
                self.triples.len(),
                triples.len(),
                MAX_TRIPLES
            )));
        }

        // 1. Batch validate before mutation
        for t in &triples {
            validate_triple(t)?;
        }

        let start_idx = self.triples.len();
        let count = triples.len();

        // 2. Add to data store
        self.triples.extend(triples);

        // 3. Efficiently update indices
        for (offset, triple) in self.triples[start_idx..].iter().enumerate() {
            let idx = start_idx + offset;
            self.subject_index
                .entry(Arc::clone(&triple.subject))
                .or_default()
                .push(idx);
            self.object_index
                .entry(Arc::clone(&triple.object))
                .or_default()
                .push(idx);

            // Auto-register entities
            if !self.entities.contains_key(&triple.subject) {
                self.entities.insert(
                    Arc::clone(&triple.subject),
                    Entity {
                        id: Arc::clone(&triple.subject),
                        entity_type: "auto".into(),
                        properties: HashMap::new(),
                    },
                );
            }
            if !self.entities.contains_key(&triple.object) {
                self.entities.insert(
                    Arc::clone(&triple.object),
                    Entity {
                        id: Arc::clone(&triple.object),
                        entity_type: "auto".into(),
                        properties: HashMap::new(),
                    },
                );
            }
        }

        Ok(count)
    }

    /// Register an entity in the graph
    pub fn add_entity(&mut self, entity: Entity) {
        self.entities.insert(Arc::clone(&entity.id), entity);
    }
}

pub struct PersistentKnowledgeGraph {
    storage: Arc<dyn Storage>,
    config: KGConfig,
}

impl PersistentKnowledgeGraph {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            config: KGConfig::default(),
        }
    }

    pub fn with_config(storage: Arc<dyn Storage>, config: KGConfig) -> Self {
        Self { storage, config }
    }

    pub fn add_triple(&self, mut triple: Triple) -> BrainResult<()> {
        validate_triple(&triple)?;

        if !triple.metadata.contains_key("source") {
            triple
                .metadata
                .insert("source".to_string(), self.config.source_tag.clone());
        }
        if !triple.metadata.contains_key("timestamp") {
            triple.metadata.insert(
                "timestamp".to_string(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
                    .to_string(),
            );
        }

        if let Some(ref s) = triple.sentiment {
            triple.metadata.insert("_sentiment".to_string(), s.clone());
        }
        triple
            .metadata
            .insert("_urgency".to_string(), triple.urgency.to_string());

        let metadata = serde_json::to_vec(&triple.metadata)
            .map_err(|e| BrainError::Internal(format!("Metadata Serialization failed: {}", e)))?;

        self.storage
            .put_triple(
                &triple.subject,
                &triple.predicate,
                &triple.object,
                &metadata,
            )
            .map_err(|e| BrainError::Internal(e.to_string()))?;

        Ok(())
    }

    pub fn add_triples_batch(&self, triples: Vec<Triple>) -> BrainResult<usize> {
        let mut batch_ready = Vec::with_capacity(triples.len());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        for mut t in triples {
            validate_triple(&t)?;
            if !t.metadata.contains_key("source") {
                t.metadata
                    .insert("source".to_string(), self.config.source_tag.clone());
            }
            if !t.metadata.contains_key("timestamp") {
                t.metadata.insert("timestamp".to_string(), now.clone());
            }

            // Sync emotional metadata into storage blobs
            if let Some(ref s) = t.sentiment {
                t.metadata.insert("_sentiment".to_string(), s.clone());
            }
            t.metadata
                .insert("_urgency".to_string(), t.urgency.to_string());

            let meta =
                serde_json::to_vec(&t.metadata).map_err(|e| BrainError::Internal(e.to_string()))?;

            batch_ready.push((
                t.subject.to_string(),
                t.predicate.to_string(),
                t.object.to_string(),
                meta,
            ));
        }

        let count = batch_ready.len();
        self.storage
            .put_triples_batch(batch_ready)
            .map_err(|e| BrainError::Internal(e.to_string()))?;
        Ok(count)
    }

    pub fn query_subject(&self, subject: &str) -> BrainResult<Vec<Triple>> {
        let results = self
            .storage
            .query_triples(Some(subject), None, None)
            .map_err(|e| BrainError::Internal(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|(s, p, o, meta)| {
                let metadata: HashMap<String, String> =
                    serde_json::from_slice(&meta).unwrap_or_default();
                let sentiment = metadata.get("_sentiment").cloned();
                let urgency = metadata
                    .get("_urgency")
                    .and_then(|u| u.parse::<f32>().ok())
                    .unwrap_or(0.0);

                Triple {
                    subject: Arc::from(s),
                    predicate: Arc::from(p),
                    object: Arc::from(o),
                    metadata,
                    sentiment,
                    urgency,
                }
            })
            .collect())
    }

    pub fn query_2hop(
        &self,
        start: &str,
    ) -> BrainResult<Vec<(Arc<str>, Arc<str>, Arc<str>, Arc<str>)>> {
        let mut results = Vec::new();
        let hop1 = self.query_subject(start)?;
        for t1 in hop1 {
            let hop2 = self.query_subject(&t1.object)?;
            for t2 in hop2 {
                results.push((
                    Arc::clone(&t1.predicate),
                    Arc::clone(&t1.object),
                    Arc::clone(&t2.predicate),
                    Arc::clone(&t2.object),
                ));
            }
        }
        Ok(results)
    }

    pub fn migrate_from_json(&self, old_json: &str) -> BrainResult<usize> {
        let old_kg: KnowledgeGraph = serde_json::from_str(old_json)
            .map_err(|e| BrainError::Internal(format!("Old KG Deserialization failed: {}", e)))?;
        self.add_triples_batch(old_kg.triples)
    }
}

impl KnowledgeGraph {
    pub fn event_to_text(&self, edge: &Hyperedge) -> String {
        let participants = edge
            .participants
            .iter()
            .map(|p| {
                self.get_entity(p)
                    .map(|e| format!("{} ({})", p, e.entity_type))
                    .unwrap_or_else(|| p.to_string())
            })
            .collect::<Vec<_>>()
            .join(", ");

        let props = edge
            .properties
            .iter()
            .filter(|(k, _)| !k.starts_with('_')) // Filter internal slots
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect::<Vec<_>>()
            .join(" | ");

        let sentiment = edge
            .sentiment
            .as_ref()
            .map(|s| format!(" [{}]", s))
            .unwrap_or_default();
        let urgency = if edge.urgency > 0.5 {
            format!(" (Urgency: {:.1})", edge.urgency)
        } else {
            String::new()
        };

        format!(
            "Event: {}{}{} | Participants: [{}] | Details: {}",
            edge.label,
            sentiment,
            urgency,
            participants,
            if props.is_empty() {
                "No extra data"
            } else {
                &props
            }
        )
    }

    pub fn add_hyperedge(&mut self, hyperedge: Hyperedge) -> BrainResult<()> {
        if self.hyperedges.len() >= MAX_HYPEREDGES {
            return Err(BrainError::Internal(format!(
                "KG Hyperedge limit reached ({})",
                MAX_HYPEREDGES
            )));
        }
        let idx = self.hyperedges.len();
        for participant in &hyperedge.participants {
            self.participant_index
                .entry(Arc::clone(participant))
                .or_default()
                .push(idx);
            if !self.entities.contains_key(participant) {
                self.entities.insert(
                    Arc::clone(participant),
                    Entity {
                        id: Arc::clone(participant),
                        entity_type: "auto".into(),
                        properties: HashMap::new(),
                    },
                );
            }
        }
        self.hyperedges.push(hyperedge);
        Ok(())
    }

    pub fn query_participant_events(&self, participant: &str) -> Vec<&Hyperedge> {
        self.participant_index
            .get(participant)
            .map(|indices| indices.iter().map(|&i| &self.hyperedges[i]).collect())
            .unwrap_or_default()
    }

    /// Phase 19.2: Natural Language Event Recall API.
    /// Finds events that involve ALL of the specified participants (intersection).
    /// Used for queries like "Remember meeting about X with Person Y".
    pub fn recall_multi_participant_event(&self, participants: &[&str]) -> Vec<&Hyperedge> {
        if participants.is_empty() || participants.len() > 16 {
            return Vec::new();
        }

        let mut sets: Vec<std::collections::HashSet<usize>> = Vec::new();
        for p in participants {
            if let Some(indices) = self.participant_index.get(*p) {
                sets.push(indices.iter().cloned().collect());
            } else {
                // If any participant is missing, the intersection is empty
                return Vec::new();
            }
        }

        // Fast intersection
        let mut first = sets.remove(0);
        for set in sets {
            first.retain(|idx| set.contains(idx));
            if first.is_empty() {
                return Vec::new();
            }
        }

        // Final sorting for deterministic output and cache-friendly iteration
        let mut final_indices: Vec<_> = first.into_iter().collect();
        final_indices.sort_unstable();

        final_indices
            .into_iter()
            .map(|idx| &self.hyperedges[idx])
            .collect()
    }

    pub fn query_subject(&self, subject: &str) -> Vec<&Triple> {
        self.subject_index
            .get(subject)
            .map(|indices| indices.iter().map(|&i| &self.triples[i]).collect())
            .unwrap_or_default()
    }

    pub fn query_object(&self, object: &str) -> Vec<&Triple> {
        self.object_index
            .get(object)
            .map(|indices| indices.iter().map(|&i| &self.triples[i]).collect())
            .unwrap_or_default()
    }

    /// 2-hop path query: find all entities reachable from `start` via exactly 2 hops.
    /// Optimized with predicate filtering and result limits.
    pub fn query_2hop_filtered(
        &self,
        start: &str,
        p1_filter: Option<&str>,
        max_results: usize,
    ) -> Vec<(Arc<str>, Arc<str>, Arc<str>, Arc<str>)> {
        let mut results = Vec::new();

        // Hop 1: Find all neighbors
        for t1 in self.query_subject(start) {
            if let Some(p) = p1_filter {
                if t1.predicate.as_ref() != p {
                    continue;
                }
            }

            // Hop 2: Find neighbors of neighbors
            for t2 in self.query_subject(&t1.object) {
                if results.len() >= max_results {
                    return results;
                }

                results.push((
                    Arc::clone(&t1.predicate),
                    Arc::clone(&t1.object),
                    Arc::clone(&t2.predicate),
                    Arc::clone(&t2.object),
                ));
            }
        }
        results
    }

    /// Get total entity count
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Optimized: Get metadata and stats for the whole graph.
    pub fn get_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();
        stats.insert("triples".to_string(), self.triples.len());
        stats.insert("entities".to_string(), self.entities.len());
        stats.insert("hyperedges".to_string(), self.hyperedges.len());
        stats.insert(
            "subject_index_size".to_string(),
            self.subject_index.keys().len(),
        );
        stats.insert(
            "object_index_size".to_string(),
            self.object_index.keys().len(),
        );
        stats.insert(
            "participant_index_size".to_string(),
            self.participant_index.keys().len(),
        );
        stats
    }

    /// Debug helper to check index integrity.
    pub fn check_index_consistency(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (entity, indices) in &self.subject_index {
            for &idx in indices {
                if idx >= self.triples.len() {
                    errors.push(format!(
                        "Out-of-bounds index {} for subject {}",
                        idx, entity
                    ));
                } else if self.triples[idx].subject != *entity {
                    errors.push(format!(
                        "Subject mismatch at index {}: expected {}, found {}",
                        idx, entity, self.triples[idx].subject
                    ));
                }
            }
        }
        errors
    }

    pub fn query_2hop(&self, start: &str) -> Vec<(Arc<str>, Arc<str>, Arc<str>, Arc<str>)> {
        let mut results = Vec::new();
        for t1 in self.query_subject(start) {
            for t2 in self.query_subject(&t1.object) {
                results.push((
                    Arc::clone(&t1.predicate),
                    Arc::clone(&t1.object),
                    Arc::clone(&t2.predicate),
                    Arc::clone(&t2.object),
                ));
            }
        }
        results
    }

    pub fn get_entity(&self, id: &str) -> Option<&Entity> {
        self.entities.get(id)
    }

    pub fn remove_entity(&mut self, entity_id: &str) {
        self.triples
            .retain(|t| t.subject.as_ref() != entity_id && t.object.as_ref() != entity_id);
        self.entities.remove(entity_id);
        self.rebuild_indices();
    }

    /// Rebuild and compact indices after mutation or load.
    /// Sorting indices improves cache locality during hop traversals.
    pub fn rebuild_indices(&mut self) {
        self.subject_index.clear();
        self.object_index.clear();
        self.participant_index.clear();

        for (idx, triple) in self.triples.iter().enumerate() {
            self.subject_index
                .entry(Arc::clone(&triple.subject))
                .or_default()
                .push(idx);
            self.object_index
                .entry(Arc::clone(&triple.object))
                .or_default()
                .push(idx);
        }

        for (idx, edge) in self.hyperedges.iter().enumerate() {
            for participant in &edge.participants {
                self.participant_index
                    .entry(Arc::clone(participant))
                    .or_default()
                    .push(idx);
            }
        }

        // Optimization: Sort indices for better memory access patterns
        for indices in self.subject_index.values_mut() {
            indices.sort_unstable();
        }
        for indices in self.object_index.values_mut() {
            indices.sort_unstable();
        }
        for indices in self.participant_index.values_mut() {
            indices.sort_unstable();
        }
    }

    /// Phase 20: Hierarchical Summarization (Distillation).
    pub fn distill_concepts(&mut self) -> BrainResult<usize> {
        let mut concept_count = 0;
        let mut degree_map: HashMap<Arc<str>, usize> = HashMap::new();
        for t in &self.triples {
            *degree_map.entry(Arc::clone(&t.subject)).or_default() += 1;
            *degree_map.entry(Arc::clone(&t.object)).or_default() += 1;
        }
        let total_degree: usize = degree_map.values().sum();
        let avg_degree = if self.entities.is_empty() {
            0
        } else {
            total_degree / self.entities.len()
        };
        let threshold = (avg_degree * 2).max(5);
        let centroids: Vec<Arc<str>> = degree_map
            .into_iter()
            .filter(|(_, d)| *d >= threshold)
            .map(|(id, _)| id)
            .collect();
        for centroid in centroids {
            let concept_id: Arc<str> = format!("concept://summary_of_{}", centroid).into();
            if !self.entities.contains_key(&concept_id) {
                self.add_entity(Entity {
                    id: Arc::clone(&concept_id),
                    entity_type: "AbstractConcept".into(),
                    properties: [("derived_from".to_string(), centroid.to_string())].into(),
                });
                self.add_triple(Triple {
                    subject: Arc::clone(&concept_id),
                    predicate: "summarizes".into(),
                    object: Arc::clone(&centroid),
                    metadata: [("system_generated".to_string(), "true".to_string())].into(),
                    sentiment: None,
                    urgency: 0.1,
                })?;
                concept_count += 1;
            }
        }
        Ok(concept_count)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(&self)
    }
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        let mut graph: Self = serde_json::from_str(json)?;
        graph.rebuild_indices();
        Ok(graph)
    }

    /// Finds entities with similar names/IDs to a given surface form.
    /// Used for NLU reference resolution to map text mentions back to graph entities.
    pub fn lookup_entities_by_surface_form(&self, surface: &str, limit: usize) -> Vec<&Entity> {
        let lower_surface = surface.to_lowercase();
        self.entities
            .values()
            .filter(|e| e.id.to_lowercase().contains(&lower_surface))
            .take(limit)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_graph() -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();
        kg.add_triple(Triple {
            subject: "Alice".into(),
            predicate: "mentor_of".into(),
            object: "Bob".into(),
            metadata: HashMap::new(),
            sentiment: None,
            urgency: 0.0,
        })
        .unwrap();
        kg.add_triple(Triple {
            subject: "Bob".into(),
            predicate: "works_at".into(),
            object: "ACME".into(),
            metadata: HashMap::new(),
            sentiment: None,
            urgency: 0.0,
        })
        .unwrap();
        kg.add_triple(Triple {
            subject: "Alice".into(),
            predicate: "works_at".into(),
            object: "ACME".into(),
            metadata: HashMap::new(),
            sentiment: None,
            urgency: 0.0,
        })
        .unwrap();
        kg.add_triple(Triple {
            subject: "Bob".into(),
            predicate: "knows".into(),
            object: "Charlie".into(),
            metadata: HashMap::new(),
            sentiment: None,
            urgency: 0.0,
        })
        .unwrap();
        kg
    }

    #[test]
    fn test_distillation() {
        let mut kg = KnowledgeGraph::new();
        for i in 0..10 {
            kg.add_triple(Triple {
                subject: "Alice".into(),
                predicate: "knows".into(),
                object: format!("Person_{}", i).into(),
                metadata: HashMap::new(),
                sentiment: None,
                urgency: 0.0,
            })
            .unwrap();
        }
        let count = kg.distill_concepts().unwrap();
        assert!(count >= 1);
        let concept_id = "concept://summary_of_Alice";
        let entity = kg
            .get_entity(concept_id)
            .expect("Concept should be created");
        assert_eq!(entity.entity_type.as_ref(), "AbstractConcept");
        let rels = kg.query_subject(concept_id);
        assert!(rels
            .iter()
            .any(|t| t.predicate.as_ref() == "summarizes" && t.object.as_ref() == "Alice"));
    }
}

#[cfg(test)]
mod persistent_tests {
    use super::*;
    use benshu_engram::storage::InMemoryStorage;

    #[test]
    fn test_persistent_kg_flow() {
        let storage = Arc::new(InMemoryStorage::new());
        let pkg = PersistentKnowledgeGraph::new(storage);
        pkg.add_triple(Triple {
            subject: "Alice".into(),
            predicate: "mentor_of".into(),
            object: "Bob".into(),
            metadata: HashMap::new(),
            sentiment: Some("Positive".into()),
            urgency: 0.1,
        })
        .unwrap();
        pkg.add_triple(Triple {
            subject: "Bob".into(),
            predicate: "works_at".into(),
            object: "ACME".into(),
            metadata: HashMap::new(),
            sentiment: None,
            urgency: 0.0,
        })
        .unwrap();
        let alice_knowledge = pkg.query_subject("Alice").unwrap();
        assert_eq!(alice_knowledge.len(), 1);
        assert_eq!(alice_knowledge[0].object.as_ref(), "Bob");
    }
}
