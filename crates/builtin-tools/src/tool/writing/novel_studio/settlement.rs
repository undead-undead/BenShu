use super::*;
use sha2::{Digest, Sha256};

const REQUIRED_END_STATE_AUTHORITY_PATH: &str = "chapter_contract.new_state_after_chapter";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettlementSource {
    FinalBodyObserver,
    ObserverDegraded,
}

impl SettlementSource {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::FinalBodyObserver => "final_body_observer",
            Self::ObserverDegraded => "observer_degraded",
        }
    }
}

pub(super) fn zero_change_degraded_settlement(
    chapter: &ChapterRecord,
    body: &str,
    authority_fingerprint: &str,
    reason: &str,
) -> SettlementOutput {
    SettlementOutput {
        chapter_fingerprint: chapter_revision_fingerprint(chapter, body),
        body_fingerprint: chapter_quality::chapter_body_fingerprint(body),
        authority_fingerprint: authority_fingerprint.to_string(),
        state_changes: Vec::new(),
        degraded_reason: reason.trim().to_string(),
        current_state: String::new(),
        pending_hooks: String::new(),
        chapter_summary: String::new(),
        continuity_updates: Vec::new(),
        resolved_hooks: Vec::new(),
    }
}

pub(super) fn validated_settlement_from_final_body(
    raw_observation: &str,
    body: &str,
    chapter: &ChapterRecord,
    authority: &governance::SealedChapterAuthority,
) -> (
    SettlementOutput,
    StateValidationOutput,
    SettlementSource,
    Option<String>,
) {
    validated_settlement_from_final_body_with_recovery(
        raw_observation,
        body,
        chapter,
        authority,
        false,
    )
}

pub(super) fn validated_settlement_from_final_body_after_observer_exhaustion(
    raw_observation: &str,
    body: &str,
    chapter: &ChapterRecord,
    authority: &governance::SealedChapterAuthority,
) -> (
    SettlementOutput,
    StateValidationOutput,
    SettlementSource,
    Option<String>,
) {
    validated_settlement_from_final_body_with_recovery(
        raw_observation,
        body,
        chapter,
        authority,
        true,
    )
}

fn validated_settlement_from_final_body_with_recovery(
    raw_observation: &str,
    body: &str,
    chapter: &ChapterRecord,
    authority: &governance::SealedChapterAuthority,
    recover_required_state: bool,
) -> (
    SettlementOutput,
    StateValidationOutput,
    SettlementSource,
    Option<String>,
) {
    let parsed = parse_explicit_settlement_output(raw_observation);
    let (mut settlement, source, parse_error) = match parsed {
        Ok(settlement) => (settlement, SettlementSource::FinalBodyObserver, None),
        Err(error) => (
            zero_change_degraded_settlement(
                chapter,
                body,
                &authority.authority_root_fingerprint,
                &error.to_string(),
            ),
            SettlementSource::ObserverDegraded,
            Some(error.to_string()),
        ),
    };
    bind_settlement_fingerprints(&mut settlement, chapter, body, authority);
    let validation = validate_and_bind_settlement(
        chapter,
        body,
        authority,
        &mut settlement,
        recover_required_state,
    );
    (settlement, validation, source, parse_error)
}

fn bind_settlement_fingerprints(
    settlement: &mut SettlementOutput,
    chapter: &ChapterRecord,
    body: &str,
    authority: &governance::SealedChapterAuthority,
) {
    settlement.chapter_fingerprint = chapter_revision_fingerprint(chapter, body);
    settlement.body_fingerprint = chapter_quality::chapter_body_fingerprint(body);
    settlement.authority_fingerprint = authority.authority_root_fingerprint.clone();
}

pub(super) fn deterministic_state_validation(
    content: &str,
    settlement: &SettlementOutput,
) -> StateValidationOutput {
    let mut warnings = Vec::new();
    let mut advisories = Vec::new();
    let language = if content.chars().any(is_cjk_unified) {
        "zh-CN"
    } else {
        "en"
    };
    if !content.trim().is_empty() {
        if settlement.current_state.trim().is_empty() {
            warnings.push("final-body settlement is missing a non-empty current_state".to_string());
        }
        if settlement.chapter_summary.trim().is_empty() {
            warnings
                .push("final-body settlement is missing a non-empty chapter_summary".to_string());
        }
    }
    for (label, value) in [
        ("current_state", settlement.current_state.as_str()),
        ("chapter_summary", settlement.chapter_summary.as_str()),
    ] {
        if !value.trim().is_empty() && chapter_summary_looks_like_prose_fragment(value, language) {
            advisories.push(format!(
                "{label} looks like a copied prose fragment instead of display metadata"
            ));
        }
        if !value.trim().is_empty() && !governance::truth_item_supported_by_chapter(value, content)
        {
            advisories.push(format!(
                "{label} contains display facts not visibly supported by final body"
            ));
        }
    }
    for update in &settlement.continuity_updates {
        if !update.trim().is_empty()
            && !governance::truth_item_supported_by_chapter(update, content)
        {
            advisories.push(format!(
                "continuity display item lacks visible support in final body: {}",
                update.trim()
            ));
        }
    }
    advisories.sort();
    advisories.dedup();
    warnings.sort();
    warnings.dedup();
    StateValidationOutput {
        passed: true,
        disposition: if warnings.is_empty() {
            StateSettlementDisposition::Ready
        } else {
            StateSettlementDisposition::DisplayMetadataDegraded
        },
        warnings,
        advisories,
    }
}

pub(super) fn validate_settlement_for_chapter(
    chapter: &ChapterRecord,
    content: &str,
    authority: &governance::SealedChapterAuthority,
    settlement: &SettlementOutput,
) -> StateValidationOutput {
    let mut checked = settlement.clone();
    validate_and_bind_settlement(chapter, content, authority, &mut checked, false)
}

fn validate_and_bind_settlement(
    chapter: &ChapterRecord,
    content: &str,
    authority: &governance::SealedChapterAuthority,
    settlement: &mut SettlementOutput,
    recover_required_state: bool,
) -> StateValidationOutput {
    let display_metadata_degraded = apply_settlement_display_fallback(chapter, content, settlement);
    let mut validation = deterministic_state_validation(content, settlement);
    if display_metadata_degraded {
        validation.disposition = validation
            .disposition
            .merge(StateSettlementDisposition::DisplayMetadataDegraded);
    }
    if !settlement.degraded_reason.trim().is_empty() {
        validation.advisories.push(format!(
            "state observer degraded: {}",
            settlement.degraded_reason.trim()
        ));
        validation.disposition = validation
            .disposition
            .merge(StateSettlementDisposition::ObserverFormatDegraded);
    }
    let expected_body = chapter_quality::chapter_body_fingerprint(content);
    if settlement.body_fingerprint != expected_body {
        validation
            .warnings
            .push("settlement belongs to a different final body".to_string());
        validation.disposition = validation
            .disposition
            .merge(StateSettlementDisposition::DependencyMismatch);
    }
    if settlement.authority_fingerprint != authority.authority_root_fingerprint {
        validation
            .warnings
            .push("settlement belongs to a different sealed chapter authority".to_string());
        validation.disposition = validation
            .disposition
            .merge(StateSettlementDisposition::DependencyMismatch);
    }
    if authority.chapter_number != chapter.number {
        validation
            .warnings
            .push("sealed authority belongs to a different chapter".to_string());
        validation.disposition = validation
            .disposition
            .merge(StateSettlementDisposition::DependencyMismatch);
    }

    let proposed_changes = std::mem::take(&mut settlement.state_changes);
    let mut accepted_changes = Vec::with_capacity(proposed_changes.len());
    let mut deferred_required_rejections = Vec::new();
    for (index, mut change) in proposed_changes.into_iter().enumerate() {
        change.change_id = format!("chapter-{:04}-change-{:04}", chapter.number, index + 1);
        bind_contract_authority(authority, &mut change);
        let entity = authority_entity_resolution(authority, &change.entity_id);
        if let Err(error) = validate_final_body_evidence(content, &entity, &mut change) {
            if recover_required_state
                && change.authority_path.trim() == REQUIRED_END_STATE_AUTHORITY_PATH
            {
                change.allowance = novel_bible::StateChangeAllowance::Rejected;
                deferred_required_rejections.push(error);
            } else {
                reject_untrusted_state_delta(&mut validation, &mut change, error);
            }
            continue;
        }
        change.allowance = match authority_allowance(authority, &entity, &change) {
            Ok(allowance) => allowance,
            Err(error) => {
                if recover_required_state
                    && change.authority_path.trim() == REQUIRED_END_STATE_AUTHORITY_PATH
                {
                    change.allowance = novel_bible::StateChangeAllowance::Rejected;
                    deferred_required_rejections.push(error);
                } else {
                    reject_untrusted_state_delta(&mut validation, &mut change, error);
                }
                continue;
            }
        };
        if change.event_type == novel_bible::ChapterStateEventType::HookDefer
            && !change
                .defer_until_chapter
                .is_some_and(|number| number > chapter.number)
        {
            let change_id = change.change_id.trim().to_string();
            reject_untrusted_state_delta(
                &mut validation,
                &mut change,
                format!(
                    "state change {} defers a hook without a later chapter",
                    change_id
                ),
            );
            continue;
        }
        accepted_changes.push(change);
    }
    if recover_required_state
        && !accepted_changes
            .iter()
            .any(|change| change.authority_path.trim() == REQUIRED_END_STATE_AUTHORITY_PATH)
    {
        if let Some(change) =
            recover_explicit_required_state_change(chapter, content, authority, &accepted_changes)
        {
            accepted_changes.push(change);
            validation.advisories.push(
                "recovered one uniquely resolved required end-state delta from sealed authority and bounded final-body evidence after observer attempts were exhausted"
                    .to_string(),
            );
        }
    }
    let required_end_state_bound = accepted_changes
        .iter()
        .any(|change| change.authority_path.trim() == REQUIRED_END_STATE_AUTHORITY_PATH);
    if !deferred_required_rejections.is_empty() {
        if required_end_state_bound {
            validation.advisories.push(
                "replaced malformed observer proposals for the required end-state slot with the uniquely validated final-body delta after observer attempts were exhausted"
                    .to_string(),
            );
        } else {
            for reason in deferred_required_rejections {
                record_untrusted_state_delta_rejection(&mut validation, reason);
            }
        }
    }
    settlement.state_changes = dedupe_required_end_state_changes(accepted_changes);
    if !authority
        .chapter_contract
        .new_state_after_chapter
        .trim()
        .is_empty()
        && !settlement
            .state_changes
            .iter()
            .any(|change| change.authority_path.trim() == REQUIRED_END_STATE_AUTHORITY_PATH)
    {
        validation.warnings.push(
            "final-body settlement is missing the required typed end-state change from chapter_contract.new_state_after_chapter"
                .to_string(),
        );
        validation.disposition = validation
            .disposition
            .merge(StateSettlementDisposition::RequiredStateMissing);
    }
    settlement.resolved_hooks =
        validated_resolved_hook_labels(authority, &settlement.state_changes);

    validation.warnings.sort();
    validation.warnings.dedup();
    validation.advisories.sort();
    validation.advisories.dedup();
    validation.passed = !validation.disposition.is_blocking();
    validation
}

fn reject_untrusted_state_delta(
    validation: &mut StateValidationOutput,
    change: &mut novel_bible::ChapterStateChange,
    reason: impl Into<String>,
) {
    change.allowance = novel_bible::StateChangeAllowance::Rejected;
    record_untrusted_state_delta_rejection(validation, reason);
}

fn record_untrusted_state_delta_rejection(
    validation: &mut StateValidationOutput,
    reason: impl Into<String>,
) {
    validation.warnings.push(format!(
        "rejected evidence-backed unauthorized state delta: {}",
        reason.into()
    ));
    // DependencyMismatch is the existing settlement owner for a typed delta
    // that cannot be bound to the sealed authority/final body.  Keep the
    // rejection in that owner instead of introducing a parallel pollution
    // disposition.
    validation.disposition = validation
        .disposition
        .merge(StateSettlementDisposition::DependencyMismatch);
}

fn apply_settlement_display_fallback(
    chapter: &ChapterRecord,
    content: &str,
    settlement: &mut SettlementOutput,
) -> bool {
    let mut degraded = false;
    let language = if content.chars().any(is_cjk_unified) {
        "zh-CN"
    } else {
        "en"
    };
    let fallback = || {
        if chapter.summary.trim().is_empty() {
            chapter_summary_fallback(content, language)
        } else {
            compact_chapter_summary(&chapter.summary, language)
        }
    };
    if settlement.chapter_summary.trim().is_empty() {
        settlement.chapter_summary = fallback();
        degraded = true;
    }
    if settlement.current_state.trim().is_empty() {
        settlement.current_state = settlement.chapter_summary.clone();
        degraded = true;
    }
    degraded
}

fn recover_explicit_required_state_change(
    chapter: &ChapterRecord,
    content: &str,
    authority: &governance::SealedChapterAuthority,
    accepted_changes: &[novel_bible::ChapterStateChange],
) -> Option<novel_bible::ChapterStateChange> {
    let required = authority.chapter_contract.new_state_after_chapter.trim();
    if required.is_empty() {
        return None;
    }
    let mut candidates = accepted_changes
        .iter()
        .filter(|change| required_state_event_allowed(change.event_type))
        .filter_map(|change| {
            let mut candidate = change.clone();
            candidate.authority_path = REQUIRED_END_STATE_AUTHORITY_PATH.to_string();
            bind_contract_authority(authority, &mut candidate);
            let entity = authority_entity_resolution(authority, &candidate.entity_id);
            validate_final_body_evidence(content, &entity, &mut candidate).ok()?;
            candidate.allowance = authority_allowance(authority, &entity, &candidate).ok()?;
            Some(candidate)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        (
            left.event_type,
            left.entity_id.as_str(),
            left.evidence.start_char,
        )
            .cmp(&(
                right.event_type,
                right.entity_id.as_str(),
                right.evidence.start_char,
            ))
    });
    candidates.dedup_by(|left, right| {
        left.event_type == right.event_type
            && left.entity_id == right.entity_id
            && left.evidence.start_char == right.evidence.start_char
            && left.evidence.end_char == right.evidence.end_char
    });
    if candidates.len() == 1 {
        return candidates.pop();
    }
    if !candidates.is_empty() {
        return None;
    }

    let mut event_candidates = required_state_event_candidates(authority, required);
    if event_candidates.is_empty() {
        event_candidates.push(novel_bible::ChapterStateEventType::Character);
    }
    let mut recovered = event_candidates
        .into_iter()
        .filter_map(|event| {
            recover_required_state_candidate_for_event(chapter, content, authority, required, event)
        })
        .collect::<Vec<_>>();
    recovered.sort_by(|left, right| {
        (
            left.event_type,
            left.entity_id.as_str(),
            left.evidence.start_char,
        )
            .cmp(&(
                right.event_type,
                right.entity_id.as_str(),
                right.evidence.start_char,
            ))
    });
    recovered.dedup_by(|left, right| {
        left.event_type == right.event_type
            && left.entity_id == right.entity_id
            && left.evidence.start_char == right.evidence.start_char
            && left.evidence.end_char == right.evidence.end_char
    });
    (recovered.len() == 1).then(|| recovered.remove(0))
}

fn required_state_event_allowed(event: novel_bible::ChapterStateEventType) -> bool {
    matches!(
        event,
        novel_bible::ChapterStateEventType::Character
            | novel_bible::ChapterStateEventType::Relationship
            | novel_bible::ChapterStateEventType::World
            | novel_bible::ChapterStateEventType::Power
            | novel_bible::ChapterStateEventType::Resource
            | novel_bible::ChapterStateEventType::HookAdvance
    )
}

fn required_state_event_candidates(
    authority: &governance::SealedChapterAuthority,
    required: &str,
) -> Vec<novel_bible::ChapterStateEventType> {
    use novel_bible::ChapterStateEventType as Event;

    let chapter = &authority.chapter_contract;
    let durable_companions = [
        (Event::Character, chapter.character_change.as_str()),
        (Event::Relationship, chapter.relationship_delta.as_str()),
        (Event::World, chapter.world_change.as_str()),
        (Event::Power, chapter.power_delta.as_str()),
        (Event::Resource, chapter.resource_delta.as_str()),
    ];
    let normalized_required = normalize_evidence_text(required);
    let mut candidates = durable_companions
        .iter()
        .filter(|(_, companion)| {
            !companion.trim().is_empty()
                && normalize_evidence_text(companion) == normalized_required
        })
        .map(|(event, _)| *event)
        .collect::<Vec<_>>();
    if candidates.is_empty()
        && !chapter.payoff_target.trim().is_empty()
        && normalize_evidence_text(&chapter.payoff_target) == normalized_required
    {
        candidates.push(Event::HookAdvance);
    }

    // Resolve a canonical character subject before comparing the required
    // outcome with optional companion fields. Companion fields can share a
    // protagonist name with the required outcome and otherwise win the
    // bounded-evidence similarity check even when the required state is a
    // different character's action. The sealed character registry is the
    // existing identity authority; this only orders its decision ahead of
    // the companion-field fallback.
    let subject_anchors = required_state_subject_anchors(authority, required);
    let canonical_character_subject = subject_anchors.len() == 1
        && serde_json::from_value::<NovelCreationContract>(authority.canonical_contract.clone())
            .ok()
            .is_some_and(|contract| {
                contract.characters.iter().any(|character| {
                    std::iter::once(character.canonical_name.as_str())
                        .chain(std::iter::once(character.character_id.as_str()))
                        .chain(character.aliases.iter().map(String::as_str))
                        .any(|surface| surface.trim() == subject_anchors[0].trim())
                })
            });
    // An exact sealed companion field is already an explicit contract
    // authority.  Only use the canonical character prefix as a fallback
    // when no exact event slot was identified; otherwise this would turn a
    // deliberately identical relationship/world/power field into an
    // artificial ambiguity.
    if canonical_character_subject && candidates.is_empty() {
        candidates.push(Event::Character);
    }

    if candidates.is_empty() {
        let cjk = required
            .chars()
            .chain(
                durable_companions
                    .iter()
                    .flat_map(|(_, companion)| companion.chars()),
            )
            .any(is_cjk_unified);
        candidates.extend(
            durable_companions
                .iter()
                .filter(|(_, companion)| {
                    !companion.trim().is_empty()
                        && governance::contract_change_supported_by_final_evidence(
                            required,
                            companion,
                            cjk,
                            &[],
                        )
                })
                .map(|(event, _)| *event),
        );
    }
    if candidates.is_empty()
        && !chapter.payoff_target.trim().is_empty()
        && existing_hook_id(authority, required).is_some()
        && governance::contract_change_supported_by_final_evidence(
            required,
            &chapter.payoff_target,
            required
                .chars()
                .chain(chapter.payoff_target.chars())
                .any(is_cjk_unified),
            &[],
        )
    {
        candidates.push(Event::HookAdvance);
    }
    if candidates.is_empty() {
        if !subject_anchors.is_empty() {
            // A chapter may declare a durable object/world outcome without
            // filling an optional companion slot.  The sealed payoff text is
            // still the authority; classify this as one world delta so
            // exhausted-observer recovery can bind the outcome to the object
            // explicitly named there.
            candidates.push(Event::World);
        }
    }
    if candidates.is_empty()
        && !chapter.relationship_delta.trim().is_empty()
        && required_state_uses_collective_relation_subject(required)
        && serde_json::from_value::<NovelCreationContract>(authority.canonical_contract.clone())
            .ok()
            .is_some_and(|contract| {
                crate::tool::writing::creation_contract::relationship_names_from_line(
                    &chapter.relationship_delta,
                    &contract.characters,
                )
                .len()
                    >= 2
            })
    {
        // Some contracts put the durable relationship wording in the optional
        // relationship delta while the required end state uses a collective
        // subject such as “双方”.  The explicit participant list in the sealed
        // relationship delta is sufficient to classify the same slot without
        // guessing a character from the collective pronoun.
        candidates.push(Event::Relationship);
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn required_state_uses_collective_relation_subject(required: &str) -> bool {
    ["双方", "两人", "二人", "彼此", "二者", "他们", "她们"]
        .iter()
        .any(|marker| required.contains(marker))
}

fn required_state_subject_anchors(
    authority: &governance::SealedChapterAuthority,
    required: &str,
) -> Vec<String> {
    let chapter = &authority.chapter_contract;
    let normalized_required = normalize_evidence_text(required);

    // A required outcome may begin with a canonical character followed by a
    // location, condition, or action (for example, “阮栖舟在荒野中发现…”).
    // The generic event-anchor extractor quite correctly treats the whole
    // pre-verb phrase as an object anchor, but that must not outrank the
    // sealed character identity when the prefix is an exact canonical
    // surface. Reuse the existing contract character registry and return the
    // unique canonical subject before falling back to generic anchors.
    if let Ok(contract) =
        serde_json::from_value::<NovelCreationContract>(authority.canonical_contract.clone())
    {
        let mut leading_characters = contract
            .characters
            .iter()
            .filter_map(|character| {
                let is_leading_surface = std::iter::once(character.canonical_name.as_str())
                    .chain(std::iter::once(character.character_id.as_str()))
                    .chain(character.aliases.iter().map(String::as_str))
                    .map(normalize_evidence_text)
                    .filter(|surface| !surface.is_empty())
                    .any(|surface| {
                        normalized_required.starts_with(&surface)
                            && authority_mentions_exact_entity(authority, &surface)
                    });
                is_leading_surface.then_some(character.canonical_name.trim().to_string())
            })
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        leading_characters.sort();
        leading_characters.dedup();
        if !leading_characters.is_empty() {
            return leading_characters;
        }
    }

    let supporting_authority = [
        chapter.goal.as_str(),
        chapter.scene_goal.as_str(),
        chapter.conflict.as_str(),
        chapter.choice.as_str(),
        chapter.cost.as_str(),
        chapter.reveal.as_str(),
        chapter.emotional_beat.as_str(),
        chapter.payoff_target.as_str(),
        chapter.character_change.as_str(),
        chapter.relationship_delta.as_str(),
        chapter.world_change.as_str(),
        chapter.power_delta.as_str(),
        chapter.resource_delta.as_str(),
    ];
    let mut anchors = governance::required_entity_anchors(required)
        .into_iter()
        .filter(|anchor| {
            supporting_authority.iter().any(|value| {
                normalize_evidence_text(value).contains(&normalize_evidence_text(anchor))
            }) && authority_mentions_exact_entity(authority, anchor)
        })
        .collect::<Vec<_>>();
    anchors.sort();
    anchors.dedup();
    anchors
}

fn recover_required_state_candidate_for_event(
    chapter: &ChapterRecord,
    content: &str,
    authority: &governance::SealedChapterAuthority,
    required: &str,
    event_type: novel_bible::ChapterStateEventType,
) -> Option<novel_bible::ChapterStateChange> {
    if !required_state_event_allowed(event_type) {
        return None;
    }
    if event_type == novel_bible::ChapterStateEventType::HookAdvance {
        let entity_id = authority_hook_entity_id(
            authority,
            event_type,
            "chapter_contract.payoff_target",
            &authority.chapter_contract.payoff_target,
        )?;
        return recovered_required_change_for_entity(
            chapter,
            content,
            authority,
            required,
            event_type,
            &entity_id,
            REQUIRED_END_STATE_AUTHORITY_PATH,
        );
    }
    if event_type != novel_bible::ChapterStateEventType::Character {
        let subject_anchors = required_state_subject_anchors(authority, required)
            .into_iter()
            .filter(|anchor| content.contains(anchor))
            .collect::<Vec<_>>();
        if subject_anchors.len() == 1 {
            let subject_anchor = &subject_anchors[0];
            let entity_id = serde_json::from_value::<NovelCreationContract>(
                authority.canonical_contract.clone(),
            )
            .ok()
            .and_then(|contract| {
                contract
                    .characters
                    .into_iter()
                    .find(|character| {
                        std::iter::once(character.canonical_name.as_str())
                            .chain(character.aliases.iter().map(String::as_str))
                            .map(normalize_evidence_text)
                            .any(|surface| {
                                !surface.is_empty()
                                    && surface == normalize_evidence_text(subject_anchor)
                            })
                    })
                    .map(|character| {
                        if character.character_id.trim().is_empty() {
                            character.canonical_name
                        } else {
                            character.character_id
                        }
                    })
            })
            .unwrap_or_else(|| subject_anchor.clone());
            return recovered_required_change_for_entity(
                chapter,
                content,
                authority,
                required,
                event_type,
                &entity_id,
                REQUIRED_END_STATE_AUTHORITY_PATH,
            );
        }
    }
    let contract =
        serde_json::from_value::<NovelCreationContract>(authority.canonical_contract.clone())
            .ok()?;
    let normalized_required = normalize_evidence_text(required);

    // Relationship outcomes are sometimes phrased with a collective subject
    // (for example, “双方……”) rather than repeating either character name in
    // `new_state_after_chapter`.  The sealed relationship delta already carries
    // the canonical participants; reuse the creation-contract relationship
    // parser and bind the relationship delta to its first canonical participant.
    // `apply_approved_chapter` resolves the unique relation from the same
    // evidence span, so this does not introduce a second relationship identity
    // or a parallel state ledger.
    if event_type == novel_bible::ChapterStateEventType::Relationship {
        let relationship_text = authority.chapter_contract.relationship_delta.trim();
        if !relationship_text.is_empty() {
            let participants =
                crate::tool::writing::creation_contract::relationship_names_from_line(
                    relationship_text,
                    &contract.characters,
                );
            if participants.len() >= 2 && participants.iter().all(|name| content.contains(name)) {
                if let Some(character) = contract
                    .characters
                    .iter()
                    .find(|character| character.canonical_name.trim() == participants[0])
                {
                    let entity_id = if character.character_id.trim().is_empty() {
                        character.canonical_name.trim()
                    } else {
                        character.character_id.trim()
                    };
                    if !entity_id.is_empty() {
                        if let Some(change) = recovered_required_change_for_entity(
                            chapter,
                            content,
                            authority,
                            required,
                            event_type,
                            entity_id,
                            REQUIRED_END_STATE_AUTHORITY_PATH,
                        ) {
                            return Some(change);
                        }
                    }
                }
            }
        }
    }
    let primary_role_is_named = character_contract_line_marks_primary(required);
    let mentioned_candidates = contract
        .characters
        .iter()
        .filter(|character| {
            let explicitly_named = [
                character.character_id.as_str(),
                character.canonical_name.as_str(),
            ]
            .into_iter()
            .chain(character.aliases.iter().map(String::as_str))
            .filter(|surface| !surface.trim().is_empty())
            .any(|surface| normalized_required.contains(&normalize_evidence_text(surface)));
            explicitly_named || (primary_role_is_named && character.role_looks_primary())
        })
        .collect::<Vec<_>>();
    let mut longest_leading_surface: Option<usize> = None;
    for character in &contract.characters {
        for surface in std::iter::once(character.canonical_name.as_str())
            .chain(character.aliases.iter().map(String::as_str))
        {
            let surface = normalize_evidence_text(surface);
            if !surface.is_empty() && normalized_required.starts_with(&surface) {
                longest_leading_surface = Some(
                    longest_leading_surface
                        .unwrap_or_default()
                        .max(surface.chars().count()),
                );
            }
        }
    }
    let mut candidates = if let Some(longest_leading_surface) = longest_leading_surface {
        contract
            .characters
            .iter()
            .filter(|character| {
                std::iter::once(character.canonical_name.as_str())
                    .chain(character.aliases.iter().map(String::as_str))
                    .map(normalize_evidence_text)
                    .any(|surface| {
                        surface.chars().count() == longest_leading_surface
                            && normalized_required.starts_with(&surface)
                    })
            })
            .collect::<Vec<_>>()
    } else {
        mentioned_candidates
    };
    candidates.sort_by(|left, right| {
        left.character_id
            .cmp(&right.character_id)
            .then_with(|| left.canonical_name.cmp(&right.canonical_name))
    });
    candidates.dedup_by(|left, right| {
        left.character_id == right.character_id && left.canonical_name == right.canonical_name
    });
    let character = (candidates.len() == 1).then(|| candidates.remove(0))?;
    let entity_id = if character.character_id.trim().is_empty() {
        character.canonical_name.trim()
    } else {
        character.character_id.trim()
    };
    if entity_id.is_empty() {
        return None;
    }
    recovered_required_change_for_entity(
        chapter,
        content,
        authority,
        required,
        event_type,
        entity_id,
        REQUIRED_END_STATE_AUTHORITY_PATH,
    )
}

fn recovered_required_change_for_entity(
    chapter: &ChapterRecord,
    content: &str,
    authority: &governance::SealedChapterAuthority,
    required: &str,
    event_type: novel_bible::ChapterStateEventType,
    entity_id: &str,
    required_path: &str,
) -> Option<novel_bible::ChapterStateChange> {
    let entity = authority_entity_resolution(authority, entity_id);
    let cjk = required.chars().chain(content.chars()).any(is_cjk_unified);
    let ignored_entity_surfaces = if state_event_is_hook(event_type) {
        &[][..]
    } else {
        entity.public_surfaces.as_slice()
    };
    let mut evidence_candidates = runner::final_body_evidence_spans(content)
        .into_iter()
        .filter_map(|span| {
            let score = governance::contract_change_evidence_score(
                required,
                &span.excerpt,
                cjk,
                ignored_entity_surfaces,
            );
            (score >= 2).then_some((score, span))
        })
        .collect::<Vec<_>>();
    let best_score = evidence_candidates.iter().map(|(score, _)| *score).max()?;
    evidence_candidates.retain(|(score, _)| *score == best_score);
    // The indexed windows intentionally include 1-, 2-, and 3-sentence
    // spans.  When a wider window only repeats a shorter equally strong
    // match, keep the narrow event span; distinct equally strong spans are
    // retained and resolved by the deterministic earliest-valid tie-break
    // below because they all describe this same sealed required slot.
    let nested_equal_score = evidence_candidates
        .iter()
        .map(|(score, span)| {
            evidence_candidates.iter().any(|(other_score, other)| {
                score == other_score
                    && (other.start_char > span.start_char || other.end_char < span.end_char)
                    && other.start_char >= span.start_char
                    && other.end_char <= span.end_char
            })
        })
        .collect::<Vec<_>>();
    evidence_candidates = evidence_candidates
        .into_iter()
        .zip(nested_equal_score)
        .filter_map(|(candidate, nested)| (!nested).then_some(candidate))
        .collect();
    // Every remaining candidate is already bound to the same sealed required
    // slot, event type, entity and minimum evidence score.  If the final body
    // restates that event in separate windows, rejecting the whole chapter as
    // "ambiguous" only turns a valid repeated confirmation into a false
    // blocker.  Prefer the earliest valid evidence deterministically; the
    // later restatement is not a second state slot and is therefore not
    // admitted separately.
    evidence_candidates.sort_by_key(|(_, span)| (span.start_char, span.end_char));
    for (_, span) in evidence_candidates {
        let mut change = novel_bible::ChapterStateChange {
            change_id: format!("chapter-{:04}-change-required-recovered", chapter.number),
            entity_id: entity_id.to_string(),
            event_type,
            value: span.excerpt.clone(),
            evidence: novel_bible::ChapterBodyEvidence {
                start_char: span.start_char,
                end_char: span.end_char,
                excerpt: span.excerpt,
            },
            authority_path: required_path.to_string(),
            ..Default::default()
        };
        bind_contract_authority(authority, &mut change);
        if validate_final_body_evidence(content, &entity, &mut change).is_err() {
            continue;
        }
        let Ok(allowance) = authority_allowance(authority, &entity, &change) else {
            continue;
        };
        change.allowance = allowance;
        return Some(change);
    }
    None
}

/// `new_state_after_chapter` is the required outcome assertion for this
/// chapter, not a second durable state slot. When the observer also emits an
/// optional typed field for the same entity and event type, keep the required
/// outcome delta and discard the overlapping optional delta so application
/// order cannot overwrite the final state with a parallel description.
fn dedupe_required_end_state_changes(
    changes: Vec<novel_bible::ChapterStateChange>,
) -> Vec<novel_bible::ChapterStateChange> {
    let required_slots = changes
        .iter()
        .filter(|change| change.authority_path.trim() == REQUIRED_END_STATE_AUTHORITY_PATH)
        .map(|change| (change.event_type, change.entity_id.trim().to_string()))
        .collect::<Vec<_>>();
    let mut kept_required_slots = Vec::new();
    changes
        .into_iter()
        .filter(|change| {
            let slot = (change.event_type, change.entity_id.trim().to_string());
            if change.authority_path.trim() == REQUIRED_END_STATE_AUTHORITY_PATH {
                if kept_required_slots.contains(&slot) {
                    return false;
                }
                kept_required_slots.push(slot);
                return true;
            }
            !required_slots.contains(&slot)
        })
        .collect()
}

fn validated_resolved_hook_labels(
    authority: &governance::SealedChapterAuthority,
    state_changes: &[novel_bible::ChapterStateChange],
) -> Vec<String> {
    let mut labels = Vec::new();
    for change in state_changes.iter().filter(|change| {
        change.event_type == novel_bible::ChapterStateEventType::HookPayOff
            && matches!(
                change.allowance,
                novel_bible::StateChangeAllowance::Contract
                    | novel_bible::StateChangeAllowance::BoundedIncidental
            )
    }) {
        let Some((_, label)) =
            authority_values(authority, novel_bible::ChapterStateEventType::HookPayOff)
                .into_iter()
                .find(|(path, _)| path == change.authority_path.trim())
        else {
            continue;
        };
        let label = label.trim();
        if !label.is_empty() && !labels.iter().any(|existing| existing == label) {
            labels.push(label.to_string());
        }
    }
    labels
}

fn validate_final_body_evidence(
    content: &str,
    entity: &AuthorityEntityResolution,
    change: &mut novel_bible::ChapterStateChange,
) -> Result<(), String> {
    if change.entity_id.trim().is_empty() || change.value.trim().is_empty() {
        return Err(format!(
            "state change {} is missing entity_id or value",
            change.change_id.trim()
        ));
    }
    rebind_final_body_evidence(content, entity, change);
    let chars = content.chars().collect::<Vec<_>>();
    let start = change.evidence.start_char;
    let end = change.evidence.end_char;
    let exact_span_matches = start < end
        && end <= chars.len()
        && chars[start..end].iter().collect::<String>() == change.evidence.excerpt;
    if !exact_span_matches {
        let excerpt_chars = change.evidence.excerpt.chars().collect::<Vec<_>>();
        let matches = chars
            .windows(excerpt_chars.len())
            .enumerate()
            .filter(|(_, window)| *window == excerpt_chars.as_slice())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(format!(
                "state change {} evidence excerpt is absent or ambiguous in final body",
                change.change_id.trim()
            ));
        }
        change.evidence.start_char = matches[0];
        change.evidence.end_char = matches[0] + excerpt_chars.len();
    }
    let normalized_excerpt = normalize_evidence_text(&change.evidence.excerpt);
    let normalized_value = normalize_evidence_text(&change.value);
    let hook_event = state_event_is_hook(change.event_type);
    let public_entity_is_present = hook_event
        || entity
            .public_surfaces
            .iter()
            .map(|surface| normalize_evidence_text(surface))
            .filter(|surface| !surface.is_empty())
            .any(|surface| normalized_excerpt.contains(&surface))
        || (entity.stable_id_resolved && entity.public_surfaces.is_empty());
    if !public_entity_is_present
        || normalized_value.is_empty()
        || !normalized_excerpt.contains(&normalized_value)
    {
        return Err(format!(
            "state change {} evidence does not explicitly contain its public entity surface and verbatim value",
            change.change_id.trim()
        ));
    }
    Ok(())
}

fn rebind_final_body_evidence(
    content: &str,
    entity: &AuthorityEntityResolution,
    change: &mut novel_bible::ChapterStateChange,
) {
    let mut excerpt = change.evidence.excerpt.trim().to_string();
    if unique_body_excerpt(content, &excerpt).is_none() {
        return;
    }
    let hook_event = state_event_is_hook(change.event_type);
    if !hook_event
        && !entity.public_surfaces.is_empty()
        && !entity
            .public_surfaces
            .iter()
            .any(|surface| excerpt.contains(surface))
    {
        if let Some(expanded) =
            expand_excerpt_to_nearby_public_surface(content, &excerpt, &entity.public_surfaces)
        {
            excerpt = expanded;
        }
    }
    if let Some(start) = unique_body_excerpt(content, &excerpt) {
        change.evidence.start_char = content[..start].chars().count();
        change.evidence.end_char = change.evidence.start_char + excerpt.chars().count();
        change.evidence.excerpt = excerpt;
    }
}

fn unique_body_excerpt(content: &str, excerpt: &str) -> Option<usize> {
    let excerpt = excerpt.trim();
    if excerpt.is_empty() {
        return None;
    }
    let mut matches = content.match_indices(excerpt);
    let first = matches.next()?.0;
    matches.next().is_none().then_some(first)
}

fn expand_excerpt_to_nearby_public_surface(
    content: &str,
    excerpt: &str,
    public_surfaces: &[String],
) -> Option<String> {
    let excerpt_start = unique_body_excerpt(content, excerpt)?;
    let excerpt_end = excerpt_start + excerpt.len();
    let paragraph_start = content[..excerpt_start]
        .rfind("\n\n")
        .map(|index| index + 2)
        .unwrap_or(0);
    let prefix = &content[paragraph_start..excerpt_start];
    let surface_start = public_surfaces
        .iter()
        .filter_map(|surface| prefix.rfind(surface).map(|index| paragraph_start + index))
        .max()?;
    let expanded = content[surface_start..excerpt_end].trim();
    let expanded_len = expanded.chars().count();
    if expanded_len == 0 || expanded_len > 320 || unique_body_excerpt(content, expanded).is_none() {
        return None;
    }
    Some(expanded.to_string())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AuthorityEntityResolution {
    stable_id_resolved: bool,
    public_surfaces: Vec<String>,
}

fn authority_entity_resolution(
    authority: &governance::SealedChapterAuthority,
    entity_id: &str,
) -> AuthorityEntityResolution {
    let needle = normalize_evidence_text(entity_id);
    if needle.is_empty() {
        return AuthorityEntityResolution::default();
    }
    let mut resolution = AuthorityEntityResolution::default();
    for registration in &authority.character_registrations {
        if normalize_evidence_text(&registration.character_id) == needle {
            resolution.stable_id_resolved = true;
            resolution
                .public_surfaces
                .push(registration.canonical_name.clone());
        }
    }
    collect_entity_resolution_from_json(&authority.canonical_contract, &needle, &mut resolution);
    collect_entity_resolution_from_json(&authority.truth_as_of_chapter, &needle, &mut resolution);
    if !looks_like_internal_entity_id(entity_id)
        && authority_mentions_exact_entity(authority, entity_id)
    {
        resolution
            .public_surfaces
            .push(entity_id.trim().to_string());
    }
    resolution
        .public_surfaces
        .retain(|surface| !surface.trim().is_empty());
    resolution.public_surfaces.sort();
    resolution.public_surfaces.dedup();
    resolution
}

fn collect_entity_resolution_from_json(
    value: &serde_json::Value,
    needle: &str,
    out: &mut AuthorityEntityResolution,
) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                collect_entity_resolution_from_json(item, needle, out);
            }
        }
        serde_json::Value::Object(fields) => {
            let id_matches = ["id", "character_id", "artifact_id", "entity_id", "hook_id"]
                .into_iter()
                .filter_map(|key| fields.get(key).and_then(serde_json::Value::as_str))
                .any(|id| normalize_evidence_text(id) == needle);
            if id_matches {
                out.stable_id_resolved = true;
                for key in ["canonical_name", "name", "title"] {
                    if let Some(surface) = fields
                        .get(key)
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|surface| !surface.is_empty())
                    {
                        out.public_surfaces.push(surface.to_string());
                    }
                }
                if let Some(characters) = fields
                    .get("characters")
                    .and_then(serde_json::Value::as_array)
                {
                    out.public_surfaces.extend(
                        characters
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::trim)
                            .filter(|surface| !surface.is_empty())
                            .map(ToString::to_string),
                    );
                }
            }
            for child in fields.values() {
                collect_entity_resolution_from_json(child, needle, out);
            }
        }
        _ => {}
    }
}

fn looks_like_internal_entity_id(value: &str) -> bool {
    let value = value.trim();
    value.rsplit_once('-').is_some_and(|(prefix, suffix)| {
        !prefix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
    })
}

fn authority_allowance(
    authority: &governance::SealedChapterAuthority,
    entity: &AuthorityEntityResolution,
    change: &novel_bible::ChapterStateChange,
) -> Result<novel_bible::StateChangeAllowance, String> {
    use novel_bible::{ChapterStateEventType as Event, StateChangeAllowance as Allowance};

    if change.event_type == Event::Incidental {
        if change.authority_path != "bounded_incidental" {
            return Err(format!(
                "state change {} exceeds bounded incidental authority",
                change.change_id.trim()
            ));
        }
        if !authority_mentions_exact_entity(authority, &change.entity_id) {
            return Err(format!(
                "state change {} names an incidental entity absent from sealed authority",
                change.change_id.trim()
            ));
        }
        return Ok(Allowance::BoundedIncidental);
    }

    let allowed = authority_values(authority, change.event_type);
    let Some((path, value)) = allowed.iter().find(|(path, value)| {
        path == &change.authority_path
            && normalize_evidence_text(value) == normalize_evidence_text(&change.authority_excerpt)
    }) else {
        return Err(format!(
            "state change {} is not allowed by the sealed chapter authority",
            change.change_id.trim()
        ));
    };
    let hook_event = state_event_is_hook(change.event_type);
    let entity_is_authorized = if hook_event {
        authority_hook_entity_id(authority, change.event_type, path, value)
            .is_some_and(|expected| expected == change.entity_id.trim())
    } else {
        authority_mentions_exact_entity(authority, &change.entity_id)
    };
    if !entity_is_authorized {
        return Err(format!(
            "state change {} does not exactly resolve to authority field {}",
            change.change_id.trim(),
            path
        ));
    }
    let cjk = value
        .chars()
        .chain(change.evidence.excerpt.chars())
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
    if !governance::contract_change_supported_by_final_evidence(
        value,
        &change.evidence.excerpt,
        cjk,
        if hook_event {
            &[]
        } else {
            &entity.public_surfaces
        },
    ) {
        return Err(format!(
            "state change {} evidence does not support sealed authority field {}",
            change.change_id.trim(),
            path
        ));
    }
    Ok(Allowance::Contract)
}

fn authority_mentions_exact_entity(
    authority: &governance::SealedChapterAuthority,
    entity_id: &str,
) -> bool {
    let needle = normalize_evidence_text(entity_id);
    let minimum_len = if needle.is_ascii() { 3 } else { 2 };
    if needle.chars().count() < minimum_len {
        return false;
    }
    let chapter = &authority.chapter_contract;
    authority
        .character_registrations
        .iter()
        .any(|registration| {
            normalize_evidence_text(&registration.canonical_name) == needle
                || normalize_evidence_text(&registration.character_id) == needle
        })
        || json_value_mentions_entity(&authority.canonical_contract, &needle)
        || json_value_mentions_entity(&authority.truth_as_of_chapter, &needle)
        || authority_values(authority, novel_bible::ChapterStateEventType::Character)
            .into_iter()
            .chain(authority_values(
                authority,
                novel_bible::ChapterStateEventType::Relationship,
            ))
            .chain(authority_values(
                authority,
                novel_bible::ChapterStateEventType::World,
            ))
            .chain(authority_values(
                authority,
                novel_bible::ChapterStateEventType::Power,
            ))
            .chain(authority_values(
                authority,
                novel_bible::ChapterStateEventType::Resource,
            ))
            .chain(
                [
                    ("chapter_contract.goal", chapter.goal.as_str()),
                    ("chapter_contract.scene_goal", chapter.scene_goal.as_str()),
                    ("chapter_contract.conflict", chapter.conflict.as_str()),
                    ("chapter_contract.choice", chapter.choice.as_str()),
                    ("chapter_contract.cost", chapter.cost.as_str()),
                    ("chapter_contract.reveal", chapter.reveal.as_str()),
                    (
                        "chapter_contract.emotional_beat",
                        chapter.emotional_beat.as_str(),
                    ),
                    (
                        "chapter_contract.new_state_after_chapter",
                        chapter.new_state_after_chapter.as_str(),
                    ),
                    (
                        "chapter_contract.relationship_delta",
                        chapter.relationship_delta.as_str(),
                    ),
                    (
                        "chapter_contract.world_change",
                        chapter.world_change.as_str(),
                    ),
                    ("chapter_contract.power_delta", chapter.power_delta.as_str()),
                    (
                        "chapter_contract.resource_delta",
                        chapter.resource_delta.as_str(),
                    ),
                ]
                .into_iter()
                .map(|(path, value)| (path.to_string(), value.to_string())),
            )
            .any(|(_, value)| normalize_evidence_text(&value).contains(&needle))
}

fn json_value_mentions_entity(value: &serde_json::Value, needle: &str) -> bool {
    let minimum_len = if needle.is_ascii() { 3 } else { 2 };
    if needle.chars().count() < minimum_len {
        return false;
    }
    match value {
        serde_json::Value::String(text) => normalize_evidence_text(text).contains(needle),
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| json_value_mentions_entity(item, needle)),
        serde_json::Value::Object(fields) => fields
            .values()
            .any(|item| json_value_mentions_entity(item, needle)),
        _ => false,
    }
}

fn authority_values(
    authority: &governance::SealedChapterAuthority,
    event: novel_bible::ChapterStateEventType,
) -> Vec<(String, String)> {
    use novel_bible::ChapterStateEventType as Event;
    let chapter = &authority.chapter_contract;
    match event {
        Event::Character => scalar_authorities([
            (
                "chapter_contract.character_change",
                chapter.character_change.as_str(),
            ),
            (
                "chapter_contract.new_state_after_chapter",
                chapter.new_state_after_chapter.as_str(),
            ),
        ]),
        Event::Relationship => scalar_authorities([
            (
                "chapter_contract.relationship_delta",
                chapter.relationship_delta.as_str(),
            ),
            (
                "chapter_contract.new_state_after_chapter",
                chapter.new_state_after_chapter.as_str(),
            ),
        ]),
        Event::World => scalar_authorities([
            (
                "chapter_contract.world_change",
                chapter.world_change.as_str(),
            ),
            (
                "chapter_contract.new_state_after_chapter",
                chapter.new_state_after_chapter.as_str(),
            ),
        ]),
        Event::Power => scalar_authorities([
            ("chapter_contract.power_delta", chapter.power_delta.as_str()),
            (
                "chapter_contract.new_state_after_chapter",
                chapter.new_state_after_chapter.as_str(),
            ),
        ]),
        Event::Resource => scalar_authorities([
            (
                "chapter_contract.resource_delta",
                chapter.resource_delta.as_str(),
            ),
            (
                "chapter_contract.new_state_after_chapter",
                chapter.new_state_after_chapter.as_str(),
            ),
        ]),
        Event::HookSeed => chapter
            .hook_opened
            .iter()
            .enumerate()
            .filter(|(_, value)| !value.trim().is_empty())
            .map(|(index, value)| {
                (
                    format!("chapter_contract.hook_opened/{index}"),
                    value.clone(),
                )
            })
            .collect(),
        Event::HookAdvance => scalar_authorities([
            (
                "chapter_contract.payoff_target",
                chapter.payoff_target.as_str(),
            ),
            (
                "chapter_contract.new_state_after_chapter",
                chapter.new_state_after_chapter.as_str(),
            ),
        ]),
        Event::HookDefer => {
            scalar_authority("chapter_contract.payoff_target", &chapter.payoff_target)
        }
        Event::HookPayOff => chapter
            .hook_paid_off
            .iter()
            .enumerate()
            .filter(|(_, value)| !value.trim().is_empty())
            .map(|(index, value)| {
                (
                    format!("chapter_contract.hook_paid_off/{index}"),
                    value.clone(),
                )
            })
            .collect(),
        Event::Incidental => Vec::new(),
    }
}

fn scalar_authority(path: &str, value: &str) -> Vec<(String, String)> {
    if value.trim().is_empty() {
        Vec::new()
    } else {
        vec![(path.to_string(), value.to_string())]
    }
}

fn scalar_authorities<const N: usize>(values: [(&str, &str); N]) -> Vec<(String, String)> {
    values
        .into_iter()
        .flat_map(|(path, value)| scalar_authority(path, value))
        .collect()
}

fn normalize_evidence_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !ch.is_ascii_punctuation())
        .flat_map(char::to_lowercase)
        .collect()
}

fn authority_event_for_path(path: &str) -> Option<novel_bible::ChapterStateEventType> {
    use novel_bible::ChapterStateEventType as Event;

    match path.trim() {
        "chapter_contract.character_change" => Some(Event::Character),
        "chapter_contract.relationship_delta" => Some(Event::Relationship),
        "chapter_contract.world_change" => Some(Event::World),
        "chapter_contract.power_delta" => Some(Event::Power),
        "chapter_contract.resource_delta" => Some(Event::Resource),
        path if path.starts_with("chapter_contract.hook_opened/") => Some(Event::HookSeed),
        path if path.starts_with("chapter_contract.hook_paid_off/") => Some(Event::HookPayOff),
        _ => None,
    }
}

fn state_event_is_hook(event: novel_bible::ChapterStateEventType) -> bool {
    use novel_bible::ChapterStateEventType as Event;

    matches!(
        event,
        Event::HookSeed | Event::HookAdvance | Event::HookPayOff | Event::HookDefer
    )
}

fn authority_hook_entity_id(
    authority: &governance::SealedChapterAuthority,
    event: novel_bible::ChapterStateEventType,
    path: &str,
    authority_value: &str,
) -> Option<String> {
    use novel_bible::ChapterStateEventType as Event;

    if !state_event_is_hook(event) {
        return None;
    }
    if let Some(existing) = existing_hook_id(authority, authority_value) {
        return Some(existing);
    }
    if event != Event::HookSeed
        || !path.trim().starts_with("chapter_contract.hook_opened/")
        || authority_value.trim().is_empty()
    {
        return None;
    }
    let normalized = normalize_evidence_text(authority_value);
    if normalized.is_empty() {
        return None;
    }
    let digest = hex::encode(Sha256::digest(normalized.as_bytes()));
    Some(format!("hook-seed-{}", &digest[..16]))
}

fn existing_hook_id(
    authority: &governance::SealedChapterAuthority,
    authority_value: &str,
) -> Option<String> {
    let needle = normalize_evidence_text(authority_value);
    if needle.is_empty() {
        return None;
    }
    let hooks = authority
        .truth_as_of_chapter
        .pointer("/story_state/hook_ledger")
        .and_then(serde_json::Value::as_array)?;
    if let Some(exact) = hooks.iter().find_map(|hook| {
        let id = hook.get("id").and_then(serde_json::Value::as_str)?.trim();
        if id.is_empty() {
            return None;
        }
        let scalar_match = ["id", "title", "reader_knows"]
            .into_iter()
            .filter_map(|key| hook.get(key).and_then(serde_json::Value::as_str))
            .any(|value| normalize_evidence_text(value) == needle);
        let evidence_match = hook
            .get("evidence")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .any(|value| normalize_evidence_text(value) == needle)
            });
        (scalar_match || evidence_match).then(|| id.to_string())
    }) {
        return Some(exact);
    }

    // Execution packages express hook progress as natural language. Reuse the
    // existing truth-support matcher, but only bind when exactly one existing
    // hook is supported; ambiguity must remain untrusted.
    let mut semantic_matches = hooks
        .iter()
        .filter_map(|hook| {
            let id = hook.get("id").and_then(serde_json::Value::as_str)?.trim();
            if id.is_empty() {
                return None;
            }
            let supported = ["title", "reader_knows"]
                .into_iter()
                .filter_map(|key| hook.get(key).and_then(serde_json::Value::as_str))
                .chain(
                    hook.get("evidence")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(serde_json::Value::as_str),
                )
                .any(|label| governance::truth_item_supported_by_chapter(label, authority_value));
            supported.then(|| id.to_string())
        })
        .collect::<Vec<_>>();
    semantic_matches.sort();
    semantic_matches.dedup();
    (semantic_matches.len() == 1).then(|| semantic_matches.remove(0))
}

fn bind_contract_authority(
    authority: &governance::SealedChapterAuthority,
    change: &mut novel_bible::ChapterStateChange,
) {
    if change.event_type == novel_bible::ChapterStateEventType::Incidental {
        return;
    }
    if let Some(event) = authority_event_for_path(&change.authority_path) {
        change.event_type = event;
    }
    if let Some((_, value)) = authority_values(authority, change.event_type)
        .into_iter()
        .find(|(path, _)| path == change.authority_path.trim())
    {
        change.authority_excerpt = value.clone();
        if let Some(entity_id) =
            authority_hook_entity_id(authority, change.event_type, &change.authority_path, &value)
        {
            change.entity_id = entity_id;
        }
    }
}

#[cfg(test)]
pub(super) fn parse_settlement_output(raw: &str, _content: &str) -> SettlementOutput {
    parse_explicit_settlement_output(raw).unwrap_or_else(|error| SettlementOutput {
        chapter_fingerprint: String::new(),
        body_fingerprint: String::new(),
        authority_fingerprint: String::new(),
        state_changes: Vec::new(),
        degraded_reason: error.to_string(),
        current_state: String::new(),
        pending_hooks: String::new(),
        chapter_summary: String::new(),
        continuity_updates: Vec::new(),
        resolved_hooks: Vec::new(),
    })
}

pub(super) fn parse_explicit_settlement_output(raw: &str) -> anyhow::Result<SettlementOutput> {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim();
    let trimmed = trimmed.strip_suffix("```").unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        anyhow::bail!("final chapter observer returned no settlement");
    }
    let settlement = serde_json::from_str::<SettlementOutput>(trimmed)
        .map_err(|error| anyhow::anyhow!("invalid explicit chapter settlement: {error}"))?;
    Ok(settlement)
}

pub(super) fn payoff_continuity_update(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook_authority(
        hook_value: &str,
        existing_hooks: serde_json::Value,
    ) -> governance::SealedChapterAuthority {
        governance::SealedChapterAuthority {
            schema_version: "test".to_string(),
            chapter_number: 1,
            canonical_contract: json!({}),
            truth_as_of_chapter: json!({
                "story_state": {
                    "hook_ledger": existing_hooks
                }
            }),
            truth_cutoff_chapter: 0,
            context_package: governance::ContextPackage {
                schema_version: "test".to_string(),
                chapter_number: 1,
                selected_context: Vec::new(),
            },
            rule_stack: governance::RuleStack {
                schema_version: "test".to_string(),
                chapter_number: 1,
                hard: Vec::new(),
                soft: Vec::new(),
                diagnostic: Vec::new(),
            },
            trace: governance::ChapterTrace {
                schema_version: "test".to_string(),
                chapter_number: 1,
                planner_inputs: Vec::new(),
                composer_inputs: Vec::new(),
                selected_sources: Vec::new(),
                notes: Vec::new(),
                selection_decisions: Vec::new(),
                prompt_context_fingerprint: String::new(),
                context_budget: json!({}),
            },
            chapter_contract: ChapterContractRecord {
                number: 1,
                title: "chapter".to_string(),
                path: String::new(),
                markdown_path: String::new(),
                goal: "goal".to_string(),
                scene_goal: String::new(),
                conflict: String::new(),
                choice: String::new(),
                cost: String::new(),
                reveal: String::new(),
                emotional_beat: String::new(),
                new_state_after_chapter: String::new(),
                relationship_delta: String::new(),
                power_delta: String::new(),
                resource_delta: String::new(),
                hook_opened: vec![hook_value.to_string()],
                hook_paid_off: Vec::new(),
                character_change: String::new(),
                world_change: String::new(),
                payoff_target: String::new(),
                new_character_requests: Vec::new(),
                character_registrations: Vec::new(),
                status: "ready".to_string(),
                created_at: String::new(),
                updated_at: String::new(),
            },
            chapter_architecture: ChapterArchitectureRecord {
                number: 1,
                title: "chapter".to_string(),
                path: String::new(),
                architecture: String::new(),
                status: "ready".to_string(),
                created_at: String::new(),
                updated_at: String::new(),
            },
            character_registrations: Vec::new(),
            role_projections: BTreeMap::new(),
            authority_root_fingerprint: String::new(),
            protected_coverage: governance::AuthorityCoverage::default(),
            sealed_at: String::new(),
        }
    }

    fn state_change(excerpt: &str, entity: &str, value: &str) -> novel_bible::ChapterStateChange {
        novel_bible::ChapterStateChange {
            change_id: "change-1".to_string(),
            entity_id: entity.to_string(),
            event_type: novel_bible::ChapterStateEventType::Character,
            value: value.to_string(),
            evidence: novel_bible::ChapterBodyEvidence {
                start_char: 99,
                end_char: 100,
                excerpt: excerpt.to_string(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn final_body_evidence_binds_a_unique_exact_excerpt() {
        let body = "风停之后，沈砚决定留下。";
        let mut change = state_change("沈砚决定留下", "沈砚", "决定留下");

        validate_final_body_evidence(
            body,
            &AuthorityEntityResolution {
                stable_id_resolved: true,
                public_surfaces: vec!["沈砚".to_string()],
            },
            &mut change,
        )
        .expect("unique excerpt should bind");

        assert_eq!(change.evidence.start_char, 5);
        assert_eq!(change.evidence.end_char, 11);
    }

    #[test]
    fn final_body_evidence_rejects_ambiguous_excerpt() {
        let body = "沈砚决定留下。沈砚决定留下。";
        let mut change = state_change("沈砚决定留下", "沈砚", "决定留下");

        let error = validate_final_body_evidence(
            body,
            &AuthorityEntityResolution {
                stable_id_resolved: true,
                public_surfaces: vec!["沈砚".to_string()],
            },
            &mut change,
        )
        .unwrap_err();

        assert!(error.contains("absent or ambiguous"));
    }

    #[test]
    fn final_body_evidence_expands_pronoun_sentence_to_nearby_public_surface() {
        let body = "闻望宁收起胶囊，望向塔顶的红灯。他不再是那个只负责修剪枝叶的园丁。";
        let mut change = state_change(
            "他不再是那个只负责修剪枝叶的园丁。",
            "character-0001",
            "他不再是那个只负责修剪枝叶的园丁。",
        );

        validate_final_body_evidence(
            body,
            &AuthorityEntityResolution {
                stable_id_resolved: true,
                public_surfaces: vec!["闻望宁".to_string()],
            },
            &mut change,
        )
        .expect("a unique same-paragraph pronoun change should bind to its named subject");

        assert!(change.evidence.excerpt.starts_with("闻望宁"));
        assert!(change.evidence.excerpt.ends_with("园丁。"));
        assert_eq!(change.value, "他不再是那个只负责修剪枝叶的园丁。");
    }

    #[test]
    fn contract_state_change_cannot_rewrite_a_paraphrase_into_exact_evidence() {
        let body = "沈砚没有放弃归山，只答应暂时留守边城。";
        let mut change = state_change("沈砚放弃归山并决定留守边城", "character-0001", "留守边城");
        let entity = AuthorityEntityResolution {
            stable_id_resolved: true,
            public_surfaces: vec!["沈砚".to_string()],
        };

        let error = validate_final_body_evidence(body, &entity, &mut change)
            .expect_err("a paraphrase or negated event must not become durable state");

        assert!(error.contains("absent or ambiguous"));
        assert_eq!(change.value, "留守边城");
    }

    #[test]
    fn entity_matching_rejects_common_one_character_tokens() {
        assert!(!json_value_mentions_entity(
            &json!("城门已经关闭"),
            &normalize_evidence_text("门")
        ));
        assert!(json_value_mentions_entity(
            &json!("沈砚已经离开"),
            &normalize_evidence_text("沈砚")
        ));
    }

    #[test]
    fn final_body_evidence_resolves_internal_character_id_to_public_name() {
        let body = "风停之后，沈砚决定留下。";
        let mut change = state_change("沈砚决定留下", "character-0001", "决定留下");

        validate_final_body_evidence(
            body,
            &AuthorityEntityResolution {
                stable_id_resolved: true,
                public_surfaces: vec!["沈砚".to_string()],
            },
            &mut change,
        )
        .expect("internal id should bind through the canonical public name");
    }

    #[test]
    fn entity_surface_lookup_reads_canonical_name_from_stable_id() {
        let mut resolution = AuthorityEntityResolution::default();
        collect_entity_resolution_from_json(
            &json!({"characters": [{
                "character_id": "character-0001",
                "canonical_name": "沈砚"
            }]}),
            &normalize_evidence_text("character-0001"),
            &mut resolution,
        );

        assert!(resolution.stable_id_resolved);
        assert_eq!(resolution.public_surfaces, vec!["沈砚".to_string()]);
    }

    #[test]
    fn surface_less_stable_world_rule_id_can_bind_verbatim_body_value() {
        let body = "阵眼崩裂后，灵脉永久停止向王城输送。";
        let mut change = state_change(
            "灵脉永久停止向王城输送",
            "world-rule-0001",
            "永久停止向王城输送",
        );

        validate_final_body_evidence(
            body,
            &AuthorityEntityResolution {
                stable_id_resolved: true,
                public_surfaces: Vec::new(),
            },
            &mut change,
        )
        .expect(
            "a sealed surface-less rule id should rely on its verbatim value and authority path",
        );
    }

    #[test]
    fn unresolved_internal_id_still_cannot_bind_state() {
        let body = "阵眼崩裂后，灵脉永久停止向王城输送。";
        let mut change = state_change(
            "灵脉永久停止向王城输送",
            "world-rule-9999",
            "永久停止向王城输送",
        );

        let error =
            validate_final_body_evidence(body, &AuthorityEntityResolution::default(), &mut change)
                .unwrap_err();
        assert!(error.contains("public entity surface"));
    }

    #[test]
    fn legacy_hook_open_event_deserializes_as_typed_seed() {
        let event: novel_bible::ChapterStateEventType =
            serde_json::from_str("\"hook_open\"").expect("legacy observer synonym");
        assert_eq!(event, novel_bible::ChapterStateEventType::HookSeed);
    }

    #[test]
    fn unrealized_allowed_hook_is_not_a_required_state_transition() {
        let authority = hook_authority("尚未发生的允许伏笔", json!([]));
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 8,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let raw = serde_json::json!({
            "current_state": "沈砚仍在城门外等待。",
            "chapter_summary": "沈砚在城门外等待。",
            "state_changes": []
        })
        .to_string();

        let (_, validation, _, parse_error) = validated_settlement_from_final_body(
            &raw,
            "沈砚仍在城门外等待。",
            &chapter,
            &authority,
        );

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
    }

    #[test]
    fn required_end_state_cannot_silently_pass_without_a_typed_delta() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_contract.new_state_after_chapter =
            "沈砚已取得旧城密钥并离开石室".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "沈砚"
            }]
        });
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 12,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "沈砚收起旧城密钥，推门离开石室。";
        let raw = json!({
            "current_state": "沈砚带着旧城密钥离开石室。",
            "chapter_summary": "沈砚取得密钥后离开石室。",
            "state_changes": []
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body(&raw, body, &chapter, &authority);

        assert!(parse_error.is_none());
        assert!(settlement.state_changes.is_empty());
        assert!(!validation.passed);
        assert!(validation
            .warnings
            .iter()
            .any(|warning| warning.contains("required typed end-state change")));
    }

    #[test]
    fn required_end_state_accepts_final_body_evidence_through_its_sealed_path() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_contract.new_state_after_chapter =
            "沈砚已取得旧城密钥并离开石室".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "沈砚"
            }]
        });
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 12,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "沈砚收起旧城密钥，推门离开石室。";
        let raw = json!({
            "current_state": "沈砚带着旧城密钥离开石室。",
            "chapter_summary": "沈砚取得密钥后离开石室。",
            "state_changes": [{
                "entity_id": "character-0001",
                "event_type": "character",
                "value": body,
                "evidence": {"excerpt": body},
                "authority_path": "chapter_contract.new_state_after_chapter",
                "authority_excerpt": "模型改写的错误权威"
            }]
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body(&raw, body, &chapter, &authority);

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
        assert_eq!(settlement.state_changes.len(), 1);
        assert_eq!(
            settlement.state_changes[0].authority_excerpt,
            authority.chapter_contract.new_state_after_chapter
        );
    }

    #[test]
    fn exhausted_observer_recovers_unique_required_character_state_from_final_body() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 4;
        authority.chapter_contract.number = 4;
        authority.chapter_contract.new_state_after_chapter =
            "主角进入意识层面的感知状态".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "顾景真",
                "role": "主角"
            }]
        });
        let chapter = ChapterRecord {
            number: 4,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 12,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "顾景真感觉意识正在脱离肉体的束缚。他进入了一种非物质的流动状态。";
        let raw = json!({
            "current_state": "顾景真进入非物质感知状态。",
            "chapter_summary": "顾景真通过共振越过物理感官边界。",
            "state_changes": []
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body_after_observer_exhaustion(
                &raw, body, &chapter, &authority,
            );

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
        assert_eq!(settlement.state_changes.len(), 1);
        let recovered = &settlement.state_changes[0];
        assert_eq!(recovered.entity_id, "character-0001");
        assert_eq!(
            recovered.authority_path,
            "chapter_contract.new_state_after_chapter"
        );
        assert_eq!(
            recovered.allowance,
            novel_bible::StateChangeAllowance::Contract
        );
        assert!(recovered.evidence.excerpt.contains("顾景真"));
        assert!(recovered.evidence.excerpt.contains("进入"));
    }

    #[test]
    fn exhausted_observer_replaces_malformed_required_delta_with_final_body_recovery() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 4;
        authority.chapter_contract.number = 4;
        authority.chapter_contract.new_state_after_chapter =
            "主角进入意识层面的感知状态".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "顾景真",
                "role": "主角"
            }]
        });
        let chapter = ChapterRecord {
            number: 4,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 34,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "顾景真感觉意识正在脱离肉体的束缚。他进入了一种非物质的流动状态。";
        let raw = json!({
            "current_state": "顾景真进入非物质感知状态。",
            "chapter_summary": "顾景真通过共振越过物理感官边界。",
            "state_changes": [{
                "entity_id": "character-0001",
                "event_type": "character",
                "value": "顾景真已经完全掌控所有非物质感知",
                "evidence": {"excerpt": body},
                "authority_path": "chapter_contract.new_state_after_chapter"
            }]
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body_after_observer_exhaustion(
                &raw, body, &chapter, &authority,
            );

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
        assert_eq!(settlement.state_changes.len(), 1);
        assert_eq!(
            settlement.state_changes[0].authority_path,
            "chapter_contract.new_state_after_chapter"
        );
        assert!(settlement.state_changes[0]
            .evidence
            .excerpt
            .contains("进入"));
        assert!(validation
            .advisories
            .iter()
            .any(|item| item.contains("replaced malformed observer proposals")));
        assert!(!validation
            .warnings
            .iter()
            .any(|item| item.contains("unauthorized state delta")));
    }

    #[test]
    fn exhausted_observer_does_not_misclassify_named_state_from_broad_payoff_similarity() {
        let mut authority = hook_authority(
            "",
            json!([
                {
                    "id": "hook-cause",
                    "title": "核心器物损坏引发环境衰竭",
                    "reader_knows": "核心器物损坏引发环境衰竭"
                },
                {
                    "id": "hook-ending",
                    "title": "修复核心器物并终结环境衰竭",
                    "reader_knows": "修复核心器物并终结环境衰竭"
                }
            ]),
        );
        authority.chapter_contract.new_state_after_chapter =
            "陶屿安在十岁时重现，并意识到青铜鼎已处于残缺状态，灵气正处于枯竭临界点。".to_string();
        authority.chapter_contract.payoff_target =
            "建立重生基调，引入青铜鼎残片并确立灵气枯竭危机。".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "陶屿安",
                "role": "主角"
            }]
        });
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 48,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "陶屿安在十岁时重新睁开眼。他确认青铜鼎只剩残缺碎片，而天地灵气已逼近彻底枯竭的临界点。";
        let raw = json!({
            "current_state": "陶屿安已经确认青铜鼎残缺且灵气濒临枯竭。",
            "chapter_summary": "陶屿安重现于十岁并确认当前危机。",
            "state_changes": []
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body_after_observer_exhaustion(
                &raw, body, &chapter, &authority,
            );

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
        assert_eq!(settlement.state_changes.len(), 1);
        assert_eq!(
            settlement.state_changes[0].event_type,
            novel_bible::ChapterStateEventType::Character
        );
        assert_eq!(settlement.state_changes[0].entity_id, "character-0001");
    }

    #[test]
    fn exhausted_observer_skips_name_only_span_and_recovers_the_required_event_span() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_contract.new_state_after_chapter =
            "利用旧时代的遮蔽装置救下了即将暴露的裴予朔".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "裴予朔",
                "role": "主角"
            }]
        });
        let chapter = ChapterRecord {
            number: 2,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 50,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "酸雨敲打窗棂，裴予朔听见机械脉搏般的回声。姜云野启动旧时代遮蔽装置，把即将暴露的裴予朔救进废墟。";

        let recovered = recover_explicit_required_state_change(&chapter, body, &authority, &[])
            .expect("the later required event span should recover");

        assert!(recovered.evidence.excerpt.contains("遮蔽装置"));
        assert!(recovered.evidence.excerpt.contains("救"));
        assert_ne!(
            recovered.evidence.excerpt,
            "酸雨敲打窗棂，裴予朔听见机械脉搏般的回声。"
        );
    }

    #[test]
    fn exhausted_observer_recovers_each_non_hook_required_state_class() {
        use novel_bible::ChapterStateEventType as Event;

        for event in [
            Event::Character,
            Event::Relationship,
            Event::World,
            Event::Power,
            Event::Resource,
        ] {
            let mut authority = hook_authority("", json!([]));
            let required = "沈砚取得铜印并成为守门人".to_string();
            authority.chapter_contract.new_state_after_chapter = required.clone();
            match event {
                Event::Character => authority.chapter_contract.character_change = required.clone(),
                Event::Relationship => {
                    authority.chapter_contract.relationship_delta = required.clone()
                }
                Event::World => authority.chapter_contract.world_change = required.clone(),
                Event::Power => authority.chapter_contract.power_delta = required.clone(),
                Event::Resource => authority.chapter_contract.resource_delta = required.clone(),
                _ => unreachable!(),
            }
            authority.canonical_contract = json!({
                "characters": [{
                    "character_id": "character-0001",
                    "canonical_name": "沈砚",
                    "role": "主角"
                }]
            });
            let chapter = ChapterRecord {
                number: 1,
                title: "chapter".to_string(),
                volume_id: String::new(),
                volume_title: String::new(),
                path: String::new(),
                summary: String::new(),
                unit_count: required.chars().count(),
                status: "draft".to_string(),
                key_facts: Vec::new(),
                continuity_updates: Vec::new(),
                created_at: String::new(),
                updated_at: String::new(),
            };

            let recovered =
                recover_explicit_required_state_change(&chapter, &required, &authority, &[])
                    .unwrap_or_else(|| panic!("required {event:?} state should recover"));

            assert_eq!(recovered.event_type, event);
            assert_eq!(recovered.entity_id, "character-0001");
            assert_eq!(
                recovered.authority_path,
                "chapter_contract.new_state_after_chapter"
            );
        }
    }

    #[test]
    fn exhausted_observer_classifies_single_named_character_without_companion_field() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 2;
        authority.chapter_contract.number = 2;
        authority.chapter_contract.new_state_after_chapter =
            "秦屿桥在护送物资时为了保护驿站规矩受了轻伤".to_string();
        authority.chapter_contract.goal =
            "应对军官对物资的强行索要，秦屿桥必须守住驿站规矩".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0002",
                "canonical_name": "秦屿桥",
                "role": "关键关系对象"
            }]
        });
        let chapter = ChapterRecord {
            number: 2,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 32,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "秦屿桥挡在货架前守住驿站规矩，推搡中侧肋被重袋擦过。他虽然受了轻伤，仍坚持把物资清点完毕。";

        let recovered = recover_explicit_required_state_change(&chapter, body, &authority, &[])
            .expect("a named character end state should recover without character_change");

        assert_eq!(
            recovered.event_type,
            novel_bible::ChapterStateEventType::Character
        );
        assert_eq!(recovered.entity_id, "character-0002");
        assert_eq!(
            recovered.authority_path,
            "chapter_contract.new_state_after_chapter"
        );
        assert!(recovered.evidence.excerpt.contains("受了轻伤"));
    }

    #[test]
    fn exhausted_observer_uses_earliest_valid_window_for_repeated_required_state() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 1;
        authority.chapter_contract.number = 1;
        authority.chapter_contract.new_state_after_chapter =
            "顾景真进入意识层面的感知状态".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "顾景真",
                "role": "主角"
            }]
        });
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 30,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "顾景真进入非物质感知状态。随后顾景真又进入非物质感知状态。";

        let recovered = recover_explicit_required_state_change(&chapter, body, &authority, &[])
            .expect("a repeated required state should have one deterministic recovery");

        assert_eq!(recovered.entity_id, "character-0001");
        assert_eq!(recovered.evidence.start_char, 0);
        assert!(recovered
            .evidence
            .excerpt
            .starts_with("顾景真进入非物质感知状态"));
    }

    #[test]
    fn exhausted_observer_prefers_canonical_character_prefix_over_location_anchor() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 1;
        authority.chapter_contract.number = 1;
        authority.chapter_contract.new_state_after_chapter =
            "阮栖舟在荒野中发现了一处蕴含微弱生机的残余灵脉".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "阮栖舟",
                "role": "主角"
            }]
        });
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 32,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "阮栖舟在荒野中发现了一处蕴含微弱生机的残余灵脉，便将药种埋在岩缝旁。";

        let recovered = recover_explicit_required_state_change(&chapter, body, &authority, &[])
            .expect("a canonical character prefix must not be downgraded to a world anchor");

        assert_eq!(
            recovered.event_type,
            novel_bible::ChapterStateEventType::Character
        );
        assert_eq!(recovered.entity_id, "character-0001");
        assert!(recovered.evidence.excerpt.contains("发现了一处"));
    }

    #[test]
    fn exhausted_observer_does_not_let_companion_similarity_override_named_character_state() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 2;
        authority.chapter_contract.number = 2;
        authority.chapter_contract.new_state_after_chapter =
            "闻云岚向许谨真展示了被企业丢弃的废弃记忆".to_string();
        authority.chapter_contract.power_delta =
            "许谨真的神经稳定性因观察废弃记忆而进一步下降".to_string();
        authority.canonical_contract = json!({
            "characters": [
                {
                    "character_id": "character-0001",
                    "canonical_name": "许谨真",
                    "role": "主角"
                },
                {
                    "character_id": "character-0002",
                    "canonical_name": "闻云岚",
                    "role": "关键关系对象"
                }
            ]
        });
        let chapter = ChapterRecord {
            number: 2,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 32,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "闻云岚向许谨真展示了被企业丢弃的废弃记忆，许谨真因此感到神经稳定性继续下降。";

        let recovered = recover_explicit_required_state_change(&chapter, body, &authority, &[])
            .expect("the required named character state must win over a similar power delta");

        assert_eq!(
            recovered.event_type,
            novel_bible::ChapterStateEventType::Character
        );
        assert_eq!(recovered.entity_id, "character-0002");
        assert!(recovered.evidence.excerpt.contains("展示了"));
    }

    #[test]
    fn exhausted_observer_recovers_collective_required_relationship_state() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 8;
        authority.chapter_contract.number = 8;
        authority.chapter_contract.new_state_after_chapter =
            "双方在生死存亡之际展开了关于密令含义的最终解读。".to_string();
        authority.chapter_contract.relationship_delta =
            "陆沉舟与沈砚舟从建立信任正式进入生死共担状态。".to_string();
        authority.canonical_contract = json!({
            "characters": [
                {
                    "character_id": "character-0001",
                    "canonical_name": "陆沉舟",
                    "role": "主角"
                },
                {
                    "character_id": "character-0002",
                    "canonical_name": "沈砚舟",
                    "role": "关键关系对象"
                }
            ]
        });
        let contract =
            serde_json::from_value::<NovelCreationContract>(authority.canonical_contract.clone())
                .expect("test contract should deserialize");
        assert_eq!(
            crate::tool::writing::creation_contract::relationship_names_from_line(
                &authority.chapter_contract.relationship_delta,
                &contract.characters,
            ),
            vec!["陆沉舟".to_string(), "沈砚舟".to_string()]
        );
        let chapter = ChapterRecord {
            number: 8,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 40,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body =
            "陆沉舟握住沈砚舟的手腕，两人在生死一线间完成了关于密令含义的最终解读，随后共同撤退。";
        let raw = json!({
            "current_state": "双方已经完成密令解读并共同撤退。",
            "chapter_summary": "陆沉舟与沈砚舟在围堵中建立生死共担关系。",
            "state_changes": []
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body_after_observer_exhaustion(
                &raw, body, &chapter, &authority,
            );

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
        assert_eq!(settlement.state_changes.len(), 1);
        assert_eq!(
            settlement.state_changes[0].event_type,
            novel_bible::ChapterStateEventType::Relationship
        );
        assert_eq!(settlement.state_changes[0].entity_id, "character-0001");
        assert_eq!(
            settlement.state_changes[0].authority_path,
            "chapter_contract.new_state_after_chapter"
        );
        assert!(settlement.state_changes[0]
            .evidence
            .excerpt
            .contains("沈砚舟"));
    }

    #[test]
    fn exhausted_observer_recovers_required_object_state_from_sealed_payoff_anchor() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_contract.new_state_after_chapter =
            "借火令引发了师父留下的残魂幻象".to_string();
        authority.chapter_contract.payoff_target =
            "引入核心道具借火令，建立主角的初始动力源。".to_string();
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "沈砚川",
                "role": "主角"
            }]
        });
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 32,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "沈砚川握紧借火令，令牌引发了师父留下的残魂幻象。";
        let raw = json!({
            "current_state": "借火令已经引发师父残魂幻象。",
            "chapter_summary": "沈砚川借助令牌看见师父残魂。",
            "state_changes": []
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body_after_observer_exhaustion(
                &raw, body, &chapter, &authority,
            );

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
        assert_eq!(settlement.state_changes.len(), 1);
        assert_eq!(
            settlement.state_changes[0].event_type,
            novel_bible::ChapterStateEventType::World
        );
        assert_eq!(settlement.state_changes[0].entity_id, "借火令");
        assert!(settlement.state_changes[0]
            .evidence
            .excerpt
            .contains("借火令"));
    }

    #[test]
    fn exhausted_observer_recovers_required_object_state_from_chapter_goal_anchor() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_contract.new_state_after_chapter = "遭遇能源管理者的直接干预".to_string();
        authority.chapter_contract.goal =
            "在能源管理者的封锁下完成坐标解析并保住医疗站".to_string();
        authority.chapter_contract.conflict = "能源管理者直接干预医疗站的电力配额".to_string();
        authority.canonical_contract = json!({
            "world_rules": ["能源管理者控制医疗站电力"]
        });
        let chapter = ChapterRecord {
            number: 4,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 40,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "宋栖言宣布能源封锁，能源管理者的巡视员进入医疗站，直接干预了电力配额。";

        let recovered = recover_explicit_required_state_change(&chapter, body, &authority, &[])
            .expect("the required object outcome should recover from the chapter goal");

        assert_eq!(
            recovered.event_type,
            novel_bible::ChapterStateEventType::World
        );
        assert_eq!(recovered.entity_id, "能源管理者");
        assert!(recovered.evidence.excerpt.contains("能源管理者"));
    }

    #[test]
    fn exhausted_observer_recovers_property_state_from_sealed_chapter_contract() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_contract.new_state_after_chapter =
            "芯片病毒的扩散程度决定了逃亡的紧迫性".to_string();
        authority.chapter_contract.goal = "在芯片病毒扩散前抵达稳定计算平台".to_string();
        authority.chapter_contract.conflict =
            "芯片病毒的扩散速度与两人的移动效率形成矛盾".to_string();
        authority.canonical_contract = json!({
            "world_rules": ["芯片病毒会沿意识连接扩散"]
        });
        let chapter = ChapterRecord {
            number: 4,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 40,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "芯片病毒的扩散率已经达到42%，两人冲入稳定计算分区后，扩散速度暂时降了下来，但逃亡的紧迫性没有消失。";

        let recovered = recover_explicit_required_state_change(&chapter, body, &authority, &[])
            .expect(
                "the required property outcome should recover from the sealed chapter contract",
            );

        assert_eq!(
            recovered.event_type,
            novel_bible::ChapterStateEventType::World
        );
        assert_eq!(recovered.entity_id, "芯片病毒");
        assert!(recovered.evidence.excerpt.contains("芯片病毒"));
    }

    #[test]
    fn exhausted_observer_does_not_guess_character_for_unnamed_required_outcome() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 4;
        authority.chapter_contract.number = 4;
        authority.chapter_contract.new_state_after_chapter =
            "样衣获得市场初步反馈，资金开始回笼。".to_string();
        authority.canonical_contract = json!({
            "characters": [
                {
                    "character_id": "character-0001",
                    "canonical_name": "唐昭遥",
                    "role": "主角"
                },
                {
                    "character_id": "character-0002",
                    "canonical_name": "梁泊澜",
                    "role": "关键关系对象"
                }
            ]
        });
        let chapter = ChapterRecord {
            number: 4,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 36,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body =
            "唐昭遥向摊主展示了样衣，获得了真实的市场反馈。她接过第一笔货款，资金终于开始回笼。";
        let raw = json!({
            "current_state": "唐昭遥完成样衣试销并回笼第一笔资金。",
            "chapter_summary": "样衣通过市场初步验证。",
            "state_changes": []
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body_after_observer_exhaustion(
                &raw, body, &chapter, &authority,
            );

        assert!(parse_error.is_none());
        assert!(!validation.passed);
        assert!(settlement.state_changes.is_empty());
        assert!(validation
            .warnings
            .iter()
            .any(|warning| warning.contains("missing the required typed end-state change")));
    }

    #[test]
    fn exhausted_observer_uses_leading_actor_when_required_state_mentions_another_character() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 2;
        authority.chapter_contract.number = 2;
        authority.chapter_contract.new_state_after_chapter =
            "秦昭原发现沈栖声采购的材料纯度远超行业平均水平".to_string();
        authority.canonical_contract = json!({
            "characters": [
                {
                    "character_id": "character-0001",
                    "canonical_name": "沈栖声",
                    "role": "女主"
                },
                {
                    "character_id": "character-0002",
                    "canonical_name": "秦昭原",
                    "role": "关键关系对象"
                }
            ]
        });
        let chapter = ChapterRecord {
            number: 2,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 24,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "秦昭原盯着沈栖声手里的报告，确认她采购的材料纯度远超行业平均水平。";
        let raw = json!({
            "current_state": "秦昭原确认材料纯度远超行业平均水平。",
            "chapter_summary": "秦昭原完成高精度检测。",
            "state_changes": []
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body_after_observer_exhaustion(
                &raw, body, &chapter, &authority,
            );

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
        assert_eq!(settlement.state_changes.len(), 1);
        assert_eq!(settlement.state_changes[0].entity_id, "character-0002");
    }

    #[test]
    fn exhausted_observer_does_not_recover_name_only_or_ambiguous_character_state() {
        let mut authority = hook_authority("", json!([]));
        authority.chapter_number = 4;
        authority.chapter_contract.number = 4;
        authority.chapter_contract.new_state_after_chapter =
            "主角进入意识层面的感知状态".to_string();
        authority.canonical_contract = json!({
            "characters": [
                {
                    "character_id": "character-0001",
                    "canonical_name": "顾景真",
                    "role": "主角"
                },
                {
                    "character_id": "character-0002",
                    "canonical_name": "程昭禾",
                    "role": "主角"
                }
            ]
        });
        let chapter = ChapterRecord {
            number: 4,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 12,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let raw = json!({
            "current_state": "两人仍在探测器内。",
            "chapter_summary": "顾景真和程昭禾继续等待。",
            "state_changes": []
        })
        .to_string();

        let (settlement, validation, _, _) =
            validated_settlement_from_final_body_after_observer_exhaustion(
                &raw,
                "顾景真看着程昭禾，两人继续等待。",
                &chapter,
                &authority,
            );

        assert!(settlement.state_changes.is_empty());
        assert!(!validation.passed);
        assert!(validation
            .warnings
            .iter()
            .any(|warning| warning.contains("required typed end-state change")));
    }

    #[test]
    fn required_end_state_replaces_parallel_delta_for_the_same_typed_slot() {
        use novel_bible::{ChapterStateChange, ChapterStateEventType};

        let changes = vec![
            ChapterStateChange {
                entity_id: "character-0001".to_string(),
                event_type: ChapterStateEventType::Character,
                authority_path: "chapter_contract.character_change".to_string(),
                value: "旧的并行描述".to_string(),
                ..Default::default()
            },
            ChapterStateChange {
                entity_id: "character-0001".to_string(),
                event_type: ChapterStateEventType::Character,
                authority_path: "chapter_contract.new_state_after_chapter".to_string(),
                value: "章末最终状态".to_string(),
                ..Default::default()
            },
        ];

        let deduped = dedupe_required_end_state_changes(changes);

        assert_eq!(deduped.len(), 1);
        assert_eq!(
            deduped[0].authority_path,
            "chapter_contract.new_state_after_chapter"
        );
        assert_eq!(deduped[0].value, "章末最终状态");
    }

    #[test]
    fn display_hook_resolution_without_typed_payoff_is_discarded() {
        let authority = hook_authority("", json!([]));
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 12,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "沈砚在城门外停下。";
        let raw = json!({
            "current_state": "沈砚仍在城门外。",
            "chapter_summary": "沈砚在城门外停下。",
            "resolved_hooks": ["旧城密钥已回收"],
            "state_changes": []
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body(&raw, body, &chapter, &authority);

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
        assert!(settlement.resolved_hooks.is_empty());
    }

    #[test]
    fn resolved_hooks_are_derived_from_the_matching_validated_typed_payoff() {
        let mut authority = hook_authority(
            "",
            json!([{"id": "hook-old-city-key", "title": "旧城密钥已回收"}]),
        );
        authority.chapter_contract.hook_paid_off = vec!["旧城密钥已回收".to_string()];
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 12,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "沈砚从石匣中取出旧城密钥，确认封锁已经解除。";
        let raw = json!({
            "current_state": "沈砚已经取回旧城密钥。",
            "chapter_summary": "沈砚取回旧城密钥并解除封锁。",
            "resolved_hooks": ["模型自行改写的错误标签"],
            "state_changes": [{
                "entity_id": "model-invented-hook-id",
                "event_type": "hook_pay_off",
                "value": body,
                "evidence": {"excerpt": body},
                "authority_path": "chapter_contract.hook_paid_off/0",
                "authority_excerpt": "模型自行改写的错误标签"
            }]
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body(&raw, body, &chapter, &authority);

        assert!(parse_error.is_none());
        assert!(validation.passed, "{:?}", validation.warnings);
        assert_eq!(settlement.resolved_hooks, ["旧城密钥已回收"]);
        assert_eq!(settlement.state_changes.len(), 1);
        assert_eq!(settlement.state_changes[0].entity_id, "hook-old-city-key");
    }

    #[test]
    fn evidence_backed_unauthorized_optional_delta_blocks_without_polluting_state() {
        let mut authority = hook_authority("", json!([]));
        authority.canonical_contract = json!({
            "characters": [{
                "character_id": "character-0001",
                "canonical_name": "沈砚"
            }]
        });
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: 12,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let body = "沈砚收起石片，决定天亮后离开山谷。";
        let raw = json!({
            "current_state": "沈砚收起石片并准备离开山谷。",
            "chapter_summary": "沈砚取得石片后决定离开山谷。",
            "state_changes": [{
                "entity_id": "character-0001",
                "event_type": "character",
                "value": body,
                "evidence": {"excerpt": body},
                "authority_path": "chapter_plan.plan",
                "authority_excerpt": "取得石片"
            }]
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body(&raw, body, &chapter, &authority);

        assert!(parse_error.is_none());
        assert!(!validation.passed, "{:?}", validation.warnings);
        assert_eq!(
            validation.disposition,
            StateSettlementDisposition::DependencyMismatch
        );
        assert!(settlement.state_changes.is_empty());
        assert!(validation
            .warnings
            .iter()
            .any(|item| item.contains("rejected evidence-backed unauthorized state delta")));
    }

    #[test]
    fn sealed_hook_path_binds_event_excerpt_and_stable_local_id() {
        let authority = hook_authority("无声雷音首次显现", json!([]));
        let mut first = novel_bible::ChapterStateChange {
            entity_id: "hook-0000".to_string(),
            event_type: novel_bible::ChapterStateEventType::HookAdvance,
            authority_path: "chapter_contract.hook_opened/0".to_string(),
            authority_excerpt: "model paraphrase".to_string(),
            ..Default::default()
        };
        let mut replay = first.clone();
        replay.entity_id = "hook-9999".to_string();

        bind_contract_authority(&authority, &mut first);
        bind_contract_authority(&authority, &mut replay);

        assert_eq!(
            first.event_type,
            novel_bible::ChapterStateEventType::HookSeed
        );
        assert_eq!(first.authority_excerpt, "无声雷音首次显现");
        assert!(first.entity_id.starts_with("hook-seed-"));
        assert_eq!(first.entity_id, replay.entity_id);
    }

    #[test]
    fn sealed_hook_path_reuses_existing_truth_hook_id() {
        let authority = hook_authority(
            "无声雷音首次显现",
            json!([{
                "id": "hook-0005",
                "title": "无声雷音首次显现",
                "reader_knows": "无声雷音首次显现",
                "evidence": ["无声雷音首次显现"]
            }]),
        );
        let mut change = novel_bible::ChapterStateChange {
            entity_id: "model-invented-id".to_string(),
            event_type: novel_bible::ChapterStateEventType::HookSeed,
            authority_path: "chapter_contract.hook_opened/0".to_string(),
            ..Default::default()
        };

        bind_contract_authority(&authority, &mut change);

        assert_eq!(change.entity_id, "hook-0005");
    }

    #[test]
    fn required_end_state_uniquely_resolves_an_existing_hook_advance() {
        let mut authority = hook_authority(
            "",
            json!([
                {
                    "id": "hook-lampwick",
                    "title": "寻找失落的灯芯",
                    "reader_knows": "寻找失落的灯芯",
                    "evidence": ["集齐灯芯后点亮青灯"]
                },
                {
                    "id": "hook-antagonist",
                    "title": "梁晏朔的吞噬欲望",
                    "reader_knows": "梁晏朔的吞噬欲望",
                    "evidence": ["终局对抗梁晏朔"]
                }
            ]),
        );
        authority.chapter_contract.new_state_after_chapter =
            "南听宁发现必须寻找散落的第三枚灯芯".to_string();
        let mut change = novel_bible::ChapterStateChange {
            entity_id: "model-invented-id".to_string(),
            event_type: novel_bible::ChapterStateEventType::HookAdvance,
            authority_path: "chapter_contract.new_state_after_chapter".to_string(),
            ..Default::default()
        };

        bind_contract_authority(&authority, &mut change);

        assert_eq!(change.entity_id, "hook-lampwick");
        assert_eq!(
            change.authority_excerpt,
            "南听宁发现必须寻找散落的第三枚灯芯"
        );
    }

    #[test]
    fn exhausted_observer_recovers_required_existing_hook_advance() {
        let required = "沈砚确认必须寻找失落的灯芯";
        let mut authority = hook_authority(
            "",
            json!([{
                "id": "hook-lampwick",
                "title": "寻找失落的灯芯",
                "reader_knows": "寻找失落的灯芯"
            }]),
        );
        authority.chapter_contract.new_state_after_chapter = required.to_string();
        authority.chapter_contract.payoff_target = required.to_string();
        let chapter = ChapterRecord {
            number: 1,
            title: "chapter".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: String::new(),
            summary: String::new(),
            unit_count: required.chars().count(),
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };

        let recovered = recover_explicit_required_state_change(&chapter, required, &authority, &[])
            .expect("required hook advance should recover");

        assert_eq!(
            recovered.event_type,
            novel_bible::ChapterStateEventType::HookAdvance
        );
        assert_eq!(recovered.entity_id, "hook-lampwick");
    }

    #[test]
    fn semantic_hook_resolution_refuses_ambiguous_candidates() {
        let mut authority = hook_authority(
            "",
            json!([
                {"id": "hook-east", "title": "寻找东方灯芯"},
                {"id": "hook-west", "title": "寻找西方灯芯"}
            ]),
        );
        authority.chapter_contract.new_state_after_chapter = "主角开始寻找灯芯".to_string();
        let mut change = novel_bible::ChapterStateChange {
            entity_id: "unresolved-hook".to_string(),
            event_type: novel_bible::ChapterStateEventType::HookAdvance,
            authority_path: "chapter_contract.new_state_after_chapter".to_string(),
            ..Default::default()
        };

        bind_contract_authority(&authority, &mut change);

        assert_eq!(change.entity_id, "unresolved-hook");
    }
}
