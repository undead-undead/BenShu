use std::collections::{BTreeMap, BTreeSet};

use super::core::*;

pub(super) fn normalize(contract: &mut NovelContractV2) {
    if contract.schema_version.trim().is_empty() {
        contract.schema_version = NOVEL_CONTRACT_V2_SCHEMA_VERSION.to_string();
    } else {
        normalize_string(&mut contract.schema_version);
    }
    normalize_field_requirements(&mut contract.field_requirements);
    normalize_resource_economy(&mut contract.resource_economy);
    normalize_emotional_contract(&mut contract.emotional_contract);
    for entry in &mut contract.emotional_state_ledger {
        normalize_emotional_state_entry(entry);
    }
    normalize_relationship_ledger(&mut contract.relationship_ledger);
    normalize_power_progression(&mut contract.power_progression);
    normalize_social_order(&mut contract.social_order);
    normalize_geography_model(&mut contract.geography_model);
    normalize_time_model(&mut contract.time_model);
    normalize_artifact_ledger(&mut contract.artifact_ledger);
    normalize_antagonist_pressure(&mut contract.antagonist_pressure);
    normalize_payoff_matrix(&mut contract.payoff_matrix);
    normalize_narration_contract(&mut contract.narration_contract);
    normalize_scene_type_mix(&mut contract.scene_type_mix);
    for voice in &mut contract.character_voice_ledger {
        normalize_character_voice_profile(voice);
    }
    contract.character_voice_ledger.retain(has_voice_content);
    normalize_reader_promise(&mut contract.reader_promise);
    normalize_chapter_ending_rotation(&mut contract.chapter_ending_rotation);
    normalize_conflict_pressure_curve(&mut contract.conflict_pressure_curve);
    for motif in &mut contract.motif_ledger {
        normalize_motif_entry(motif);
    }
    contract.motif_ledger.retain(has_motif_content);
    for reveal in &mut contract.reveal_schedule {
        normalize_reveal_schedule_entry(reveal);
    }
    contract.reveal_schedule.retain(has_reveal_content);
    for quota in &mut contract.relationship_interaction_quotas {
        normalize_relationship_interaction_quota(quota);
    }
    contract
        .relationship_interaction_quotas
        .retain(has_relationship_quota_content);
}

fn normalize_field_requirements(requirements: &mut BTreeMap<String, String>) {
    *requirements = std::mem::take(requirements)
        .into_iter()
        .filter_map(|(key, value)| {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            (!key.is_empty() && !value.is_empty()).then_some((key, value))
        })
        .collect();
}

fn normalize_resource_economy(value: &mut ResourceEconomy) {
    normalize_strings([
        &mut value.currency,
        &mut value.value_scale,
        &mut value.class_impact,
    ]);
    for values in [
        &mut value.resource_types,
        &mut value.income_sources,
        &mut value.cost_examples,
        &mut value.scarcity_rules,
        &mut value.trade_rules,
    ] {
        normalize_string_list(values);
    }
}

fn normalize_emotional_contract(value: &mut EmotionalContract) {
    normalize_strings([
        &mut value.primary_emotion,
        &mut value.emotional_promise,
        &mut value.ending_emotional_state,
    ]);
    for values in [
        &mut value.emotional_beats,
        &mut value.relief_beats,
        &mut value.payoff_requirements,
    ] {
        normalize_string_list(values);
    }
}

fn normalize_emotional_state_entry(value: &mut EmotionalStateLedgerEntry) {
    normalize_strings([
        &mut value.character,
        &mut value.current_emotion,
        &mut value.pressure,
        &mut value.desire,
        &mut value.fear,
        &mut value.expected_next_shift,
        &mut value.payoff_target,
    ]);
    for transition in &mut value.transition_history {
        normalize_strings([
            &mut transition.from_emotion,
            &mut transition.to_emotion,
            &mut transition.trigger_event,
            &mut transition.relationship_effect,
            &mut transition.evidence,
        ]);
    }
}

fn normalize_relationship_ledger(values: &mut [RelationshipLedgerEntry]) {
    for value in values {
        for list in [
            &mut value.character_ids,
            &mut value.characters,
            &mut value.conflicts,
            &mut value.secrets,
            &mut value.turning_points,
        ] {
            normalize_string_list(list);
        }
        normalize_strings([
            &mut value.arc_type,
            &mut value.relationship_type,
            &mut value.stage,
            &mut value.next_expected_stage,
            &mut value.start_state,
            &mut value.current_state,
            &mut value.desired_end_state,
            &mut value.evidence,
        ]);
        for transition in &mut value.transition_history {
            normalize_strings([
                &mut transition.from_state,
                &mut transition.to_state,
                &mut transition.from_stage,
                &mut transition.to_stage,
                &mut transition.event,
                &mut transition.evidence,
                &mut transition.relationship_delta,
            ]);
        }
    }
}

fn normalize_power_progression(value: &mut PowerProgression) {
    normalize_string(&mut value.system_name);
    for list in [
        &mut value.levels,
        &mut value.advancement_costs,
        &mut value.bottlenecks,
        &mut value.failure_consequences,
        &mut value.anti_power_creep_rules,
    ] {
        normalize_string_list(list);
    }
    for state in &mut value.character_current_levels {
        normalize_strings([&mut state.character, &mut state.level, &mut state.evidence]);
    }
}

fn normalize_social_order(value: &mut SocialOrder) {
    normalize_strings([&mut value.rank_system, &mut value.class_structure]);
    for list in [
        &mut value.institutions,
        &mut value.exam_or_promotion_rules,
        &mut value.laws,
        &mut value.authority_conflicts,
    ] {
        normalize_string_list(list);
    }
}

fn normalize_geography_model(value: &mut GeographyModel) {
    for list in [
        &mut value.regions,
        &mut value.distance_rules,
        &mut value.travel_constraints,
        &mut value.location_changes,
    ] {
        normalize_string_list(list);
    }
    for location in &mut value.important_locations {
        normalize_strings([&mut location.name, &mut location.role]);
        normalize_string_list(&mut location.known_facts);
    }
}

fn normalize_time_model(value: &mut TimeModel) {
    normalize_strings([
        &mut value.calendar,
        &mut value.story_start_time,
        &mut value.elapsed_time,
    ]);
    normalize_string_list(&mut value.deadline_events);
    normalize_string_list(&mut value.time_skip_rules);
    for age in &mut value.age_progression {
        normalize_strings([&mut age.character, &mut age.start_age, &mut age.current_age]);
    }
}

fn normalize_artifact_ledger(values: &mut [ArtifactLedgerEntry]) {
    for value in values {
        normalize_strings([
            &mut value.name,
            &mut value.owner,
            &mut value.origin,
            &mut value.ability,
            &mut value.cost_or_limit,
            &mut value.status,
        ]);
    }
}

fn normalize_antagonist_pressure(value: &mut AntagonistPressure) {
    normalize_string(&mut value.primary_pressure);
    for antagonist in &mut value.antagonists {
        normalize_strings([
            &mut antagonist.name,
            &mut antagonist.goal,
            &mut antagonist.knowledge_state,
            &mut antagonist.current_move,
            &mut antagonist.defeat_condition,
        ]);
        normalize_string_list(&mut antagonist.resources);
        normalize_string_list(&mut antagonist.escalation_plan);
    }
}

fn normalize_payoff_matrix(values: &mut [PayoffMatrixEntry]) {
    for value in values {
        normalize_strings([
            &mut value.promise,
            &mut value.payoff_target,
            &mut value.status,
        ]);
        normalize_string_list(&mut value.evidence);
    }
}

fn normalize_narration_contract(value: &mut NarrationContract) {
    normalize_strings([
        &mut value.pov,
        &mut value.tense,
        &mut value.narrative_distance,
        &mut value.dialogue_style,
        &mut value.description_density,
        &mut value.chapter_pacing,
    ]);
    normalize_string_list(&mut value.forbidden_style_drift);
}

fn normalize_scene_type_mix(value: &mut SceneTypeMix) {
    normalize_strings([
        &mut value.action,
        &mut value.dialogue,
        &mut value.everyday,
        &mut value.reveal,
        &mut value.emotional,
        &mut value.turning_point,
        &mut value.balance_rule,
    ]);
}

fn normalize_character_voice_profile(value: &mut CharacterVoiceProfile) {
    normalize_strings([&mut value.character, &mut value.voice_style]);
    normalize_string_list(&mut value.catchphrases);
    normalize_string_list(&mut value.forbidden_expressions);
    normalize_string_list(&mut value.dialogue_rules);
}

fn has_voice_content(value: &CharacterVoiceProfile) -> bool {
    !value.character.is_empty()
        || !value.voice_style.is_empty()
        || !value.catchphrases.is_empty()
        || !value.forbidden_expressions.is_empty()
        || !value.dialogue_rules.is_empty()
}

fn normalize_reader_promise(value: &mut ReaderPromise) {
    normalize_strings([
        &mut value.core_hook,
        &mut value.curiosity_engine,
        &mut value.payoff_style,
    ]);
    normalize_string_list(&mut value.pleasure_points);
}

fn normalize_chapter_ending_rotation(value: &mut ChapterEndingRotation) {
    normalize_string_list(&mut value.planned_rotation);
    normalize_string(&mut value.avoid_repetition_rule);
}

fn normalize_conflict_pressure_curve(value: &mut ConflictPressureCurve) {
    normalize_strings([&mut value.release_strategy, &mut value.peak_policy]);
    for beat in &mut value.global_curve {
        normalize_strings([
            &mut beat.range,
            &mut beat.pressure_level,
            &mut beat.function,
        ]);
    }
    value.global_curve.retain(|beat| {
        !beat.range.is_empty() || !beat.pressure_level.is_empty() || !beat.function.is_empty()
    });
}

fn normalize_motif_entry(value: &mut MotifLedgerEntry) {
    normalize_strings([
        &mut value.motif,
        &mut value.meaning,
        &mut value.payoff_target,
    ]);
    normalize_string_list(&mut value.evolution);
}

fn has_motif_content(value: &MotifLedgerEntry) -> bool {
    !value.motif.is_empty()
        || !value.meaning.is_empty()
        || !value.evolution.is_empty()
        || !value.payoff_target.is_empty()
}

fn normalize_reveal_schedule_entry(value: &mut RevealScheduleEntry) {
    normalize_strings([
        &mut value.secret,
        &mut value.reader_knows,
        &mut value.protagonist_knows,
        &mut value.antagonist_knows,
        &mut value.reveal_window,
        &mut value.status,
    ]);
}

fn has_reveal_content(value: &RevealScheduleEntry) -> bool {
    !value.secret.is_empty()
        || !value.reader_knows.is_empty()
        || !value.protagonist_knows.is_empty()
        || !value.antagonist_knows.is_empty()
        || !value.reveal_window.is_empty()
}

fn normalize_relationship_interaction_quota(value: &mut RelationshipInteractionQuota) {
    normalize_strings([
        &mut value.relationship,
        &mut value.cadence,
        &mut value.next_due,
        &mut value.required_interaction,
    ]);
    normalize_string_list(&mut value.characters);
}

fn has_relationship_quota_content(value: &RelationshipInteractionQuota) -> bool {
    !value.relationship.is_empty()
        || !value.characters.is_empty()
        || !value.cadence.is_empty()
        || !value.next_due.is_empty()
        || !value.required_interaction.is_empty()
}

fn normalize_strings<const N: usize>(values: [&mut String; N]) {
    for value in values {
        normalize_string(value);
    }
}

fn normalize_string(value: &mut String) {
    let trimmed = value.trim();
    if trimmed.len() != value.len() {
        *value = trimmed.to_string();
    }
}

fn normalize_string_list(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain_mut(|value| {
        normalize_string(value);
        !value.is_empty() && seen.insert(value.clone())
    });
}
