//! Intervention policy primitives for BenShu.
//!
//! Runtime integration remains in `benshu-brain`; this crate owns pure
//! intervention constants, priority, prompt construction, and trigger shaping.

pub mod prompt;
pub mod types;

pub use prompt::{
    budget_breaker_prompt, metabolic_warning_prompt, reflexion_prompt, status_recap_prompt,
};
pub use types::{intervention_constants, InterventionTrigger, InterventionType};
