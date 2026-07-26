use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureClass {
    Quality,
    Execution,
    Transport,
    Resource,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureAnalysisInput {
    pub evolution_manager_available: bool,
    pub tool_name_is_empty: bool,
    pub normalized_error_is_empty: bool,
}

pub fn should_enqueue_failure_analysis(input: FailureAnalysisInput) -> bool {
    input.evolution_manager_available
        && !input.tool_name_is_empty
        && !input.normalized_error_is_empty
}

pub fn classify_failure(error: &str) -> FailureClass {
    let normalized = error.trim().to_lowercase();
    if normalized.is_empty() {
        return FailureClass::Unknown;
    }

    let quality_markers = [
        "no response from llm",
        "empty response",
        "missing final answer",
        "placeholder",
        "<|tool_call>",
        "tool_call",
        "did not produce a natural language answer",
        "failed to provide final answer",
        "critique",
    ];
    if quality_markers
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return FailureClass::Quality;
    }

    let transport_markers = [
        "timeout",
        "timed out",
        "connection",
        "rate limit",
        "overloaded",
        "502",
        "503",
        "504",
        "bad gateway",
        "service unavailable",
        "api error",
        "providerapi",
    ];
    if transport_markers
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return FailureClass::Transport;
    }

    let resource_markers = [
        "oom",
        "out of memory",
        "device lost",
        "errordevicelost",
        "vram",
        "memory pressure",
        "resource exhausted",
    ];
    if resource_markers
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return FailureClass::Resource;
    }

    let execution_markers = [
        "cannot be opened",
        "file does not exist",
        "permission denied",
        "tool failed",
        "error executing tool",
        "invalid argument",
        "no such file",
        "unsupported",
    ];
    if execution_markers
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return FailureClass::Execution;
    }

    FailureClass::Unknown
}

pub fn should_trigger_error_reflexion(classification: FailureClass) -> bool {
    matches!(classification, FailureClass::Quality)
}

pub fn retry_allows_reflexion_upgrade(
    retry_count: usize,
    max_reflexion_retries: usize,
    failure_classification: FailureClass,
) -> bool {
    retry_count > 0
        && retry_count <= max_reflexion_retries
        && should_trigger_error_reflexion(failure_classification)
}
