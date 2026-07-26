use super::{ArtifactDeliveryScope, ArtifactQualityContract};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArtifactContractKind {
    ResearchPaper,
    AnalyticalReport,
    LongformDocument,
    Document,
}

pub(crate) fn infer_quality_contract(
    intent: &str,
    requested_chars: Option<usize>,
    max_chars: Option<usize>,
) -> ArtifactQualityContract {
    infer_quality_contract_for_type(None, intent, requested_chars, max_chars)
}

pub(crate) fn infer_quality_contract_for_type(
    artifact_type: Option<&str>,
    intent: &str,
    requested_chars: Option<usize>,
    max_chars: Option<usize>,
) -> ArtifactQualityContract {
    let kind = artifact_type
        .and_then(explicit_artifact_contract_kind)
        .or_else(|| structured_artifact_contract_kind(intent))
        .unwrap_or(ArtifactContractKind::Document);
    let mut contract = quality_contract_for_kind(kind, requested_chars, max_chars);
    if structured_delivery_scope(intent) == Some(ArtifactDeliveryScope::Stage) {
        let stage_min = structured_positive_usize(
            intent,
            &[
                "step_target_chars:",
                "step_target_chars=",
                "stage_target_chars:",
                "stage_target_chars=",
                "阶段目标字数：",
                "阶段目标字数:",
            ],
        )
        .unwrap_or_else(|| default_min_chars(kind));
        contract.use_stage_delivery(stage_min);
    }
    contract
}

fn quality_contract_for_kind(
    kind: ArtifactContractKind,
    requested_chars: Option<usize>,
    max_chars: Option<usize>,
) -> ArtifactQualityContract {
    match kind {
        ArtifactContractKind::ResearchPaper => ArtifactQualityContract::new(
            "research_paper",
            requested_chars,
            requested_chars.unwrap_or(7_000),
            max_chars,
            2,
            sections(&["摘要", "引言", "方法", "结果", "讨论", "结论", "参考文献"]),
            "research_sections",
            true,
            true,
        ),
        ArtifactContractKind::AnalyticalReport => ArtifactQualityContract::new(
            "analytical_report",
            requested_chars,
            requested_chars.unwrap_or(4_000),
            max_chars,
            1,
            sections(&["摘要", "背景", "分析", "建议", "结论"]),
            "report_sections",
            true,
            true,
        ),
        ArtifactContractKind::LongformDocument => ArtifactQualityContract::new(
            "longform_document",
            requested_chars,
            requested_chars.unwrap_or(5_000),
            max_chars,
            0,
            sections(&["标题", "正文"]),
            "longform_sections",
            true,
            true,
        ),
        ArtifactContractKind::Document => ArtifactQualityContract::new(
            "document",
            requested_chars,
            requested_chars.unwrap_or(800),
            max_chars,
            0,
            sections(&["标题", "正文"]),
            "document_sections",
            true,
            false,
        ),
    }
}

const fn default_min_chars(kind: ArtifactContractKind) -> usize {
    match kind {
        ArtifactContractKind::ResearchPaper => 7_000,
        ArtifactContractKind::AnalyticalReport => 4_000,
        ArtifactContractKind::LongformDocument => 5_000,
        ArtifactContractKind::Document => 800,
    }
}

fn sections(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn explicit_artifact_contract_kind(value: &str) -> Option<ArtifactContractKind> {
    let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        "research_paper" | "paper" | "academic_paper" | "manuscript" | "论文" => {
            Some(ArtifactContractKind::ResearchPaper)
        }
        "analytical_report" | "report" | "briefing" | "white_paper" | "报告" => {
            Some(ArtifactContractKind::AnalyticalReport)
        }
        "longform_document" | "longform" | "novel" | "fiction" | "小说" => {
            Some(ArtifactContractKind::LongformDocument)
        }
        "document" | "written_document" | "article" | "essay" | "文档" | "文章" => {
            Some(ArtifactContractKind::Document)
        }
        _ => None,
    }
}

fn structured_artifact_contract_kind(intent: &str) -> Option<ArtifactContractKind> {
    for marker in [
        "artifact_type:",
        "artifact_type=",
        "document_type:",
        "document_type=",
        "产物类型：",
        "产物类型:",
        "文档类型：",
        "文档类型:",
    ] {
        let Some((_, right)) = intent.split_once(marker) else {
            continue;
        };
        let value = right
            .split(|ch| matches!(ch, '\n' | ',' | '，' | ';' | '；'))
            .next()
            .unwrap_or_default()
            .trim();
        if let Some(kind) = explicit_artifact_contract_kind(value) {
            return Some(kind);
        }
    }
    None
}

fn structured_delivery_scope(intent: &str) -> Option<ArtifactDeliveryScope> {
    for marker in [
        "artifact_scope:",
        "artifact_scope=",
        "delivery_scope:",
        "delivery_scope=",
        "交付范围：",
        "交付范围:",
    ] {
        let Some((_, right)) = intent.split_once(marker) else {
            continue;
        };
        let value = right
            .split(|ch| matches!(ch, '\n' | ',' | '，' | ';' | '；'))
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        return match value.as_str() {
            "stage" | "step" | "partial" | "阶段" | "分步" => {
                Some(ArtifactDeliveryScope::Stage)
            }
            "final" | "complete" | "whole" | "最终" | "完整" => {
                Some(ArtifactDeliveryScope::Final)
            }
            _ => None,
        };
    }
    None
}

fn structured_positive_usize(intent: &str, markers: &[&str]) -> Option<usize> {
    markers.iter().find_map(|marker| {
        let (_, right) = intent.split_once(marker)?;
        right
            .trim_start()
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_language_terms_do_not_override_missing_structured_type() {
        let contract = infer_quality_contract("制定小说创作方案", None, None);
        assert_eq!(contract.artifact_type, "document");
    }

    #[test]
    fn structured_type_is_authoritative() {
        let contract = infer_quality_contract(
            "artifact_type: longform_document\n制定小说创作方案",
            Some(50_000),
            None,
        );
        assert_eq!(contract.artifact_type, "longform_document");
        assert_eq!(contract.final_target_chars, Some(50_000));
        assert_eq!(contract.min_chars, 50_000);
    }

    #[test]
    fn staged_delivery_keeps_final_target_separate_from_current_step() {
        let contract = infer_quality_contract(
            "artifact_type: longform_document\nartifact_scope: stage\nstage_target_chars: 4000",
            Some(500_000),
            None,
        );

        assert_eq!(contract.delivery_scope, ArtifactDeliveryScope::Stage);
        assert_eq!(contract.final_target_chars, Some(500_000));
        assert_eq!(contract.min_chars, 4_000);
    }
}
