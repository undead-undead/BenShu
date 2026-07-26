use super::super::{surface_sanitizer, text_sanitizer};
use async_trait::async_trait;
use benshu_brain::runtime::continuous_task::{
    ContinuousStepRequest, ContinuousStepResult, ContinuousStepRunner, ContinuousTaskContract,
};

const MIN_LONGFORM_STEP_BODY_CHARS: usize = 240;

pub(crate) struct LongformArtifactGuardedRunner<R> {
    inner: R,
    guard: LongformArtifactGuard,
}

impl<R> LongformArtifactGuardedRunner<R> {
    pub(crate) fn new(inner: R, planned_total_steps: usize) -> Self {
        Self {
            inner,
            guard: LongformArtifactGuard::new(planned_total_steps),
        }
    }
}

#[async_trait]
impl<R> ContinuousStepRunner for LongformArtifactGuardedRunner<R>
where
    R: ContinuousStepRunner + Send,
{
    async fn run_step(
        &mut self,
        request: ContinuousStepRequest,
    ) -> anyhow::Result<ContinuousStepResult> {
        let mut result = self.inner.run_step(request.clone()).await?;
        self.guard.repair(&request, &mut result);
        self.guard.validate(&request, &result)?;
        result.summary =
            LongformArtifactGuard::public_checkpoint_summary(&result.output, &request.step.label);
        Ok(result)
    }
}

/// Runtime state for a checkpointed text artifact.
///
/// The guard owns structural invariants only. Story semantics, character
/// authority, naming quality, and prose revision belong to the artifact's typed
/// contract and quality gate.
#[derive(Debug, Clone, Default)]
pub(crate) struct LongformArtifactGuard {
    planned_total_steps: usize,
    locked_title: Option<String>,
    locked_primary_anchor: Option<String>,
    last_next_hook: Option<String>,
}

impl LongformArtifactGuard {
    pub(crate) fn new(planned_total_steps: usize) -> Self {
        Self {
            planned_total_steps,
            ..Self::default()
        }
    }

    pub(crate) fn validate(
        &mut self,
        request: &ContinuousStepRequest,
        result: &ContinuousStepResult,
    ) -> anyhow::Result<()> {
        self.prime_from_contract(request.contract.as_ref());
        let output = result.output.trim();
        if output.is_empty() {
            anyhow::bail!(
                "longform artifact step {} returned empty output",
                request.step.index
            );
        }

        let body_chars = Self::generated_body_char_count(output);
        if Self::should_enforce_body_minimum(request) && body_chars < MIN_LONGFORM_STEP_BODY_CHARS {
            anyhow::bail!(
                "longform artifact step {} returned too little body content ({} chars, minimum {}); output is likely truncated",
                request.step.index,
                body_chars,
                MIN_LONGFORM_STEP_BODY_CHARS
            );
        }
        if Self::should_enforce_body_minimum(request) && !Self::has_continuity_tail(output) {
            anyhow::bail!(
                "longform artifact step {} is missing continuity note or next hook; output is likely truncated",
                request.step.index
            );
        }
        if let Some(reason) = surface_sanitizer::high_confidence_surface_issue(output) {
            anyhow::bail!(
                "longform artifact step {} contains malformed text surface: {}",
                request.step.index,
                reason
            );
        }

        if let Some(total) = Self::extract_declared_progress_total(output) {
            if total != self.planned_total_steps {
                anyhow::bail!(
                    "longform artifact step {} declared progress total {} but the locked plan has {} steps",
                    request.step.index,
                    total,
                    self.planned_total_steps
                );
            }
        }
        if let Some(current) = Self::extract_declared_progress_current(output) {
            if current != request.step.index {
                anyhow::bail!(
                    "longform artifact step {} declared current progress {}",
                    request.step.index,
                    current
                );
            }
        }
        for (ordinal, heading) in Self::content_heading_ordinals(output) {
            if ordinal != request.step.index
                && !Self::content_heading_mentions_ordinal(&heading, request.step.index)
            {
                anyhow::bail!(
                    "longform artifact step {} emitted content heading for step {}: {}",
                    request.step.index,
                    ordinal,
                    heading
                );
            }
        }

        let title = Self::extract_document_title(output);
        let next_hook = Self::extract_next_hook_text(output);
        if let (Some(previous), Some(current)) =
            (self.last_next_hook.as_deref(), next_hook.as_deref())
        {
            if Self::normalize_next_hook_fragment(previous)
                == Self::normalize_next_hook_fragment(current)
            {
                anyhow::bail!(
                    "longform artifact step {} repeated the prior next hook",
                    request.step.index
                );
            }
        }

        if self.locked_title.is_none() {
            let title = title.ok_or_else(|| {
                anyhow::anyhow!(
                    "longform artifact first step must establish a document title/identity block"
                )
            })?;
            if Self::title_is_placeholder(&title) {
                anyhow::bail!("longform artifact title is a placeholder: {}", title);
            }
            self.locked_title = Some(title);
        } else if let (Some(locked), Some(declared)) = (self.locked_title.as_deref(), title) {
            if declared != locked {
                anyhow::bail!(
                    "longform artifact step {} attempted to rename title from '{}' to '{}'",
                    request.step.index,
                    locked,
                    declared
                );
            }
        }

        if self.locked_primary_anchor.is_none() {
            self.locked_primary_anchor = Self::extract_labeled_primary_anchor(output);
        } else if let (Some(locked), Some(declared)) = (
            self.locked_primary_anchor.as_deref(),
            Self::extract_labeled_primary_anchor(output),
        ) {
            if declared != locked {
                anyhow::bail!(
                    "longform artifact step {} declared primary subject '{}' but the contract locks '{}'",
                    request.step.index,
                    declared,
                    locked
                );
            }
        }

        self.last_next_hook = next_hook;
        Ok(())
    }

    fn prime_from_contract(&mut self, contract: Option<&ContinuousTaskContract>) {
        let Some(contract) = contract else {
            return;
        };
        if self.locked_title.is_none() {
            self.locked_title =
                Self::contract_anchor(contract, "locked_title").and_then(Self::normalize_title);
        }
        if self.locked_primary_anchor.is_none() {
            self.locked_primary_anchor = Self::contract_anchor(contract, "locked_primary_anchor")
                .and_then(Self::normalize_primary_anchor);
        }
        if self.last_next_hook.is_none() {
            self.last_next_hook = Self::contract_anchor(contract, "last_next_hook")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
        }
    }

    fn contract_anchor<'a>(contract: &'a ContinuousTaskContract, name: &str) -> Option<&'a str> {
        contract
            .anchors
            .iter()
            .find(|anchor| anchor.name == name)
            .map(|anchor| anchor.value.as_str())
    }

    pub(crate) fn repair(
        &self,
        request: &ContinuousStepRequest,
        result: &mut ContinuousStepResult,
    ) {
        let report = text_sanitizer::sanitize_common_surface_report(
            &result.output,
            text_sanitizer::WritingSanitizeStage::ModelOutput,
        );
        if report.changed {
            result.output = report.text;
            result.summary = format!("{}; stripped provider protocol residue", result.summary);
        }

        let repaired =
            Self::repair_declared_progress_total(&result.output, self.planned_total_steps);
        if repaired != result.output {
            result.output = repaired;
            result.summary = format!(
                "{}; repaired declared progress total from locked runtime plan",
                result.summary
            );
        }

        if request.step.index > 1 {
            let stripped = Self::strip_nonfirst_document_identity_blocks(&result.output);
            if stripped != result.output {
                result.output = stripped;
                result.summary = format!(
                    "{}; stripped repeated document identity metadata",
                    result.summary
                );
            }
            if let (Some(locked), Some(candidate)) = (
                self.locked_title.as_deref(),
                Self::extract_document_title(&result.output),
            ) {
                if candidate != locked {
                    let downgraded = Self::downgrade_stray_document_title_to_step_heading(
                        &result.output,
                        &candidate,
                        request.step.index,
                    );
                    if downgraded != result.output {
                        result.output = downgraded;
                        result.summary = format!(
                            "{}; treated stray document title as a step heading",
                            result.summary
                        );
                    }
                }
            }
        }

        result.summary = text_sanitizer::sanitize_common_surface_report(
            &result.summary,
            text_sanitizer::WritingSanitizeStage::StreamProgress,
        )
        .text;
    }
}
