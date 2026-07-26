use crate::runtime_contract::TruthVerificationMetadata;
use crate::trace::RunTrace;

#[derive(Debug, Clone)]
pub struct WindowsNativeScorecardFinding {
    pub reason: String,
    pub severe: bool,
}

#[derive(Debug, Clone)]
pub struct TruthVerificationScorecardFinding {
    pub reason: String,
    pub severe: bool,
}

pub fn normalize_verification_value(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_was_separator = false;
    let mut previous_was_lower_or_digit = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() {
                if !normalized.is_empty() && previous_was_lower_or_digit && !previous_was_separator
                {
                    normalized.push('_');
                }
                normalized.push(ch.to_ascii_lowercase());
                previous_was_separator = false;
                previous_was_lower_or_digit = false;
            } else {
                normalized.push(ch.to_ascii_lowercase());
                previous_was_separator = false;
                previous_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
            }
        } else if !normalized.is_empty() && !previous_was_separator {
            normalized.push('_');
            previous_was_separator = true;
            previous_was_lower_or_digit = false;
        }
    }

    normalized.trim_matches('_').to_string()
}

pub fn collect_truth_verification_scorecard_findings(
    run_trace: &RunTrace,
) -> Vec<TruthVerificationScorecardFinding> {
    const ACCEPTED_TRUTH_STATUSES: &[&str] = &["Verified", "NotObserved"];
    const ACCEPTED_OUTCOMES: &[&str] = &["VerificationSucceeded", "VerificationNotRequired"];
    const SEVERE_OUTCOMES: &[&str] = &[
        "VerificationToolUnavailable",
        "VerificationFetchFailed",
        "VerificationSourceInsufficient",
        "VerificationExecutionMissing",
        "VerificationStateMissing",
        "VerificationSkippedByPolicyGap",
    ];
    const SEVERE_SOURCE_POSTURES: &[&str] = &[
        "SourcesRequiredButMissing",
        "SourcesReferencedButNotAttached",
    ];

    let mut findings = Vec::new();
    let metadata = TruthVerificationMetadata::from_map(&run_trace.metadata);

    if let Some(truth_status) = metadata.truth_status {
        if !truth_status.is_empty() && !ACCEPTED_TRUTH_STATUSES.contains(&truth_status) {
            findings.push(TruthVerificationScorecardFinding {
                reason: format!(
                    "verification::truth_status::{}",
                    normalize_verification_value(truth_status)
                ),
                severe: false,
            });
        }
    }

    let verification_domain = metadata
        .verification_domain
        .map(normalize_verification_value)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown_domain".to_string());
    let verification_requirement = metadata.verification_requirement;
    let verification_mode = metadata.verification_mode;

    if let Some(outcome) = metadata.verification_outcome {
        if !outcome.is_empty() && !ACCEPTED_OUTCOMES.contains(&outcome) {
            findings.push(TruthVerificationScorecardFinding {
                reason: format!(
                    "verification::{}::{}",
                    verification_domain,
                    normalize_verification_value(outcome)
                ),
                severe: SEVERE_OUTCOMES.contains(&outcome),
            });
        }
    }

    if matches!(verification_requirement, Some("LocalContextAllowed"))
        && matches!(verification_mode, Some("LocalContextOnly"))
    {
        findings.push(TruthVerificationScorecardFinding {
            reason: "verification::knowledge_fact::local_context_only".to_string(),
            severe: false,
        });
    }

    if let Some(source_posture) = metadata.source_posture {
        if !source_posture.is_empty()
            && !matches!(
                source_posture,
                "SourcesAttached"
                    | "ExecutionEvidenceAttached"
                    | "StateEvidenceAttached"
                    | "NoSourcesRequired"
            )
        {
            findings.push(TruthVerificationScorecardFinding {
                reason: format!(
                    "verification::source_posture::{}",
                    normalize_verification_value(source_posture)
                ),
                severe: SEVERE_SOURCE_POSTURES.contains(&source_posture),
            });
        }
    }

    let cite_required = metadata.verification_cite_required == Some("true");
    let answer_readiness = metadata.verification_answer_readiness;
    let source_posture = metadata.source_posture;
    let source_required_still_missing = cite_required
        && !matches!(
            source_posture,
            Some("SourcesAttached" | "ExecutionEvidenceAttached" | "StateEvidenceAttached")
        )
        && matches!(
            answer_readiness,
            Some("search_results_only" | "verification_pending" | "local_context_only") | None
        );
    if source_required_still_missing {
        findings.push(TruthVerificationScorecardFinding {
            reason: "verification::source_required::still_missing".to_string(),
            severe: true,
        });
    }

    findings
}

pub fn collect_windows_native_scorecard_findings(
    run_trace: &RunTrace,
) -> Vec<WindowsNativeScorecardFinding> {
    const ACTIVE_OUTCOMES: &[&str] = &["windows_native_active", "active", "not_observed"];
    const MILD_OUTCOMES: &[&str] = &[
        "fallback_runtime_active",
        "migrate_to_windows_native_runtime",
        "cpu_fallback_provider_downgrade",
        "cpu_fallback_no_accelerator_route",
        "cpu_fallback_active",
    ];
    const SEVERE_OUTCOMES: &[&str] = &[
        "backend_unlinked",
        "runtime_missing",
        "accelerator_unavailable",
        "validation_only",
        "model_contract_incompatible",
        "accelerator_resource_exhausted",
        "windows_native_provider_execution_failed",
        "windows_native_execution_failed",
    ];

    let mut findings = Vec::new();
    for role in ["embed", "rerank"] {
        let outcome_key = format!("engram_windows_native_{role}_outcome");
        let Some(outcome) = run_trace
            .metadata
            .get(&outcome_key)
            .map(|value| value.trim())
        else {
            continue;
        };
        if outcome.is_empty() || ACTIVE_OUTCOMES.contains(&outcome) {
            continue;
        }

        let severe = SEVERE_OUTCOMES.contains(&outcome);
        let mild = MILD_OUTCOMES.contains(&outcome);
        if severe || mild {
            findings.push(WindowsNativeScorecardFinding {
                reason: format!("windows_native::{role}::{outcome}"),
                severe,
            });
        } else {
            findings.push(WindowsNativeScorecardFinding {
                reason: format!("windows_native::{role}::{outcome}"),
                severe: false,
            });
        }
    }

    findings
}
