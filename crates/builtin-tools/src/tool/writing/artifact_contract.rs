//! Generic file-artifact contract facade for the writing tool.

mod model;
mod prompt;
mod resolver;
mod sanitizer;
mod validator;

pub(crate) use model::*;
pub(crate) use prompt::*;
pub(crate) use resolver::*;
pub(crate) use sanitizer::*;
pub(crate) use validator::*;
