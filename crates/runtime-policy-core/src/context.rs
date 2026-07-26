/// Configuration for the context manager.
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum tokens allowed in the context window.
    pub max_tokens: usize,
    /// Maximum number of messages to keep in history.
    pub max_history_messages: usize,
    /// Reserve tokens for the response.
    pub response_reserve: usize,
    /// Whether to emit provider-specific prompt cache-control markers.
    pub enable_cache_control: bool,
    /// Whether to summarize pruned history.
    pub smart_pruning: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 128000,
            max_history_messages: 50,
            response_reserve: 4096,
            enable_cache_control: false,
            smart_pruning: false,
        }
    }
}

/// Context occupancy metrics from the last assembled prompt.
#[derive(Debug, Clone, Default)]
pub struct ContextOccupancyMetrics {
    pub max_window_tokens: usize,
    pub reserved_response_tokens: usize,
    pub safety_margin_tokens: usize,
    pub history_budget_tokens: usize,
    pub static_prefix_tokens: usize,
    pub provisional_background_tokens: usize,
    pub effective_background_tokens: usize,
    pub dynamic_injection_tokens: usize,
    pub selected_history_tokens: usize,
    pub pruned_history_tokens: usize,
    pub estimated_prefix_tokens: usize,
    pub estimated_final_prompt_tokens: usize,
    pub effective_max_history_messages: usize,
    pub selected_history_messages: usize,
    pub pruned_history_messages: usize,
    pub dynamic_injection_messages: usize,
    pub background_message_count: usize,
    pub background_occupancy_ratio: f32,
    pub prompt_occupancy_ratio: f32,
    pub pressure_band: BackgroundPressureBand,
    pub local_provider_mode: bool,
}

/// Background compression pressure derived from prompt occupancy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundPressureBand {
    #[default]
    Normal,
    High,
    Critical,
}

impl BackgroundPressureBand {
    pub fn from_prompt_occupancy_ratio(ratio: f32) -> Self {
        if ratio >= 0.85 {
            Self::Critical
        } else if ratio >= 0.70 {
            Self::High
        } else {
            Self::Normal
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context_config_uses_large_window() {
        let config = ContextConfig::default();

        assert_eq!(config.max_tokens, 128000);
        assert_eq!(config.response_reserve, 4096);
        assert_eq!(config.max_history_messages, 50);
    }

    #[test]
    fn pressure_band_tracks_prompt_occupancy() {
        assert_eq!(
            BackgroundPressureBand::from_prompt_occupancy_ratio(0.69),
            BackgroundPressureBand::Normal
        );
        assert_eq!(
            BackgroundPressureBand::from_prompt_occupancy_ratio(0.70),
            BackgroundPressureBand::High
        );
        assert_eq!(
            BackgroundPressureBand::from_prompt_occupancy_ratio(0.85),
            BackgroundPressureBand::Critical
        );
        assert_eq!(BackgroundPressureBand::Critical.as_str(), "critical");
    }
}
