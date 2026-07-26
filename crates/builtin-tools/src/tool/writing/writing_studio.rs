//! General writing studio facade.
//!
//! Non-fiction/document writing actions live under `writing_studio/` so the
//! top-level writing module stays a stable import surface.

mod core;
mod model;
mod project_lock;
mod quality;
mod storage;

pub use core::*;
