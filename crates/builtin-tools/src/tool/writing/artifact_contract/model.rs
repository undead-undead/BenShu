#[derive(Clone, Debug)]
pub(crate) struct ArtifactQualityReport {
    pub(crate) artifact_type: String,
    pub(crate) passed: bool,
    pub(crate) blockers: Vec<String>,
    pub(crate) repairable: Vec<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) metrics: Vec<(String, usize)>,
    pub(crate) review_receipt_required: bool,
}

impl ArtifactQualityReport {
    #[cfg(test)]
    pub(crate) fn actionable_issues(&self) -> Vec<String> {
        self.blockers
            .iter()
            .chain(self.repairable.iter())
            .cloned()
            .collect()
    }

    pub(crate) fn should_attempt_revision(&self) -> bool {
        if self.blockers.is_empty() {
            return !self.repairable.is_empty();
        }
        self.blockers.iter().all(|issue| {
            matches!(
                issue.as_str(),
                "artifact_body_is_empty"
                    | "provider_control_token_or_hidden_reasoning_leaked_into_artifact"
            )
        })
    }

    pub(crate) fn to_tool_result_section(&self) -> String {
        let mut lines = vec![
            format!(
                "quality_contract: {}",
                if self.passed { "pass" } else { "fail" }
            ),
            format!("artifact_type: {}", self.artifact_type),
        ];
        if self.passed {
            lines.push("runtime_effect: artifact.quality".to_string());
        }
        if self.review_receipt_required {
            lines.push(format!(
                "quality_review_receipt: {}",
                if self.passed { "pass" } else { "fail" }
            ));
        }
        append_section(&mut lines, "quality_blockers", &self.blockers);
        append_section(&mut lines, "quality_repairable", &self.repairable);
        append_section(&mut lines, "quality_warnings", &self.warnings);
        if !self.metrics.is_empty() {
            lines.push("quality_metrics:".to_string());
            for (name, value) in &self.metrics {
                lines.push(format!("- {name}: {value}"));
            }
        }
        lines.join("\n")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArtifactDeliveryScope {
    Final,
    Stage,
}

impl ArtifactDeliveryScope {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Final => "final",
            Self::Stage => "stage",
        }
    }
}

fn append_section(lines: &mut Vec<String>, label: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    lines.push(format!("{label}:"));
    lines.extend(values.iter().map(|value| format!("- {value}")));
}

#[derive(Clone, Debug)]
pub(crate) struct ArtifactQualityContract {
    pub(crate) artifact_type: String,
    pub(crate) delivery_scope: ArtifactDeliveryScope,
    pub(crate) final_target_chars: Option<usize>,
    pub(crate) min_chars: usize,
    pub(crate) max_chars: Option<usize>,
    pub(crate) min_citations: usize,
    pub(crate) required_sections: Vec<String>,
    pub(crate) required_section_label: String,
    pub(crate) require_title: bool,
    pub(crate) require_review_receipt: bool,
}

impl ArtifactQualityContract {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        artifact_type: impl Into<String>,
        target_chars: Option<usize>,
        min_chars: usize,
        max_chars: Option<usize>,
        min_citations: usize,
        required_sections: Vec<String>,
        required_section_label: impl Into<String>,
        require_title: bool,
        require_review_receipt: bool,
    ) -> Self {
        Self {
            artifact_type: artifact_type.into(),
            delivery_scope: ArtifactDeliveryScope::Final,
            final_target_chars: target_chars,
            min_chars,
            max_chars,
            min_citations,
            required_sections,
            required_section_label: required_section_label.into(),
            require_title,
            require_review_receipt,
        }
    }

    pub(crate) fn use_stage_delivery(&mut self, stage_min_chars: usize) {
        self.delivery_scope = ArtifactDeliveryScope::Stage;
        self.min_chars = stage_min_chars.max(1);
    }
}
