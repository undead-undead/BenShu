//! Novel pipeline facade.
//!
//! Workflow descriptors and phase contracts live under `novel_pipeline/` so the
//! top-level writing module stays thin.

mod core;
pub(crate) mod lifecycle;
mod transition;

pub(crate) use core::*;
pub(crate) use transition::*;
