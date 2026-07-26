//! Canonical chapter lifecycle semantics used by storage and orchestration.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChapterLifecycleStatus {
    Draft,
    ImportedUnverified,
    NeedsRevision,
    ReviewPassed,
    StateReady,
    StateRepairRequired,
    Approved,
    Rejected,
    Cancelled,
    Unknown,
}

impl ChapterLifecycleStatus {
    pub(crate) fn parse(status: &str) -> Self {
        match status.trim().to_ascii_lowercase().as_str() {
            "draft" | "drafted" | "revised" | "written" => Self::Draft,
            "imported" | "imported_unverified" => Self::ImportedUnverified,
            "needs_revision" => Self::NeedsRevision,
            "audit_passed" | "reviewed_passed" | "review_passed" => Self::ReviewPassed,
            "state_ready" => Self::StateReady,
            "state_repair_required" | "state-degraded" | "state_degraded" => {
                Self::StateRepairRequired
            }
            "approved" | "final" | "accepted" => Self::Approved,
            "rejected" | "discarded" | "deleted" => Self::Rejected,
            "cancelled" => Self::Cancelled,
            _ => Self::Unknown,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::ImportedUnverified => "imported_unverified",
            Self::NeedsRevision => "needs_revision",
            Self::ReviewPassed => "review_passed",
            Self::StateReady => "state_ready",
            Self::StateRepairRequired => "state_repair_required",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
}

pub(crate) fn status_is_approved(status: &str) -> bool {
    ChapterLifecycleStatus::parse(status) == ChapterLifecycleStatus::Approved
}

pub(crate) fn status_is_rejected(status: &str) -> bool {
    matches!(
        ChapterLifecycleStatus::parse(status),
        ChapterLifecycleStatus::Rejected | ChapterLifecycleStatus::Cancelled
    )
}

pub(crate) fn status_requires_state_repair(status: &str) -> bool {
    ChapterLifecycleStatus::parse(status) == ChapterLifecycleStatus::StateRepairRequired
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_is_not_approved() {
        assert_eq!(
            ChapterLifecycleStatus::parse("audit_passed"),
            ChapterLifecycleStatus::ReviewPassed
        );
        assert!(!status_is_approved("audit_passed"));
    }

    #[test]
    fn legacy_terminal_statuses_keep_compatibility() {
        assert!(status_is_approved("final"));
        assert!(status_is_approved("accepted"));
        assert!(status_is_rejected("discarded"));
    }

    #[test]
    fn state_degraded_statuses_route_to_state_repair() {
        assert!(status_requires_state_repair("state_repair_required"));
        assert!(status_requires_state_repair("state-degraded"));
        assert!(!status_is_approved("state_repair_required"));
    }

    #[test]
    fn imported_chapters_remain_explicitly_unverified() {
        assert_eq!(
            ChapterLifecycleStatus::parse("imported"),
            ChapterLifecycleStatus::ImportedUnverified
        );
        assert!(!status_is_approved("imported_unverified"));
    }
}
