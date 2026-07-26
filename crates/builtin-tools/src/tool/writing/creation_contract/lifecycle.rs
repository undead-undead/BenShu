//! Creation draft lifecycle authority.
//!
//! This module owns the stable lifecycle labels for session-level writing
//! drafts. Other writing modules may read these states, but state transitions
//! should stay in `creation_contract` lifecycle helpers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreationDraftLifecycleStatus {
    DraftingContract,
    ContractReady,
    Approved,
    Writing,
    Blocked,
    Cleared,
}

impl CreationDraftLifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DraftingContract => "drafting_contract",
            Self::ContractReady => "contract_ready",
            Self::Approved => "approved",
            Self::Writing => "writing",
            Self::Blocked => "blocked",
            Self::Cleared => "cleared",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim() {
            "active" | "drafting_contract" => Some(Self::DraftingContract),
            "contract_ready" => Some(Self::ContractReady),
            "approved" => Some(Self::Approved),
            "writing" => Some(Self::Writing),
            "blocked" => Some(Self::Blocked),
            "cleared" => Some(Self::Cleared),
            _ => None,
        }
    }

    pub fn is_loadable(self) -> bool {
        matches!(
            self,
            Self::DraftingContract
                | Self::ContractReady
                | Self::Approved
                | Self::Writing
                | Self::Blocked
        )
    }
}
