//! Writing-domain tools.
//!
//! Keep writing project tools together so the writer worker can own written
//! artifacts without mixing that surface into coder or generic file tools.

pub(crate) mod artifact_contract;
pub(crate) mod chapter_quality;
pub(crate) mod contract_semantic_review;
pub mod creation_contract;
pub(crate) mod creation_contract_model;
pub(crate) mod creation_contract_normalizer;
pub(crate) mod intent_policy;
pub(crate) mod longform_guard;
pub(crate) mod longform_policy;
pub(crate) mod naming;
pub(crate) mod novel_bible;
pub(crate) mod novel_contract_v2;
pub(crate) mod novel_governance;
pub(crate) mod novel_pipeline;
pub(crate) mod novel_runner;
pub mod novel_studio;
pub mod novel_workflow_driver;
pub(crate) mod path_recovery;
pub(crate) mod policy;
pub mod session_route;
pub mod session_surface;
pub(crate) mod surface_sanitizer;
pub(crate) mod text_sanitizer;
pub(crate) mod typed_contract_gate;
pub mod writing_studio;

pub use novel_studio::NovelStudioTool;
pub use writing_studio::WritingStudioTool;
