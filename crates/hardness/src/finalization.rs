use crate::failure::FailureClass;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinalizationFallbackKind {
    QualityNoAnswer,
    MediaUnderstandingRetryHint,
    TransportUnavailable,
    ResourcePressure,
    ExecutionFailure,
    UnknownFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizationFallbackInput {
    pub failure_classification: FailureClass,
    pub has_media_input: bool,
    pub simple_media_understanding: bool,
}

pub fn decide_finalization_fallback(
    input: FinalizationFallbackInput,
) -> Option<FinalizationFallbackKind> {
    match input.failure_classification {
        FailureClass::Quality if input.has_media_input && input.simple_media_understanding => {
            Some(FinalizationFallbackKind::MediaUnderstandingRetryHint)
        }
        FailureClass::Quality => Some(FinalizationFallbackKind::QualityNoAnswer),
        FailureClass::Transport => Some(FinalizationFallbackKind::TransportUnavailable),
        FailureClass::Resource => Some(FinalizationFallbackKind::ResourcePressure),
        FailureClass::Execution => Some(FinalizationFallbackKind::ExecutionFailure),
        FailureClass::Unknown => Some(FinalizationFallbackKind::UnknownFailure),
    }
}
