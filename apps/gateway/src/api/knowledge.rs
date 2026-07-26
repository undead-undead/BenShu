use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures::stream::Stream;
// use std::convert::Infallible;
use crate::api::server::AppState;
use benshu_engram::{HybridSearchResult, RetrievalReport};
use benshu_infra::traits::security::{
    QueryProtectionAction, QueryProtectionDecision, QueryProtectionRequest,
};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt as _;

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum SearchEvent {
    #[serde(rename = "fast_result")]
    FastResult(Vec<HybridSearchResult>),
    #[serde(rename = "query_protection")]
    QueryProtection(QueryProtectionDecision),
    #[serde(rename = "slow_result")]
    SlowResult {
        results: Vec<HybridSearchResult>,
        report: RetrievalReport,
    },
    #[serde(rename = "done")]
    Done,
}

pub async fn search_handler(
    State(state): State<AppState>,
    Json(payload): Json<SearchRequest>,
) -> Sse<impl Stream<Item = Result<Event, axum::Error>>> {
    let query = payload.query;
    let protection = state
        .kernel
        .security()
        .protect_query(&QueryProtectionRequest {
            surface: "gateway_knowledge_search".to_string(),
            query: query.clone(),
            requested_limit: 5,
            estimated_cost: None,
            prefers_deep_retrieval: true,
        });

    // Create a channel for sending events
    let (tx, rx) = tokio::sync::mpsc::channel(10);

    let state_clone = state.clone();
    let query_clone = query.clone();
    let tx_clone = tx.clone();
    let protection_clone = protection.clone();

    tokio::spawn(async move {
        if protection_clone.action != QueryProtectionAction::Allow {
            let _ = tx_clone
                .send(SearchEvent::QueryProtection(protection_clone.clone()))
                .await;
        }

        // Fast Track (FTS)
        let fast_task = async {
            if let Ok(results) = state_clone.kernel.search_engine().search(&query_clone, 5) {
                if !results.is_empty() {
                    let _ = tx_clone.send(SearchEvent::FastResult(results)).await;
                }
            }
        };

        // Slow Track (Recursive)
        let slow_task = async {
            if protection_clone.action != QueryProtectionAction::Allow {
                return;
            }
            if let Ok(outcome) = state_clone
                .kernel
                .retriever()
                .search_recursive_with_report(&query_clone, 5)
                .await
            {
                if !outcome.results.is_empty() {
                    let _ = tx_clone
                        .send(SearchEvent::SlowResult {
                            results: outcome.results,
                            report: outcome.report,
                        })
                        .await;
                }
            }
        };

        // Execute in parallel
        tokio::join!(fast_task, slow_task);

        let _ = tx_clone.send(SearchEvent::Done).await;
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|event| {
        Event::default()
            .json_data(event)
            .map_err(|e| axum::Error::new(e))
    });

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}
