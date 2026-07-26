use crate::types::intervention_constants;

pub fn budget_breaker_prompt(usage: u32, limit: u32) -> String {
    format!(
        "{}\nCRITICAL: Token budget exhausted ({} / {}). HALTING high-cost tasks.\n\
        IMMEDIATE ACTION: Transition to task finalization and report results.",
        intervention_constants::MARKER_BUDGET,
        usage,
        limit
    )
}

pub fn metabolic_warning_prompt(reasons: &[String]) -> String {
    format!(
        "{}\nSYSTEM METABOLIC WARNING: {} detected. Throttling autonomous expansion.\n\
        ADAPTIVE STRATEGY: Use concise reasoning and minimize tool calls.",
        intervention_constants::MARKER_METABOLIC,
        reasons.join(" & ")
    )
}

pub fn status_recap_prompt(reason: &str) -> String {
    format!(
        "{} ({})\n\
         Context density threshold reached. Provide a concise summary of results and next actions.",
        intervention_constants::MARKER_RECAP,
        reason
    )
}

pub fn reflexion_prompt(error: &str) -> String {
    format!(
        "{}\n\
         EXECUTION ERROR DETECTED: {}\n\
         SYSTEM 2 REFLEXION REQUIRED: PAUSE, analyze root cause, and develop a corrective action plan.",
        intervention_constants::MARKER_REFLEXION,
        error
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_include_stable_markers() {
        assert!(reflexion_prompt("boom").contains(intervention_constants::MARKER_REFLEXION));
        assert!(
            status_recap_prompt("Step threshold").contains(intervention_constants::MARKER_RECAP)
        );
        assert!(metabolic_warning_prompt(&["High CPU".to_string()])
            .contains(intervention_constants::MARKER_METABOLIC));
        assert!(budget_breaker_prompt(10, 9).contains(intervention_constants::MARKER_BUDGET));
    }
}
