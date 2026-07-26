//! Reusable compression primitives for BenShu.
//!
//! This crate owns generic output shaping only. Domain-specific decisions stay
//! in their original crates, while those crates can call these primitives to
//! keep truncation, compact JSON, and evidence-friendly output consistent.

pub mod browser;
pub mod command;
pub mod gateway;
pub mod json;
pub mod preview;
pub mod rag;
pub mod text;
pub mod tool;

pub use browser::{render_search_results, SearchResultSummaryItem};
pub use command::{
    command_requests_verbose_output, compress_command_output, interpret_command_outcome,
    CommandCompressionMode, CommandCompressionResult, CommandOutcome, CommandOutcomeKind,
};
pub use gateway::compact_external_error_message;
pub use preview::{preview_text, preview_text_result, preview_text_with_total, PreviewResult};
pub use rag::{
    format_knowledge_result, knowledge_snippet, knowledge_snippet_text, KnowledgeSnippet,
};
pub use text::{
    ellipsize, head_tail, head_tail_with_notice, head_with_notice, line_window, CompressionResult,
    TruncationNotice,
};
pub use tool::{compress_tool_output, ToolOutputCompression};
