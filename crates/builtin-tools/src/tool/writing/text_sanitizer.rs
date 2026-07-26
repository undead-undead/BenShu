//! Shared text sanitizing primitives for writing tools.
//!
//! This module only removes provider/protocol residue that is unsafe to persist
//! or display as prose. It does not judge contract readiness, naming quality, or
//! chapter literary quality.

const PROVIDER_MARKERS: &[&str] = &[
    "<|channel>thought",
    "<|channel>analysis",
    "<|channel>final",
    "<|channel>",
    "<|channel|>thought",
    "<|channel|>analysis",
    "<|channel|>final",
    "<|channel|>",
    "< | channel>thought",
    "<channel|>",
    "<|/channel|>",
    "<|eot_id|>",
    "<|start_header_id|>",
    "<|end_header_id|>",
    "<|im_start|>",
    "<|im_end|>",
    "<|end|>",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SanitizeReport {
    pub(crate) text: String,
    pub(crate) original_chars: usize,
    pub(crate) sanitized_chars: usize,
    pub(crate) removed_provider_markers: usize,
    pub(crate) removed_lines: usize,
    pub(crate) changed: bool,
    pub(crate) notes: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WritingSanitizeStage {
    ModelOutput,
    ProviderPrompt,
    StreamProgress,
    ChapterBody,
    SavedProse,
    ReadableExport,
}

impl WritingSanitizeStage {
    fn note(self) -> &'static str {
        match self {
            Self::ModelOutput => "model_output",
            Self::ProviderPrompt => "provider_prompt",
            Self::StreamProgress => "stream_progress",
            Self::ChapterBody => "chapter_body",
            Self::SavedProse => "saved_prose",
            Self::ReadableExport => "readable_export",
        }
    }
}

impl SanitizeReport {
    pub(crate) fn from_text(original: &str, text: String) -> Self {
        let original_chars = original.chars().count();
        let sanitized_chars = text.chars().count();
        Self {
            changed: original != text,
            text,
            original_chars,
            sanitized_chars,
            ..Self::default()
        }
    }

    pub(crate) fn note(mut self, note: &'static str) -> Self {
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
        self
    }

    pub(crate) fn with_removed_lines(mut self, removed_lines: usize) -> Self {
        self.removed_lines += removed_lines;
        if removed_lines > 0 {
            self.changed = true;
            self = self.note("removed_lines");
        }
        self
    }

    pub(crate) fn merge(mut self, next: SanitizeReport) -> Self {
        self.removed_provider_markers += next.removed_provider_markers;
        self.removed_lines += next.removed_lines;
        self.changed |= next.changed;
        for note in next.notes {
            if !self.notes.contains(&note) {
                self.notes.push(note);
            }
        }
        self
    }
}

pub(crate) fn sanitize_common_surface_report(
    raw: &str,
    stage: WritingSanitizeStage,
) -> SanitizeReport {
    strip_provider_protocol_markers_report(raw).note(stage.note())
}

pub(crate) fn normalize_newlines(raw: &str) -> String {
    raw.replace("\r\n", "\n").replace('\r', "\n")
}

pub(crate) fn strip_provider_protocol_markers_report(raw: &str) -> SanitizeReport {
    let mut cleaned = normalize_newlines(raw);
    let mut removed_provider_markers = 0usize;
    for marker in PROVIDER_MARKERS {
        removed_provider_markers += cleaned.matches(marker).count();
        cleaned = cleaned.replace(marker, "");
    }
    let mut report = SanitizeReport::from_text(raw, cleaned);
    report.removed_provider_markers = removed_provider_markers;
    if removed_provider_markers > 0 {
        report.changed = true;
        report = report.note("provider_protocol_markers");
    }
    report
}

pub(crate) fn line_starts_with_provider_protocol_marker(line: &str) -> bool {
    let trimmed = line.trim_start();
    PROVIDER_MARKERS
        .iter()
        .any(|marker| trimmed.starts_with(marker))
}
