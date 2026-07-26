use async_trait::async_trait;
use std::time::Duration;

#[async_trait]
pub trait Prunable: Send + Sync {
    fn prune_inactive(&self, timeout: Duration);
}
