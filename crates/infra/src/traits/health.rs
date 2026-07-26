use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded(String),
    Unhealthy(String),
}

#[async_trait]
pub trait HealthCheck: Send + Sync {
    async fn check_health(&self) -> HealthStatus;
    fn module_name(&self) -> &'static str;
}
