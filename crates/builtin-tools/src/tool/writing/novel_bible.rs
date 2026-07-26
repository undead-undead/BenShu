//! Story bible facade.
//!
//! Story truth, hooks, volumes, characters, and rendering live under
//! `novel_bible/` so this top-level module stays thin.

mod contract_settlement;
mod core;
mod model;
mod rendering;

pub(crate) use core::*;
pub(crate) use model::*;
pub(crate) use rendering::*;
