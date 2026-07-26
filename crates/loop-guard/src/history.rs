use crate::policy::{LoopAlert, LoopGuardAction, LoopGuardPolicy};
use std::collections::{HashMap, HashSet};
use tracing::{debug, warn};

/// Represents a record of a tool call.
#[derive(Debug, Clone)]
pub struct CallRecord {
    pub tool_name: String,
    pub input: String,
}

/// Tracks history of tool calls to detect repeating patterns.
#[derive(Debug, Clone)]
pub struct QueryHistory {
    records: Vec<CallRecord>,
    counts: HashMap<String, usize>,
    policy: LoopGuardPolicy,
}

impl Default for QueryHistory {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            counts: HashMap::new(),
            policy: LoopGuardPolicy::default(),
        }
    }
}

impl QueryHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_policy(policy: LoopGuardPolicy) -> Self {
        Self {
            records: Vec::new(),
            counts: HashMap::new(),
            policy,
        }
    }

    pub fn policy(&self) -> LoopGuardPolicy {
        self.policy
    }

    /// Add a call to the history with retention policy.
    pub fn record(&mut self, tool_name: String, input: String) {
        let count = self.counts.entry(tool_name.clone()).or_insert(0);
        *count += 1;
        self.records.push(CallRecord { tool_name, input });

        if self.records.len() > self.policy.max_history_records {
            let removed = self.records.remove(0);
            if let Some(c) = self.counts.get_mut(&removed.tool_name) {
                if *c > 0 {
                    *c -= 1;
                }
            }
        }
    }

    /// Calculate Jaccard similarity between two strings based on word overlap.
    pub fn calculate_similarity(s1: &str, s2: &str) -> f64 {
        Self::calculate_similarity_with_min_token_length(
            s1,
            s2,
            LoopGuardPolicy::default().min_token_length,
        )
    }

    fn calculate_similarity_with_min_token_length(
        s1: &str,
        s2: &str,
        min_token_length: usize,
    ) -> f64 {
        let tokens1: HashSet<_> = s1
            .split_whitespace()
            .map(|s| s.to_lowercase().replace(|c: char| !c.is_alphanumeric(), ""))
            .filter(|s| s.len() > min_token_length)
            .collect();

        let tokens2: HashSet<_> = s2
            .split_whitespace()
            .map(|s| s.to_lowercase().replace(|c: char| !c.is_alphanumeric(), ""))
            .filter(|s| s.len() > min_token_length)
            .collect();

        if tokens1.is_empty() && tokens2.is_empty() {
            return 1.0;
        }

        let intersection: HashSet<_> = tokens1.intersection(&tokens2).collect();
        let union: HashSet<_> = tokens1.union(&tokens2).collect();

        intersection.len() as f64 / union.len() as f64
    }

    fn canonicalize_for_exact_match(input: &str) -> String {
        input.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Check if a tool call is too similar to previous calls of the same tool.
    ///
    /// The guard is intentionally progress-oriented: exact repeats first reuse
    /// an existing result instead of being treated as runtime failure. Hard
    /// blocks are reserved for severe stagnation.
    pub fn check_loop(&self, tool_name: &str, input: &str, threshold: f64) -> Option<LoopAlert> {
        let call_count = self.get_count(tool_name);
        let max_frequency = self.policy.max_frequency_for_tool(tool_name);
        if call_count >= max_frequency.saturating_mul(2) {
            return Some(LoopAlert {
                action: LoopGuardAction::Block,
                message: format!(
                    "CRITICAL: '{}' has been called {} times in this session. \
                    This indicates a plan stagnation. You MUST stop and re-think: \
                    are you using the wrong tool, or misinterpreting the error/result?",
                    tool_name, call_count
                ),
            });
        }
        if call_count >= max_frequency {
            return Some(LoopAlert {
                action: LoopGuardAction::Warn,
                message: format!(
                    "WARNING: '{}' has already been called {} times in this task. \
                    Continue only if this call has changed arguments or is expected to produce new information.",
                    tool_name, call_count
                ),
            });
        }

        if let Some(previous) = self
            .records
            .iter()
            .rev()
            .find(|record| record.tool_name == tool_name)
        {
            if Self::canonicalize_for_exact_match(&previous.input)
                == Self::canonicalize_for_exact_match(input)
            {
                warn!(tool = %tool_name, "Detected exact repeated tool invocation");
                return Some(LoopAlert {
                    action: LoopGuardAction::ReusePrevious,
                    message: format!(
                        "CRITICAL: This call to '{}' exactly repeats the most recent invocation. \
                        Reuse the existing result instead of hammering the same tool with the same arguments. \
                        If the existing result is insufficient, change the arguments or choose a different next step.",
                        tool_name
                    ),
                });
            }
        }

        for record in self.records.iter().rev() {
            if record.tool_name == tool_name {
                let similarity = Self::calculate_similarity_with_min_token_length(
                    &record.input,
                    input,
                    self.policy.min_token_length,
                );
                if similarity >= threshold {
                    debug!(tool = %tool_name, similarity = %similarity, "Detected potential loop call");
                    return Some(LoopAlert {
                        action: LoopGuardAction::Warn,
                        message: format!(
                            "WARNING: This call to '{}' is {:.0}% similar to a previous call. \
                            Repeating highly similar actions may indicate a logic loop. \
                            Prefer changing parameters, reusing the prior result, or choosing the next step more narrowly.",
                            tool_name,
                            similarity * self.policy.percent_multiplier
                        ),
                    });
                }
            }
        }
        None
    }

    pub fn get_count(&self, tool_name: &str) -> usize {
        *self.counts.get(tool_name).unwrap_or(&0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_similarity() {
        let s1 = "Search for Apple stock price in 2024";
        let s2 = "Search for Apple stock price 2024";
        let sim = QueryHistory::calculate_similarity(s1, s2);
        assert!(sim > 0.8);

        let s3 = "Get latest news about Tesla";
        let sim2 = QueryHistory::calculate_similarity(s1, s3);
        assert!(sim2 < 0.2);
    }

    #[test]
    fn test_loop_detection() {
        let mut history = QueryHistory::new();
        history.record("search".to_string(), "Apple stock".to_string());

        let result = history.check_loop("search", "Apple stock price", 0.6);
        assert!(matches!(
            result.as_ref().map(|alert| alert.action),
            Some(LoopGuardAction::Warn)
        ));

        let result2 = history.check_loop("search", "Tesla news", 0.6);
        assert!(result2.is_none());
    }

    #[test]
    fn test_exact_repeat_reuses_previous() {
        let mut history = QueryHistory::new();
        history.record(
            "delegate".to_string(),
            "{\"role\":\"researcher\"}".to_string(),
        );

        let result = history.check_loop("delegate", "{\"role\":\"researcher\"}", 0.8);
        assert!(matches!(
            result.as_ref().map(|alert| alert.action),
            Some(LoopGuardAction::ReusePrevious)
        ));
    }

    #[test]
    fn lookup_tools_warn_before_hard_blocking_frequency() {
        let mut history = QueryHistory::new();
        history.record(
            "web_search".to_string(),
            "{\"query\":\"lancet heart disease 2026\"}".to_string(),
        );
        history.record(
            "web_search".to_string(),
            "{\"query\":\"lancet cardiovascular treatment 2026\"}".to_string(),
        );

        let result = history.check_loop(
            "web_search",
            "{\"query\":\"site:thelancet.com heart failure treatment\"}",
            0.8,
        );
        assert!(matches!(
            result.as_ref().map(|alert| alert.action),
            Some(LoopGuardAction::Warn)
        ));
    }

    #[test]
    fn policy_can_tune_lookup_frequency() {
        let policy = LoopGuardPolicy::default().with_max_lookup_tool_call_frequency(3);
        let mut history = QueryHistory::with_policy(policy);
        history.record("browser_browse".to_string(), "one".to_string());
        history.record("browser_browse".to_string(), "two".to_string());

        assert!(history
            .check_loop("browser_browse", "three", 0.99)
            .is_none());
    }

    #[test]
    fn compound_tool_actions_are_counted_independently() {
        let mut history = QueryHistory::new();
        for action in [
            "init_project",
            "add_source",
            "set_contract",
            "run_next_chapter",
            "plan_chapter",
            "compose_context",
            "architect_chapter",
            "write_draft",
        ] {
            history.record(
                format!("novel_studio::{action}"),
                format!("{{\"action\":\"{action}\"}}"),
            );
        }

        assert!(history
            .check_loop(
                "novel_studio::audit_chapter",
                "{\"action\":\"audit_chapter\"}",
                0.8
            )
            .is_none());
    }
}
