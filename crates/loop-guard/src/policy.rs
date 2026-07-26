/// Engineering constants for tool history and loop detection.
pub mod history_constants {
    /// Maximum number of tool call records to keep.
    pub const MAX_HISTORY_RECORDS: usize = 50;
    /// Minimum length for a token to be considered in similarity.
    pub const MIN_TOKEN_LENGTH: usize = 2;
    /// Threshold to trigger a loop warning based on call frequency.
    pub const MAX_TOOL_CALL_FREQUENCY: usize = 5;
    /// Expensive external lookup tools should fail fast instead of burning the
    /// whole request timeout on repeated browser/network attempts.
    pub const MAX_LOOKUP_TOOL_CALL_FREQUENCY: usize = 2;
    /// Multiplier for percentage display.
    pub const PERCENT_MULTIPLIER: f64 = 100.0;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopGuardAction {
    Warn,
    ReusePrevious,
    Block,
}

#[derive(Debug, Clone)]
pub struct LoopAlert {
    pub action: LoopGuardAction,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopGuardPolicy {
    pub max_history_records: usize,
    pub min_token_length: usize,
    pub max_tool_call_frequency: usize,
    pub max_lookup_tool_call_frequency: usize,
    pub percent_multiplier: f64,
}

impl Default for LoopGuardPolicy {
    fn default() -> Self {
        Self {
            max_history_records: history_constants::MAX_HISTORY_RECORDS,
            min_token_length: history_constants::MIN_TOKEN_LENGTH,
            max_tool_call_frequency: history_constants::MAX_TOOL_CALL_FREQUENCY,
            max_lookup_tool_call_frequency: history_constants::MAX_LOOKUP_TOOL_CALL_FREQUENCY,
            percent_multiplier: history_constants::PERCENT_MULTIPLIER,
        }
    }
}

impl LoopGuardPolicy {
    pub fn max_frequency_for_tool(self, tool_name: &str) -> usize {
        match tool_name {
            "browser" | "browser_browse" | "web_fetch" | "web_search" => {
                self.max_lookup_tool_call_frequency
            }
            _ => self.max_tool_call_frequency,
        }
    }

    pub fn with_max_history_records(mut self, max_history_records: usize) -> Self {
        self.max_history_records = max_history_records.max(1);
        self
    }

    pub fn with_max_tool_call_frequency(mut self, max_tool_call_frequency: usize) -> Self {
        self.max_tool_call_frequency = max_tool_call_frequency.max(1);
        self
    }

    pub fn with_max_lookup_tool_call_frequency(
        mut self,
        max_lookup_tool_call_frequency: usize,
    ) -> Self {
        self.max_lookup_tool_call_frequency = max_lookup_tool_call_frequency.max(1);
        self
    }
}
