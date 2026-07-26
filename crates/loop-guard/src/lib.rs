//! Loop prevention primitives for BenShu runtime orchestration.
//!
//! This crate owns pure loop-guard policy and tool-call history. Runtime
//! execution, hooks, message metadata, and user-facing recovery stay in
//! `benshu-brain`.

pub mod history;
pub mod policy;

pub use history::{CallRecord, QueryHistory};
pub use policy::{history_constants, LoopAlert, LoopGuardAction, LoopGuardPolicy};
