use crate::intent::RetrievalIntent;
use crate::kg::PersistentKnowledgeGraph;
use crate::rag::{Document, VectorStore};
use crate::router::{IntentAnalysisAgent, IntentRouter};
use benshu_infra::error::Result;
use std::sync::Arc;
use tracing::{info, instrument};

/// Phase 20.2: Hierarchical Retrieval Engine (Tiered RAG)
///
/// This engine implements a multi-level search strategy:
/// 1. Intent Routing (L0): Determine where to look.
/// 2. Semantic Search (L1): Vector-based neighbor discovery.
/// 3. Graph Enrichment (L2): Pull related triples for context consistency.
/// 4. Distilled Concept Priority (L3): Boost AbstractConcepts over raw data.
pub struct HierarchicalRetriever {
    pub router: Arc<IntentRouter>,
    pub kg: Arc<PersistentKnowledgeGraph>,
    pub vector_store: Arc<dyn VectorStore>,
}

impl HierarchicalRetriever {
    pub fn new(
        router: Arc<IntentRouter>,
        kg: Arc<PersistentKnowledgeGraph>,
        vector_store: Arc<dyn VectorStore>,
    ) -> Self {
        Self {
            router,
            kg,
            vector_store,
        }
    }

    /// Primary retrieval entry point
    #[instrument(skip(self, agent), fields(query = %query))]
    pub async fn retrieve(
        &self,
        agent: &dyn IntentAnalysisAgent,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Document>> {
        // Step 1: Intent Analysis
        let intent = self.router.analyze(agent, query).await?;
        info!(
            "🔍 Hierarchical Retrieval [Intent: {:?}]",
            intent.primary_intent
        );

        // Step 2: Multi-Tiered Search
        let mut results = match intent.primary_intent {
            RetrievalIntent::Chat => Vec::new(),
            _ => {
                // Semantic Retrieval (L1)
                self.vector_store.search(query, limit).await?
            }
        };

        // Step 3: Graph Enrichment (L2)
        // For each document, if it corresponds to an entity, pull its context
        let mut enriched_context = Vec::new();
        for doc in &results {
            if let Ok(triples) = self.kg.query_subject(&doc.title) {
                for triple in triples {
                    enriched_context.push(format!(
                        "Fact: {} {} {} (Sentiment: {:?})",
                        triple.subject, triple.predicate, triple.object, triple.sentiment
                    ));
                }
            }
        }

        // Step 4: Inject enriched context into the most relevant document
        if !enriched_context.is_empty() && !results.is_empty() {
            let context_str = enriched_context.join("\n");
            results[0].content = format!(
                "{}\n\n### Related Knowledge Graph Context:\n{}",
                results[0].content, context_str
            );
        }

        // Tiered RAG Optimization: If a document has a summary, we could choose to return
        // only the summary to the agent if the context window is tight.
        // For now, we return full documents but prioritize by score.

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        Ok(results.into_iter().take(limit).collect())
    }

    /// Quick retrieval for time-sensitive tasks
    pub async fn retrieve_fast(&self, query: &str) -> Result<Vec<Document>> {
        self.vector_store.search(query, 3).await
    }
}
