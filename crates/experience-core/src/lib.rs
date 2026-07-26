//! Durable task-experience contracts for BenShu.
//!
//! This crate owns the system-experience contract: records about how BenShu
//! successfully or unsuccessfully executed tasks. It deliberately does not store
//! user knowledge, document facts, or conversation memory. Those belong to the
//! knowledge/memory namespaces. Experience records may be projected into a
//! semantic index, but `experience.redb` remains the authority.

pub mod matcher;
pub mod model;
pub mod projection;
pub mod store;

pub use matcher::{rank_experiences, ExperienceMatch, ExperienceQuery};
pub use model::{
    current_time_ms, EvidenceRefs, ExperienceScope, ExperienceStatus, ExperienceStep,
    FailureSignature, PreflightCheck, PreflightKind, TaskExperience, UsageStats,
    DEFAULT_EXPERIENCE_NAMESPACE,
};
pub use projection::{ExperienceIndexProjection, EXPERIENCE_INDEX_NAMESPACE};
pub use store::{ExperienceStore, ExperienceStoreStats};
