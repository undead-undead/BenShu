use super::*;
use sha2::{Digest, Sha256};

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

pub(super) fn legacy_zero_change_degraded_settlement(
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
    let parsed = parse_explicit_settlement_output(raw_observation);
    let (mut settlement, source, parse_error) = match parsed {
        Ok(settlement) => (settlement, SettlementSource::FinalBodyObserver, None),
        Err(error) => (
            legacy_zero_change_degraded_settlement(
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
    let validation = validate_and_bind_settlement(chapter, body, authority, &mut settlement);
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
        passed: warnings.is_empty(),
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
    validate_and_bind_settlement(chapter, content, authority, &mut checked)
}

fn validate_and_bind_settlement(
    chapter: &ChapterRecord,
    content: &str,
    authority: &governance::SealedChapterAuthority,
    settlement: &mut SettlementOutput,
) -> StateValidationOutput {
    let mut validation = deterministic_state_validation(content, settlement);
    if !settlement.degraded_reason.trim().is_empty() {
        validation.warnings.push(format!(
            "state observer degraded: {}",
            settlement.degraded_reason.trim()
        ));
    }
    let expected_body = chapter_quality::chapter_body_fingerprint(content);
    if settlement.body_fingerprint != expected_body {
        validation
            .warnings
            .push("settlement belongs to a different final body".to_string());
    }
    if settlement.authority_fingerprint != authority.authority_root_fingerprint {
        validation
            .warnings
            .push("settlement belongs to a different sealed chapter authority".to_string());
    }
    if authority.chapter_number != chapter.number {
        validation
            .warnings
            .push("sealed authority belongs to a different chapter".to_string());
    }

    let proposed_changes = std::mem::take(&mut settlement.state_changes);
    let mut accepted_changes = Vec::with_capacity(proposed_changes.len());
    for (index, mut change) in proposed_changes.into_iter().enumerate() {
        change.change_id = format!("chapter-{:04}-change-{:04}", chapter.number, index + 1);
        bind_contract_authority(authority, &mut change);
        if state_change_claims_forbidden_transition(&change) {
            change.allowance = novel_bible::StateChangeAllowance::Rejected;
            validation.warnings.push(format!(
                "state change {} claims a high-risk transition outside deterministic approval",
                change.change_id.trim()
            ));
            continue;
        }
        let entity = authority_entity_resolution(authority, &change.entity_id);
        if let Err(error) = validate_final_body_evidence(content, &entity, &mut change) {
            change.allowance = novel_bible::StateChangeAllowance::Rejected;
            validation
                .advisories
                .push(format!("ignored untrusted typed delta: {error}"));
            continue;
        }
        change.allowance = match authority_allowance(authority, &change) {
            Ok(allowance) => allowance,
            Err(error) => {
                change.allowance = novel_bible::StateChangeAllowance::Rejected;
                validation
                    .advisories
                    .push(format!("ignored untrusted typed delta: {error}"));
                continue;
            }
        };
        if change.event_type == novel_bible::ChapterStateEventType::HookDefer
            && !change
                .defer_until_chapter
                .is_some_and(|number| number > chapter.number)
        {
            change.allowance = novel_bible::StateChangeAllowance::Rejected;
            validation.advisories.push(format!(
                "ignored untrusted typed delta: state change {} defers a hook without a later chapter",
                change.change_id.trim()
            ));
            continue;
        }
        accepted_changes.push(change);
    }
    settlement.state_changes = dedupe_required_end_state_changes(accepted_changes);
    if !authority
        .chapter_contract
        .new_state_after_chapter
        .trim()
        .is_empty()
        && !settlement.state_changes.iter().any(|change| {
            change.authority_path.trim() == "chapter_contract.new_state_after_chapter"
        })
    {
        validation.warnings.push(
            "final-body settlement is missing the required typed end-state change from chapter_contract.new_state_after_chapter"
                .to_string(),
        );
    }
    settlement.resolved_hooks =
        validated_resolved_hook_labels(authority, &settlement.state_changes);

    validation.warnings.sort();
    validation.warnings.dedup();
    validation.advisories.sort();
    validation.advisories.dedup();
    validation.passed = validation.warnings.is_empty();
    validation
}

/// `new_state_after_chapter` is the required outcome assertion for this
/// chapter, not a second durable state slot. When the observer also emits an
/// optional typed field for the same entity and event type, keep the required
/// outcome delta and discard the overlapping optional delta so application
/// order cannot overwrite the final state with a parallel description.
fn dedupe_required_end_state_changes(
    changes: Vec<novel_bible::ChapterStateChange>,
) -> Vec<novel_bible::ChapterStateChange> {
    const REQUIRED_PATH: &str = "chapter_contract.new_state_after_chapter";
    let required_slots = changes
        .iter()
        .filter(|change| change.authority_path.trim() == REQUIRED_PATH)
        .map(|change| (change.event_type, change.entity_id.trim().to_string()))
        .collect::<Vec<_>>();
    let mut kept_required_slots = Vec::new();
    changes
        .into_iter()
        .filter(|change| {
            let slot = (change.event_type, change.entity_id.trim().to_string());
            if change.authority_path.trim() == REQUIRED_PATH {
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

fn state_change_claims_forbidden_transition(change: &novel_bible::ChapterStateChange) -> bool {
    change.changes_identity
        || change.changes_core_ability
        || change.changes_bottom_line
        || change.changes_world_hard_rule
        || change.pays_future_hook_early
        || change.opens_new_mainline
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
    let hook_event = matches!(
        change.event_type,
        novel_bible::ChapterStateEventType::HookSeed
            | novel_bible::ChapterStateEventType::HookAdvance
            | novel_bible::ChapterStateEventType::HookPayOff
            | novel_bible::ChapterStateEventType::HookDefer
    );
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
    let hook_event = matches!(
        change.event_type,
        novel_bible::ChapterStateEventType::HookSeed
            | novel_bible::ChapterStateEventType::HookAdvance
            | novel_bible::ChapterStateEventType::HookPayOff
            | novel_bible::ChapterStateEventType::HookDefer
    );
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
    change: &novel_bible::ChapterStateChange,
) -> Result<novel_bible::StateChangeAllowance, String> {
    use novel_bible::{ChapterStateEventType as Event, StateChangeAllowance as Allowance};

    if change.event_type == Event::Incidental {
        let forbidden = change.changes_identity
            || change.changes_core_ability
            || change.changes_bottom_line
            || change.changes_world_hard_rule
            || change.pays_future_hook_early
            || change.opens_new_mainline;
        if forbidden || change.authority_path != "bounded_incidental" {
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
    let hook_event = matches!(
        change.event_type,
        Event::HookSeed | Event::HookAdvance | Event::HookPayOff | Event::HookDefer
    );
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

fn authority_hook_entity_id(
    authority: &governance::SealedChapterAuthority,
    event: novel_bible::ChapterStateEventType,
    path: &str,
    authority_value: &str,
) -> Option<String> {
    use novel_bible::ChapterStateEventType as Event;

    if !matches!(
        event,
        Event::HookSeed | Event::HookAdvance | Event::HookPayOff | Event::HookDefer
    ) {
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
    fn unsupported_optional_typed_delta_is_discarded_without_blocking_settlement() {
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
        assert!(validation.passed, "{:?}", validation.warnings);
        assert!(settlement.state_changes.is_empty());
        assert!(validation
            .advisories
            .iter()
            .any(|item| item.contains("ignored untrusted typed delta")));
    }

    #[test]
    fn high_risk_typed_delta_still_blocks_without_polluting_pending_state() {
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
        let body = "沈砚忽然改名为林默。";
        let raw = json!({
            "current_state": "沈砚忽然改名。",
            "chapter_summary": "人物身份发生变化。",
            "state_changes": [{
                "entity_id": "character-0001",
                "event_type": "character",
                "value": body,
                "evidence": {"excerpt": body},
                "authority_path": "chapter_contract.character_change",
                "authority_excerpt": "改名",
                "changes_identity": true
            }]
        })
        .to_string();

        let (settlement, validation, _, parse_error) =
            validated_settlement_from_final_body(&raw, body, &chapter, &authority);

        assert!(parse_error.is_none());
        assert!(!validation.passed);
        assert!(settlement.state_changes.is_empty());
        assert!(validation
            .warnings
            .iter()
            .any(|item| item.contains("high-risk transition")));
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
