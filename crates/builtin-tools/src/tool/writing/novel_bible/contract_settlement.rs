use super::model::{ApprovedChapterDelta, ChapterStateEventType, StateChangeAllowance};
use crate::tool::writing::novel_contract_v2::NovelContractV2;

/// Applies only locally validated, approved typed deltas. Display metadata and
/// narrative prose are deliberately excluded from durable contract state.
pub(super) fn apply_approved_chapter(
    contract: &mut NovelContractV2,
    chapter: &ApprovedChapterDelta,
) {
    let mut changed = false;
    for delta in &chapter.state_changes {
        if !matches!(
            delta.allowance,
            StateChangeAllowance::Contract | StateChangeAllowance::BoundedIncidental
        ) {
            continue;
        }
        let evidence = format!(
            "chapter {} chars {}..{}",
            chapter.number, delta.evidence.start_char, delta.evidence.end_char
        );
        match delta.event_type {
            ChapterStateEventType::Relationship => {
                if let Some(relation) = contract.relationship_ledger.iter_mut().find(|relation| {
                    relation
                        .characters
                        .iter()
                        .any(|name| name == &delta.entity_id)
                        || relation.characters.join("|") == delta.entity_id
                }) {
                    relation.current_state = delta.value.clone();
                    relation.evidence = evidence;
                    relation.last_changed_chapter = Some(chapter.number);
                    changed = true;
                }
            }
            ChapterStateEventType::Power => {
                if let Some(state) = contract
                    .power_progression
                    .character_current_levels
                    .iter_mut()
                    .find(|state| state.character == delta.entity_id)
                {
                    state.level = delta.value.clone();
                    state.evidence = evidence;
                    changed = true;
                }
            }
            ChapterStateEventType::Resource => {
                if let Some(artifact) = contract
                    .artifact_ledger
                    .iter_mut()
                    .find(|artifact| artifact.name == delta.entity_id)
                {
                    artifact.last_seen_chapter = Some(chapter.number);
                    changed = true;
                }
            }
            ChapterStateEventType::HookSeed
            | ChapterStateEventType::HookAdvance
            | ChapterStateEventType::HookPayOff
            | ChapterStateEventType::HookDefer => {
                if let Some(payoff) = contract.payoff_matrix.iter_mut().find(|payoff| {
                    payoff.promise == delta.entity_id || payoff.payoff_target == delta.entity_id
                }) {
                    if !payoff.evidence.iter().any(|item| item == &evidence) {
                        payoff.evidence.push(evidence);
                    }
                    payoff.introduced_chapter.get_or_insert(chapter.number);
                    payoff.status = match delta.event_type {
                        ChapterStateEventType::HookPayOff => "paid_off",
                        ChapterStateEventType::HookAdvance => "advancing",
                        ChapterStateEventType::HookDefer => "deferred",
                        _ => "seeded",
                    }
                    .to_string();
                    if delta.event_type == ChapterStateEventType::HookPayOff {
                        payoff.payoff_chapter.get_or_insert(chapter.number);
                    }
                    changed = true;
                }
            }
            ChapterStateEventType::Character
            | ChapterStateEventType::World
            | ChapterStateEventType::Incidental => {}
        }
    }
    if changed {
        contract.bump_revision();
    }
}
