//! Novel runner facade.
//!
//! Prompt construction, parser helpers, and runner output parsing live under
//! `novel_runner/` so this top-level module stays a stable import surface.

mod core;

pub(crate) use core::*;
