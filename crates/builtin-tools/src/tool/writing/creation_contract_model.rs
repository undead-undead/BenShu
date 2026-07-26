//! Typed creation-contract model facade.
//!
//! The data model implementation lives under `creation_contract_model/` so this
//! top-level module remains a stable, thin import surface.

mod core;

pub(crate) use core::*;
