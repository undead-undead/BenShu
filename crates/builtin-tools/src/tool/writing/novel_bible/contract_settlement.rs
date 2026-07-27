use super::model::{
    ApprovedChapterDelta, ChapterStateEventType, CharacterAnchor, StateChangeAllowance,
};
use crate::tool::writing::novel_contract_v2::{CharacterProgressionState, NovelContractV2};

/// Applies only locally validated, approved typed deltas. Display metadata and
/// narrative prose are deliberately excluded from durable contract state.
pub(super) fn apply_approved_chapter(
    contract: &mut NovelContractV2,
    character_ledger: &[CharacterAnchor],
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
        let canonical_character = resolve_character_name(character_ledger, &delta.entity_id);
        match delta.event_type {
            ChapterStateEventType::Relationship => {
                if let Some(index) = resolve_relationship_index(
                    &contract.relationship_ledger,
                    &delta.entity_id,
                    canonical_character,
                    &delta.authority_excerpt,
                    &delta.value,
                ) {
                    let relation = &mut contract.relationship_ledger[index];
                    relation.current_state = delta.value.clone();
                    relation.evidence = evidence;
                    relation.last_changed_chapter = Some(chapter.number);
                    changed = true;
                }
            }
            ChapterStateEventType::Power => {
                let character = canonical_character.unwrap_or(delta.entity_id.trim());
                if character.is_empty() {
                    continue;
                }
                if let Some(state) = contract
                    .power_progression
                    .character_current_levels
                    .iter_mut()
                    .find(|state| state.character == character)
                {
                    state.level = delta.value.clone();
                    state.evidence = evidence;
                    changed = true;
                } else if canonical_character.is_some() {
                    contract.power_progression.character_current_levels.push(
                        CharacterProgressionState {
                            character: character.to_string(),
                            level: delta.value.clone(),
                            evidence,
                        },
                    );
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

fn resolve_relationship_index(
    relationships: &[crate::tool::writing::novel_contract_v2::RelationshipLedgerEntry],
    entity_id: &str,
    canonical_character: Option<&str>,
    authority_excerpt: &str,
    value: &str,
) -> Option<usize> {
    let entity_id = entity_id.trim();
    let canonical_character = canonical_character
        .map(str::trim)
        .filter(|name| !name.is_empty());

    if let Some(index) = unique_matching_index(relationships, |relationship| {
        relationship.characters.join("|") == entity_id
            || relationship.character_ids.join("|") == entity_id
    }) {
        return Some(index);
    }

    let owns_entity =
        |relationship: &crate::tool::writing::novel_contract_v2::RelationshipLedgerEntry| {
            relationship
                .character_ids
                .iter()
                .any(|id| id.trim() == entity_id)
                || relationship.characters.iter().any(|name| {
                    name.trim() == entity_id
                        || canonical_character.is_some_and(|canonical| name.trim() == canonical)
                })
        };
    if let Some(index) = unique_matching_index(relationships, owns_entity) {
        return Some(index);
    }

    let evidence = format!("{} {}", authority_excerpt.trim(), value.trim());
    unique_matching_index(relationships, |relationship| {
        owns_entity(relationship)
            && relationship.characters.iter().any(|name| {
                let name = name.trim();
                !name.is_empty()
                    && Some(name) != canonical_character
                    && name != entity_id
                    && evidence.contains(name)
            })
    })
}

fn unique_matching_index<T>(items: &[T], predicate: impl Fn(&T) -> bool) -> Option<usize> {
    let mut matches = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| predicate(item).then_some(index));
    let index = matches.next()?;
    matches.next().is_none().then_some(index)
}

fn resolve_character_name<'a>(
    character_ledger: &'a [CharacterAnchor],
    entity_id: &str,
) -> Option<&'a str> {
    let entity_id = entity_id.trim();
    character_ledger
        .iter()
        .find(|character| character.id.trim() == entity_id || character.name.trim() == entity_id)
        .map(|character| character.name.trim())
        .filter(|name| !name.is_empty())
}
