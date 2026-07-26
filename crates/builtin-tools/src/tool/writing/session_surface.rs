//! Writing session surface facade.
//!
//! User-visible status rendering, path previews, and session task summaries live
//! under `session_surface/` so this top-level module stays a stable import
//! surface.

mod core;

pub use core::*;
