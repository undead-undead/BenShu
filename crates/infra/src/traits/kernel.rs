use crate::traits::resource::{AllocationRequest, AllocationResponse};
use async_trait::async_trait;

/// BenShu Kernel Capability Protocol (Hardened)
///
/// This trait provides a scoped, safe interface for tools and sub-agents to
/// interact with the kernel. It enforces the "Single-Direction Physical Dependency"
/// and "Least Privilege" principles by hiding the underlying Service instances.
#[async_trait]
pub trait KernelCapability: Send + Sync {
    /// Request an allocation of system resources (VRAM, RAM, CPU).
    async fn request_resource(&self, request: AllocationRequest) -> AllocationResponse;

    /// Update current resource usage (Phase 10: Runtime Feedback Loop).
    async fn report_usage(&self, agent_id: &str, vram_mb: usize);

    /// Read an encrypted secret. Audited and scoped to the caller's identity.
    async fn get_secret(&self, key: &str) -> anyhow::Result<Option<String>>;

    /// Execute a memory search (RAG) through the kernel's managed pipeline.
    async fn query_memory(&self, query: &str, limit: usize) -> anyhow::Result<String>;

    /// Record a fact into memory (Audited).
    async fn record_fact(&self, fact: &str, category: &str) -> anyhow::Result<()>;

    /// Check if the caller has permission for a specific action.
    fn check_permission(&self, action: &str) -> bool;

    /// Spawns a child agent with restricted parameters.
    /// - restricted: If true, strips high-privilege tool capability (Vault, Audit).
    /// - tool_whitelist: If provided, only allows these specific tools.
    /// - vram_quota_mb: Hard limit for the sub-agent.
    async fn spawn_sub_agent(
        &self,
        role_name: &str,
        restricted: bool,
        tool_whitelist: Option<Vec<String>>,
        vram_quota_mb: usize,
    ) -> anyhow::Result<()>;
}
