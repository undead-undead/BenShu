use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReflexionUpgradeReason {
    HighComplexity,
    RetryRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflexionUpgradeDecision {
    pub should_upgrade: bool,
    pub reason: Option<ReflexionUpgradeReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflexionUpgradeInput {
    pub current_strategy_is_react: bool,
    pub complexity_score: u16,
    pub retry_count: usize,
    pub max_reflexion_retries: usize,
    pub retry_recovery_eligible: bool,
    pub explicit_image_generation_turn: bool,
    pub has_media_input: bool,
    pub simple_media_understanding: bool,
}

pub fn decide_reflexion_strategy_upgrade(input: ReflexionUpgradeInput) -> ReflexionUpgradeDecision {
    if !input.current_strategy_is_react
        || input.explicit_image_generation_turn
        || input.has_media_input
        || input.simple_media_understanding
    {
        return ReflexionUpgradeDecision {
            should_upgrade: false,
            reason: None,
        };
    }

    if input.retry_recovery_eligible
        && input.retry_count > 0
        && input.retry_count <= input.max_reflexion_retries
    {
        return ReflexionUpgradeDecision {
            should_upgrade: true,
            reason: Some(ReflexionUpgradeReason::RetryRecovery),
        };
    }

    if input.complexity_score > 80 {
        return ReflexionUpgradeDecision {
            should_upgrade: true,
            reason: Some(ReflexionUpgradeReason::HighComplexity),
        };
    }

    ReflexionUpgradeDecision {
        should_upgrade: false,
        reason: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflexionReviewInput {
    pub strategy_is_reflexion: bool,
    pub current_step: usize,
    pub max_steps: usize,
    pub has_media_input: bool,
    pub simple_media_understanding: bool,
}

pub fn should_run_reflexion_review(input: ReflexionReviewInput) -> bool {
    input.strategy_is_reflexion
        && input.current_step < input.max_steps
        && !input.has_media_input
        && !input.simple_media_understanding
}

pub fn extract_reflexion_critique_reason(critique_text: &str) -> Option<String> {
    let upper = critique_text.to_uppercase();
    if upper.contains("[PASSED]") {
        return None;
    }

    let critique_idx = upper.find("[CRITIQUE]")?;
    let marker_len = "[CRITIQUE]".len();
    let reason = critique_text[critique_idx + marker_len..]
        .trim()
        .to_string();
    if reason.is_empty() {
        Some("unspecified critique".to_string())
    } else if !reflexion_reason_has_actionable_issue(&reason) {
        None
    } else {
        Some(reason)
    }
}

fn reflexion_reason_has_actionable_issue(reason: &str) -> bool {
    let mut lowered = reason.to_lowercase();
    for passed_phrase in [
        "no missing steps",
        "missing steps or factual errors",
        "no factual errors",
        "no factual error",
        "factual errors",
        "factual error",
        "no inaccuracies",
        "no inaccuracy",
        "not inaccurate",
        "not wrong",
        "accurate and appropriate",
        "factually correct",
        "correctly states",
        "correctly conveys",
        "without unnecessary elaboration",
        "no errors are present",
        "no error is present",
    ] {
        lowered = lowered.replace(passed_phrase, "");
    }

    [
        "missing",
        "lacks",
        "lack of",
        "inaccurate",
        "incorrect",
        "error",
        "wrong",
        "unsupported",
        "hallucinat",
        "incomplete",
        "not answer",
        "does not answer",
        "contradict",
        "unsafe",
        "too vague",
        "ambiguous",
    ]
    .iter()
    .any(|term| lowered.contains(term))
}
