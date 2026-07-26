use async_trait::async_trait;

use crate::error::Result;
use crate::traits::resource::AcceleratorInfo;

#[async_trait]
pub trait SensoryLiaison: Send + Sync {
    async fn dispatch(&self, request: serde_json::Value) -> Result<serde_json::Value>;

    async fn get_hardware_utilization(&self) -> Result<AcceleratorInfo>;
}
