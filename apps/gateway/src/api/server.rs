//! Gateway API Server
//!
//! This module is now refactored into a modular structure:
//! - `state`: Application state and error types
//! - `middleware`: Authentication and security guards
//! - `handlers`: API endpoint implementations
//! - `init`: Server startup and routing logic

pub use crate::api::init::start_server;
pub use crate::api::state::{AppError, AppState};
