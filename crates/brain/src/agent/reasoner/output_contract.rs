use super::reasoner_constants;
use crate::skills::tool::{CapabilityRouteHint, CoordinatorTaskMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputContractKind {
    ShortAnswer,
    Explanation,
    RealtimeLookup,
    ToolExecution,
    Artifact,
    Longform,
    Code,
    FileTransform,
    Document,
    Vision,
}

impl OutputContractKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ShortAnswer => "short_answer",
            Self::Explanation => "explanation",
            Self::RealtimeLookup => "realtime_lookup",
            Self::ToolExecution => "tool_execution",
            Self::Artifact => "artifact",
            Self::Longform => "longform",
            Self::Code => "code",
            Self::FileTransform => "file_transform",
            Self::Document => "document",
            Self::Vision => "vision",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputSurface {
    Chat,
    ToolResult,
    Artifact,
    BackgroundTask,
}

impl OutputSurface {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::ToolResult => "tool_result",
            Self::Artifact => "artifact",
            Self::BackgroundTask => "background_task",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputContract {
    pub(crate) kind: OutputContractKind,
    pub(crate) surface: OutputSurface,
    pub(crate) max_tokens: u64,
    pub(crate) requires_background: bool,
    pub(crate) requires_artifact: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutputContractInput<'a> {
    pub(crate) latest_user_text: Option<&'a str>,
    pub(crate) tools_visible: bool,
    pub(crate) execution_turn: bool,
    pub(crate) direct_capability_route: Option<CapabilityRouteHint>,
    pub(crate) coordinator_task_mode: CoordinatorTaskMode,
    pub(crate) configured_ceiling: Option<u64>,
    pub(crate) max_step_tokens: u64,
}

pub(crate) fn resolve_output_contract(input: OutputContractInput<'_>) -> OutputContract {
    let text = input.latest_user_text.unwrap_or_default().trim();
    let configured_ceiling = input
        .configured_ceiling
        .unwrap_or(input.max_step_tokens)
        .min(input.max_step_tokens)
        .max(1);
    let (kind, surface, requested_tokens, requires_background, requires_artifact) =
        classify_contract(input, text);

    OutputContract {
        kind,
        surface,
        max_tokens: requested_tokens.min(configured_ceiling).max(1),
        requires_background,
        requires_artifact,
    }
}

fn classify_contract(
    input: OutputContractInput<'_>,
    text: &str,
) -> (OutputContractKind, OutputSurface, u64, bool, bool) {
    if input.execution_turn {
        let kind = match input.direct_capability_route {
            Some(CapabilityRouteHint::RealtimeLookup(_)) => OutputContractKind::RealtimeLookup,
            Some(CapabilityRouteHint::Coding) => OutputContractKind::Code,
            Some(CapabilityRouteHint::FileOps) => OutputContractKind::FileTransform,
            Some(CapabilityRouteHint::DocumentUnderstanding) => OutputContractKind::Document,
            Some(CapabilityRouteHint::VisualUnderstanding) => OutputContractKind::Vision,
            _ => OutputContractKind::ToolExecution,
        };
        return (kind, OutputSurface::ToolResult, 2_048, false, false);
    }

    match input.coordinator_task_mode {
        CoordinatorTaskMode::VisionLite => {
            return (
                OutputContractKind::Vision,
                OutputSurface::Chat,
                2_048,
                false,
                false,
            );
        }
        CoordinatorTaskMode::DocumentLite => {
            return (
                OutputContractKind::Document,
                OutputSurface::Chat,
                2_048,
                false,
                false,
            );
        }
        CoordinatorTaskMode::ToolAgent => {
            let kind = match input.direct_capability_route {
                Some(CapabilityRouteHint::Writing) => {
                    if text_looks_like_longform(text) {
                        OutputContractKind::Longform
                    } else {
                        OutputContractKind::Artifact
                    }
                }
                Some(CapabilityRouteHint::Coding) => OutputContractKind::Code,
                Some(CapabilityRouteHint::FileOps) => OutputContractKind::FileTransform,
                Some(CapabilityRouteHint::RealtimeLookup(_)) => OutputContractKind::RealtimeLookup,
                _ if looks_like_artifact_request(text) => OutputContractKind::Artifact,
                _ => OutputContractKind::ToolExecution,
            };
            let background = matches!(
                kind,
                OutputContractKind::Longform | OutputContractKind::Artifact
            );
            return (
                kind,
                if background {
                    OutputSurface::BackgroundTask
                } else {
                    OutputSurface::ToolResult
                },
                if background { 4_096 } else { 2_048 },
                background,
                background,
            );
        }
        CoordinatorTaskMode::ChatLite => {}
    }

    if text_looks_like_longform(text) {
        return (
            OutputContractKind::Longform,
            OutputSurface::BackgroundTask,
            4_096,
            true,
            true,
        );
    }
    if looks_like_artifact_request(text) {
        return (
            OutputContractKind::Artifact,
            OutputSurface::Artifact,
            4_096,
            true,
            true,
        );
    }
    if looks_like_explanation(text) {
        let max_tokens = if looks_like_deep_explanation(text) {
            reasoner_constants::EXPLANATION_MAX_TOKENS
        } else {
            reasoner_constants::BRIEF_EXPLANATION_MAX_TOKENS
        };
        return (
            OutputContractKind::Explanation,
            OutputSurface::Chat,
            max_tokens,
            false,
            false,
        );
    }

    let text_chars = text.chars().count();
    if text_chars > 240 || input.tools_visible {
        (
            OutputContractKind::Explanation,
            OutputSurface::Chat,
            reasoner_constants::EXPLANATION_MAX_TOKENS / 2,
            false,
            false,
        )
    } else {
        (
            OutputContractKind::ShortAnswer,
            OutputSurface::Chat,
            reasoner_constants::SHORT_ANSWER_MAX_TOKENS,
            false,
            false,
        )
    }
}

fn looks_like_explanation(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let ascii_terms = [
        "why",
        "how",
        "explain",
        "compare",
        "analyze",
        "analysis",
        "principle",
        "reason",
        "difference",
        "pros and cons",
    ];
    ascii_terms.iter().any(|term| lowered.contains(term))
        || [
            "为什么",
            "怎么",
            "如何",
            "解释",
            "原理",
            "分析",
            "对比",
            "区别",
            "优缺点",
            "详细",
            "讲讲",
        ]
        .iter()
        .any(|term| text.contains(term))
}

fn looks_like_deep_explanation(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "deep",
        "detailed",
        "in detail",
        "comprehensive",
        "systematic",
        "thorough",
        "long answer",
        "full explanation",
    ]
    .iter()
    .any(|term| lowered.contains(term))
        || [
            "详细",
            "深入",
            "系统",
            "全面",
            "展开",
            "长一点",
            "讲透",
            "完整解释",
        ]
        .iter()
        .any(|term| text.contains(term))
}

fn looks_like_artifact_request(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "pdf", "txt", "markdown", "md", "docx", "file", "export", "save",
    ]
    .iter()
    .any(|term| lowered.contains(term))
        || [
            "保存",
            "导出",
            "文件",
            "文档",
            "入库",
            "知识库",
            "生成",
            "写一篇",
            "写成",
        ]
        .iter()
        .any(|term| text.contains(term))
}

pub(crate) fn text_looks_like_longform(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    if [
        "novel",
        "book-length",
        "longform",
        "long-form",
        "multi-chapter",
        "chapter",
        "chapters",
    ]
    .iter()
    .any(|term| lowered.contains(term))
        || ["小说", "长篇", "章节", "章", "连载", "故事", "剧本"]
            .iter()
            .any(|term| text.contains(term))
    {
        return true;
    }

    contains_large_text_quantity(text)
}

fn contains_large_text_quantity(text: &str) -> bool {
    let mut saw_digit = false;
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            saw_digit = true;
            current.push(ch);
            continue;
        }

        if saw_digit {
            if let Some(value) = current.parse::<f64>().ok() {
                let rest = text
                    .split_once(&current)
                    .map(|(_, tail)| tail)
                    .unwrap_or_default();
                if rest.trim_start().starts_with('万') && value >= 1.0 {
                    return true;
                }
                let rest_lowered = rest.to_ascii_lowercase();
                if value >= 3_000.0
                    && (rest_lowered.trim_start().starts_with("words")
                        || rest_lowered.trim_start().starts_with("word")
                        || rest_lowered.trim_start().starts_with("chars")
                        || rest_lowered.trim_start().starts_with("characters")
                        || rest.trim_start().starts_with('字'))
                {
                    return true;
                }
            }
            current.clear();
            saw_digit = false;
        }
    }
    false
}
