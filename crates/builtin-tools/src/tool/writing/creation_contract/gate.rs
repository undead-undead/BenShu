//! Creation contract gate result types.
//!
//! The typed contract gate decides readiness. These structs are the transport
//! envelope used by draft lifecycle and gateway adapters; they should not grow
//! their own field-quality rules.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractGateStatus {
    Ready,
    NeedsRepair,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractGateResult {
    pub status: ContractGateStatus,
    pub blocking_issues: Vec<String>,
    pub repairable_issues: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ContractSubmissionOutcome {
    pub gate: ContractGateResult,
    pub committed: bool,
}

impl ContractGateResult {
    pub fn ready() -> Self {
        Self {
            status: ContractGateStatus::Ready,
            blocking_issues: Vec::new(),
            repairable_issues: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.status == ContractGateStatus::Ready
    }

    pub fn actionable_issues(&self) -> Vec<String> {
        self.blocking_issues
            .iter()
            .chain(self.repairable_issues.iter())
            .cloned()
            .collect()
    }
}

impl ContractSubmissionOutcome {
    pub fn is_ready(&self) -> bool {
        self.gate.is_ready() && self.committed
    }
}
