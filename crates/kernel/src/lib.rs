//! AgentOS Kernel (BenShu-KERNEL)
//!
//! The central nervous system of BenShu. Orchestrates boot process,
//! domain synchronization, and system-wide service discovery.

pub mod boot;
pub mod registry;
pub mod service;

pub use boot::{AgentTemplate, KernelBootstrapper};
pub use registry::KernelRegistry;

use std::sync::Arc;

/// The central Kernel handle for the AgentOS
pub struct AgentKernel {
    pub registry: Arc<KernelRegistry>,
}

impl AgentKernel {
    pub fn new(registry: KernelRegistry) -> Self {
        Self {
            registry: Arc::new(registry),
        }
    }
}
