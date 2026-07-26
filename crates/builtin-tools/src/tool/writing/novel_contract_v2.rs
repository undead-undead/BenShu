//! Novel contract v2 facade.
//!
//! Contract data structures, structural normalization, and read-only rendering
//! live under `novel_contract_v2/` so this top-level module stays a stable
//! import surface. Genre policy is owned by `longform_policy`.

mod core;
mod normalization;
mod rendering;

pub(crate) use core::*;
pub(crate) use rendering::summary_lines;
