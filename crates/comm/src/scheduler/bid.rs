use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// State of an ongoing task bidding process
#[derive(Debug, Clone)]
pub struct BidInfo {
    pub bidder_id: String,
    pub amount: f64,
    pub timestamp: DateTime<Utc>,
    pub metadata: Option<String>,
}

/// A2A Bidding State Manager
///
/// Responsibility:
/// - Store and sync bids for task requests
/// - Expire old/stale bidding sessions
pub struct BiddingState {
    /// Map of request_id -> list of bids
    active_bids: Arc<RwLock<HashMap<String, Vec<BidInfo>>>>,
    /// Timestamps of when requests were created
    request_times: Arc<RwLock<HashMap<String, DateTime<Utc>>>>,
}

impl BiddingState {
    pub fn new() -> Self {
        Self {
            active_bids: Arc::new(RwLock::new(HashMap::new())),
            request_times: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new task request for bidding
    pub async fn register_request(&self, request_id: String) {
        let mut times = self.request_times.write().await;
        times.insert(request_id, Utc::now());
    }

    /// Add a bid to an existing request
    pub async fn add_bid(&self, request_id: String, bid: BidInfo) {
        let mut bids = self.active_bids.write().await;
        bids.entry(request_id).or_default().push(bid);
    }

    /// Get all bids for a request
    pub async fn get_bids(&self, request_id: &str) -> Vec<BidInfo> {
        let bids = self.active_bids.read().await;
        bids.get(request_id).cloned().unwrap_or_default()
    }

    /// Clear a finished bidding session
    pub async fn clear_request(&self, request_id: &str) {
        let mut bids = self.active_bids.write().await;
        let mut times = self.request_times.write().await;
        bids.remove(request_id);
        times.remove(request_id);
    }
}
