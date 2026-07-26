//! Longform artifact guard facade.
//!
//! The implementation lives under `longform_guard/` so the top-level writing
//! module stays thin.

mod core;
mod structure;

#[cfg(test)]
mod tests;

pub(crate) use core::*;
