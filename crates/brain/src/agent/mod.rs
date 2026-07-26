#[cfg(not(target_arch = "wasm32"))]
pub mod agent_identity;
#[cfg(not(target_arch = "wasm32"))]
pub mod agent_liaison_impl;
#[cfg(not(target_arch = "wasm32"))]
pub mod agent_runtime_api;
#[cfg(not(target_arch = "wasm32"))]
pub mod attempt;
#[cfg(not(target_arch = "wasm32"))]
pub mod background_runtime;
#[cfg(not(target_arch = "wasm32"))]
pub mod builder;
#[cfg(not(target_arch = "wasm32"))]
pub mod cache;
#[cfg(not(target_arch = "wasm32"))]
pub mod clarification_trace;
#[cfg(not(target_arch = "wasm32"))]
pub mod comm_runtime;
pub mod component;
#[cfg(not(target_arch = "wasm32"))]
pub mod context;
#[cfg(not(target_arch = "wasm32"))]
pub mod core;
#[cfg(not(target_arch = "wasm32"))]
pub mod executor;
#[cfg(not(target_arch = "wasm32"))]
pub mod foreground_runtime;
#[cfg(not(target_arch = "wasm32"))]
pub mod forge_trace;
#[cfg(not(target_arch = "wasm32"))]
pub mod governance;
#[cfg(not(target_arch = "wasm32"))]
pub mod history;
#[cfg(not(target_arch = "wasm32"))]
pub mod intervention;
#[cfg(not(target_arch = "wasm32"))]
pub mod media_runtime_trace;
#[cfg(not(target_arch = "wasm32"))]
pub mod memory;
#[cfg(not(target_arch = "wasm32"))]
pub mod middleware;

pub use memory::{LearnedMemoryInjector, MemoryManager};
pub mod message;
#[cfg(not(target_arch = "wasm32"))]
pub mod meta;
#[cfg(not(target_arch = "wasm32"))]
pub mod multi_agent;
#[cfg(all(feature = "vector-db", not(target_arch = "wasm32")))]
pub mod namespaced_memory;
#[cfg(not(target_arch = "wasm32"))]
pub mod prompt_surface;
pub mod protocol;
#[cfg(not(target_arch = "wasm32"))]
pub mod provider;
#[cfg(not(target_arch = "wasm32"))]
pub mod provider_trace;
pub mod reasoner;
#[cfg(not(target_arch = "wasm32"))]
pub mod run_trace_builder;
#[cfg(not(target_arch = "wasm32"))]
pub mod runtime_context_budget;
#[cfg(not(target_arch = "wasm32"))]
pub mod runtime_contract;
#[cfg(not(target_arch = "wasm32"))]
pub mod runtime_stage_trace;
#[cfg(not(target_arch = "wasm32"))]
pub mod runtime_support;
// Moved to standalone benshu-scheduler crate
// #[cfg(all(feature = "cron", not(target_arch = "wasm32")))]
// pub mod scheduler;
pub mod session;
#[cfg(not(target_arch = "wasm32"))]
pub mod stream_chat_runtime;
#[cfg(not(target_arch = "wasm32"))]
pub mod streaming;
#[cfg(not(target_arch = "wasm32"))]
pub mod tactical;
#[cfg(not(target_arch = "wasm32"))]
pub mod trace_metadata_helpers;
#[cfg(not(target_arch = "wasm32"))]
pub mod truth_verification_policy;
#[cfg(not(target_arch = "wasm32"))]
pub mod windows_native_trace;
// Swarm and orchestration module (gated sub-modules)
#[cfg(not(target_arch = "wasm32"))]
pub mod evolution;
#[cfg(not(target_arch = "wasm32"))]
pub mod layered_agent;

#[cfg(not(target_arch = "wasm32"))]
pub use benshu_inference::{CachePage, InferenceConfig, KvEngine};
#[cfg(not(target_arch = "wasm32"))]
pub use builder::AgentBuilder;
#[cfg(not(target_arch = "wasm32"))]
pub use core::Agent;
#[cfg(not(target_arch = "wasm32"))]
pub use governance::GovernanceContext;
#[cfg(all(feature = "vector-db", not(target_arch = "wasm32")))]
pub use namespaced_memory::{MemoryEntry, NamespacedMemory};
pub use protocol::*;
pub use session::{AgentSession, SessionStatus};
