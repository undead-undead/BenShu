use serde::{Deserialize, Serialize};
use std::fmt;

/// Engineering constants for the intervention system.
pub mod intervention_constants {
    /// Priority levels for different interventions.
    pub const PRIORITY_REFLEXION: i32 = 100;
    pub const PRIORITY_METABOLIC: i32 = 80;
    pub const PRIORITY_STATUS_RECAP: i32 = 50;
    pub const PRIORITY_BUDGET_BREAKER: i32 = 110;

    /// How many recent messages to check for existing markers.
    pub const RECENT_MESSAGE_CHECK_LIMIT: usize = 3;

    /// Unique markers to identify interventions in history.
    pub const MARKER_REFLEXION: &str = "### SYSTEM 2 REFLEXION LOOP";
    pub const MARKER_RECAP: &str = "### INTERNAL STATUS RECAP";
    pub const MARKER_METABOLIC: &str = "### SYSTEM METABOLIC WARNING";
    pub const MARKER_BUDGET: &str = "### TOKEN BUDGET BREAKER";
}

/// The type of intervention being triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InterventionType {
    Reflexion,
    MetabolicWarning,
    StatusRecap,
    BudgetBreaker,
}

impl fmt::Display for InterventionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reflexion => write!(f, "Reflexion"),
            Self::MetabolicWarning => write!(f, "MetabolicWarning"),
            Self::StatusRecap => write!(f, "StatusRecap"),
            Self::BudgetBreaker => write!(f, "BudgetBreaker"),
        }
    }
}

impl InterventionType {
    pub fn priority(self) -> i32 {
        match self {
            Self::Reflexion => intervention_constants::PRIORITY_REFLEXION,
            Self::MetabolicWarning => intervention_constants::PRIORITY_METABOLIC,
            Self::StatusRecap => intervention_constants::PRIORITY_STATUS_RECAP,
            Self::BudgetBreaker => intervention_constants::PRIORITY_BUDGET_BREAKER,
        }
    }

    pub fn marker(self) -> &'static str {
        match self {
            Self::Reflexion => intervention_constants::MARKER_REFLEXION,
            Self::StatusRecap => intervention_constants::MARKER_RECAP,
            Self::MetabolicWarning => intervention_constants::MARKER_METABOLIC,
            Self::BudgetBreaker => intervention_constants::MARKER_BUDGET,
        }
    }
}

impl PartialOrd for InterventionType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InterventionType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority().cmp(&other.priority())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterventionTrigger {
    pub typ: InterventionType,
    pub prompt: String,
}

impl InterventionTrigger {
    pub fn new(typ: InterventionType, prompt: impl Into<String>) -> Self {
        Self {
            typ,
            prompt: prompt.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_sort_budget_before_reflexion() {
        let mut items = [
            InterventionType::StatusRecap,
            InterventionType::BudgetBreaker,
            InterventionType::Reflexion,
        ];
        items.sort_by(|a, b| b.cmp(a));
        assert_eq!(items[0], InterventionType::BudgetBreaker);
        assert_eq!(items[1], InterventionType::Reflexion);
    }
}
