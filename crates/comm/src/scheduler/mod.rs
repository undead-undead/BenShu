use crate::error::SchedulerError;
use crate::protocol::a2a::A2AMessage;
use crate::protocol::CommEnvelope;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod bid;
pub mod discovery;

use bid::{BidInfo, BiddingState};
pub use discovery::{Discovery, LocalDiscovery};

/// Result type for scheduler
type SchedulerResult<T> = std::result::Result<T, SchedulerError>;

/// A2A Scheduler & Rate Limiter
///
/// Core responsibility:
/// - Routing messages based on Address
/// - Maintaining bidding states (stateless, pure sync)
/// - Controlling message flow (Throttling)
pub struct A2AScheduler {
    /// Rate limit map per agent (agent_id -> messages_per_second)
    throttles: Arc<RwLock<HashMap<String, u32>>>,
    /// Rate limit map per tenant (tenant_id -> messages_per_second)
    tenant_throttles: Arc<RwLock<HashMap<String, u32>>>,
    /// Bidding state manager
    pub bidding: BiddingState,
    /// Agent discovery manager
    pub discovery: Arc<dyn Discovery>,
}

impl A2AScheduler {
    /// Create new scheduler instance
    pub fn new() -> Self {
        Self {
            throttles: Arc::new(RwLock::new(HashMap::new())),
            tenant_throttles: Arc::new(RwLock::new(HashMap::new())),
            bidding: BiddingState::new(),
            discovery: Arc::new(LocalDiscovery::new()),
        }
    }

    /// Set message rate limit for an agent
    pub async fn set_throttle(&self, agent_id: &str, limit: u32) {
        let mut throttles = self.throttles.write().await;
        throttles.insert(agent_id.to_string(), limit);
    }

    /// Set message rate limit for a tenant
    pub async fn set_tenant_throttle(&self, tenant_id: &str, limit: u32) {
        let mut throttles = self.tenant_throttles.write().await;
        throttles.insert(tenant_id.to_string(), limit);
    }

    /// Primary entry point for routing and state updates
    pub async fn handle_message(&self, envelope: &CommEnvelope) -> SchedulerResult<()> {
        // 1. Perform rate limiting check
        self.check_throttle(envelope).await?;

        // 2. Perform internal state tracking
        if let Ok(a2a_msg) = serde_json::from_slice::<A2AMessage>(&envelope.payload) {
            match a2a_msg {
                A2AMessage::Announcement(manifest) => {
                    self.discovery.register(manifest).await?;
                }
                A2AMessage::TaskRequest { request_id, .. } => {
                    self.bidding.register_request(request_id).await;
                }
                A2AMessage::Bid {
                    request_id,
                    bidder_id,
                    bid_amount,
                    metadata,
                } => {
                    self.bidding
                        .add_bid(
                            request_id,
                            BidInfo {
                                bidder_id,
                                amount: bid_amount,
                                timestamp: Utc::now(),
                                metadata,
                            },
                        )
                        .await;
                }
                A2AMessage::Result { request_id, .. } => {
                    // Finalize bidding if needed
                    self.bidding.clear_request(&request_id).await;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Execute a routing check for an envelope
    pub async fn check_throttle(&self, envelope: &CommEnvelope) -> SchedulerResult<()> {
        // 1. Agent-level check
        let agent_id = envelope.meta.source.id();
        let throttles = self.throttles.read().await;

        if let Some(limit) = throttles.get(agent_id) {
            if *limit == 0 {
                return Err(SchedulerError::Throttled {
                    agent_id: agent_id.to_string(),
                    limit: *limit,
                });
            }
        }

        // 2. Tenant-level check
        if let Some(tenant_id) = &envelope.meta.tenant_id {
            let tenant_throttles = self.tenant_throttles.read().await;
            if let Some(limit) = tenant_throttles.get(tenant_id) {
                if *limit == 0 {
                    return Err(SchedulerError::Throttled {
                        agent_id: format!("tenant:{}", tenant_id),
                        limit: *limit,
                    });
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Address, Metadata};
    use uuid::Uuid;

    #[tokio::test]
    async fn test_scheduler_bidding_sync() {
        let scheduler = A2AScheduler::new();
        let request_id = Uuid::new_v4().to_string();

        let from = Address::Agent("requester".to_string());

        // 1. Simulate Task Request
        let req_msg = A2AMessage::TaskRequest {
            request_id: request_id.clone(),
            requester_id: "requester".to_string(),
            task_content: "Train Model".to_string(),
            required_capabilities: vec![],
            delegation: None,
        };

        let payload = serde_json::to_vec(&req_msg).unwrap();
        let envelope = CommEnvelope::new(
            Address::System("all".to_string()),
            payload,
            Metadata::new(from.clone()),
        );

        scheduler.handle_message(&envelope).await.unwrap();

        // 2. Simulate Bids from multiple agents
        for i in 1..=3 {
            let bidder_id = format!("agent-{}", i);
            let bid_msg = A2AMessage::Bid {
                request_id: request_id.clone(),
                bidder_id: bidder_id.clone(),
                bid_amount: 10.0 * i as f64,
                metadata: None,
            };

            let bid_payload = serde_json::to_vec(&bid_msg).unwrap();
            let bid_env = CommEnvelope::new(
                from.clone(),
                bid_payload,
                Metadata::new(Address::Agent(bidder_id)),
            );

            scheduler.handle_message(&bid_env).await.unwrap();
        }

        // 3. Verify bidding state
        let bids = scheduler.bidding.get_bids(&request_id).await;
        assert_eq!(bids.len(), 3);
        assert_eq!(bids[0].amount, 10.0);
        assert_eq!(bids[2].bidder_id, "agent-3");
    }
}
