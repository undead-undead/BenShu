use super::NovelPhase;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NovelPipelineFacts {
    pub source_intake_requested: bool,
    pub source_ready: bool,
    pub contract_ready: bool,
    pub context_ready: bool,
    pub execution_package_ready: bool,
    pub chapter_exists: bool,
    pub chapter_needs_revision: bool,
    pub chapter_state_repair_required: bool,
    pub audit_passed: bool,
    pub settlement_ready: bool,
    pub truth_validated: bool,
    pub chapter_approved: bool,
    pub snapshot_requested: bool,
    pub snapshot_ready: bool,
    pub export_requested: bool,
    pub export_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NovelTransitionDecision {
    pub next_phase: Option<NovelPhase>,
    pub reason: &'static str,
}

pub(crate) fn next_transition(facts: &NovelPipelineFacts) -> NovelTransitionDecision {
    let decision = if facts.source_intake_requested && !facts.source_ready {
        (
            Some(NovelPhase::SourceIntake),
            "requested source intake is incomplete",
        )
    } else if !facts.contract_ready {
        (
            Some(NovelPhase::StoryContract),
            "the authoritative story contract is not ready",
        )
    } else if facts.chapter_approved {
        if facts.snapshot_requested && !facts.snapshot_ready {
            (
                Some(NovelPhase::Snapshot),
                "the approved chapter requires a checkpoint",
            )
        } else if facts.export_requested && !facts.export_ready {
            (
                Some(NovelPhase::Export),
                "the project requested an export after approval",
            )
        } else {
            (None, "the chapter pipeline is complete")
        }
    } else if !facts.context_ready {
        (
            Some(NovelPhase::ContextPackage),
            "the chapter context package is missing",
        )
    } else if !facts.execution_package_ready {
        (
            Some(NovelPhase::ChapterExecutionPackage),
            "the chapter execution package is missing",
        )
    } else if !facts.chapter_exists {
        (Some(NovelPhase::Draft), "the chapter draft does not exist")
    } else if facts.chapter_state_repair_required {
        (
            Some(NovelPhase::TruthSettlement),
            "the final chapter body requires state repair before approval",
        )
    } else if facts.chapter_needs_revision {
        (
            Some(NovelPhase::Revision),
            "the chapter quality result requires revision",
        )
    } else if !facts.audit_passed {
        (Some(NovelPhase::Audit), "the chapter has not passed audit")
    } else if !facts.settlement_ready {
        (
            Some(NovelPhase::TruthSettlement),
            "the pending truth settlement is missing",
        )
    } else if !facts.truth_validated {
        (
            Some(NovelPhase::TruthValidation),
            "the pending truth settlement is not validated",
        )
    } else {
        (
            Some(NovelPhase::Approval),
            "the chapter is ready for approval",
        )
    };

    NovelTransitionDecision {
        next_phase: decision.0,
        reason: decision.1,
    }
}

#[cfg(test)]
fn transition_is_allowed(from: NovelPhase, to: NovelPhase) -> bool {
    use NovelPhase::*;
    matches!(
        (from, to),
        (SourceIntake, StoryContract)
            | (StoryContract, ContextPackage)
            | (ContextPackage, ChapterExecutionPackage)
            | (ChapterExecutionPackage, Draft)
            | (Draft, Audit | Revision)
            | (Audit, Revision | TruthSettlement)
            | (Revision, Audit | TruthSettlement)
            | (TruthSettlement, TruthValidation)
            | (TruthValidation, TruthSettlement | Approval)
            | (Approval, Snapshot | Export | ContextPackage)
            | (Snapshot, Export | ContextPackage)
            | (Export, ContextPackage)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_until_draft() -> NovelPipelineFacts {
        NovelPipelineFacts {
            contract_ready: true,
            context_ready: true,
            execution_package_ready: true,
            ..Default::default()
        }
    }

    #[test]
    fn actual_chapter_order_places_context_before_execution_package() {
        let facts = NovelPipelineFacts {
            contract_ready: true,
            ..Default::default()
        };
        assert_eq!(
            next_transition(&facts).next_phase,
            Some(NovelPhase::ContextPackage)
        );

        let facts = NovelPipelineFacts {
            context_ready: true,
            ..facts
        };
        assert_eq!(
            next_transition(&facts).next_phase,
            Some(NovelPhase::ChapterExecutionPackage)
        );
    }

    #[test]
    fn revision_is_conditional_and_returns_to_audit() {
        let facts = NovelPipelineFacts {
            chapter_exists: true,
            chapter_needs_revision: true,
            ..ready_until_draft()
        };
        assert_eq!(
            next_transition(&facts).next_phase,
            Some(NovelPhase::Revision)
        );
        assert!(transition_is_allowed(
            NovelPhase::Revision,
            NovelPhase::Audit
        ));
    }

    #[test]
    fn approved_chapter_only_runs_requested_tail_phases() {
        let facts = NovelPipelineFacts {
            chapter_approved: true,
            snapshot_requested: false,
            export_requested: false,
            ..ready_until_draft()
        };
        assert_eq!(next_transition(&facts).next_phase, None);
    }

    #[test]
    fn approval_requires_audit_settlement_and_truth_validation() {
        let mut facts = NovelPipelineFacts {
            chapter_exists: true,
            audit_passed: true,
            ..ready_until_draft()
        };
        assert_eq!(
            next_transition(&facts).next_phase,
            Some(NovelPhase::TruthSettlement)
        );
        facts.settlement_ready = true;
        assert_eq!(
            next_transition(&facts).next_phase,
            Some(NovelPhase::TruthValidation)
        );
        facts.truth_validated = true;
        assert_eq!(
            next_transition(&facts).next_phase,
            Some(NovelPhase::Approval)
        );
    }

    #[test]
    fn state_repair_does_not_route_back_to_prose_revision() {
        let facts = NovelPipelineFacts {
            chapter_exists: true,
            chapter_state_repair_required: true,
            audit_passed: true,
            ..ready_until_draft()
        };
        let decision = next_transition(&facts);
        assert_eq!(decision.next_phase, Some(NovelPhase::TruthSettlement));
        assert!(decision.reason.contains("state repair"));
    }
}
