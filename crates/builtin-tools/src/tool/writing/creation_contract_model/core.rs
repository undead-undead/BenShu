//! Strongly typed creation contracts for governed writing projects.
//!
//! JSON is only the boundary format used by the model/tool surface. The runtime
//! contract is this Rust model, which validates and normalizes before syncing
//! into the existing session draft state.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::super::creation_contract::issue::ContractIssueList;
use super::super::creation_contract_normalizer;
use super::super::longform_policy;
use super::super::novel_contract_v2::NovelContractV2;
#[cfg(test)]
use super::super::novel_contract_v2::RelationshipLedgerEntry;
use super::super::novel_runner;
use super::super::surface_sanitizer;
use super::super::typed_contract_gate;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct NovelCreationContract {
    #[serde(default)]
    pub(crate) title: TitleContract,
    #[serde(default)]
    pub(crate) language: String,
    #[serde(default)]
    pub(crate) genre: String,
    #[serde(default)]
    pub(crate) brief: String,
    #[serde(default)]
    pub(crate) target_units: Option<usize>,
    #[serde(default)]
    pub(crate) chapter_unit_target: Option<usize>,
    #[serde(default)]
    pub(crate) max_chapters_per_turn: Option<usize>,
    #[serde(default)]
    pub(crate) premise: String,
    #[serde(default)]
    pub(crate) ending: EndingContract,
    #[serde(default)]
    pub(crate) protagonist_arc: String,
    #[serde(default)]
    pub(crate) world_imagery: String,
    #[serde(default, alias = "main_ca_spine", alias = "main_spine")]
    pub(crate) main_causal_spine: String,
    #[serde(default)]
    pub(crate) characters: Vec<CharacterContract>,
    #[serde(default)]
    pub(crate) themes: Vec<String>,
    #[serde(default)]
    pub(crate) world_rules: Vec<String>,
    #[serde(default)]
    pub(crate) style_rules: Vec<String>,
    #[serde(default)]
    pub(crate) must_avoid: Vec<String>,
    #[serde(default)]
    pub(crate) outline: OutlineContract,
    #[serde(default)]
    pub(crate) structured: NovelContractV2,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct TitleContract {
    #[serde(default, alias = "title")]
    pub(crate) canonical_title: String,
    #[serde(default)]
    pub(crate) candidates: Vec<String>,
    #[serde(default, alias = "title_rationale")]
    pub(crate) rationale: String,
    #[serde(default)]
    pub(crate) source: TitleSource,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TitleSource {
    User,
    LlmContract,
    Repaired,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct CharacterContract {
    #[serde(default)]
    pub(crate) character_id: String,
    #[serde(default, alias = "name")]
    pub(crate) canonical_name: String,
    #[serde(default)]
    pub(crate) name_source: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    #[serde(default)]
    pub(crate) previous_names: Vec<String>,
    #[serde(default)]
    pub(crate) role: String,
    #[serde(default)]
    pub(crate) desire: String,
    #[serde(default)]
    pub(crate) fear: String,
    #[serde(default)]
    pub(crate) bottom_line: String,
    #[serde(default)]
    pub(crate) arc_start: String,
    #[serde(default)]
    pub(crate) arc_end: String,
    #[serde(default)]
    pub(crate) planned_entry: String,
    #[serde(default)]
    pub(crate) planned_exit: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct EndingContract {
    #[serde(default, alias = "ending_direction")]
    pub(crate) desired_resolution: String,
    #[serde(default)]
    pub(crate) final_state: String,
    #[serde(default)]
    pub(crate) must_resolve: Vec<String>,
    #[serde(default)]
    pub(crate) allowed_open_questions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OutlineContract {
    #[serde(default)]
    pub(crate) volumes: Vec<VolumeContract>,
    #[serde(default)]
    pub(crate) near_chapters: Vec<ChapterSeedContract>,
    #[serde(default)]
    pub(crate) raw_outline: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct VolumeContract {
    #[serde(default)]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) objective: String,
    #[serde(default)]
    pub(crate) ending_change: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ChapterSeedContract {
    #[serde(default)]
    pub(crate) number: Option<usize>,
    #[serde(default)]
    pub(crate) goal: String,
    #[serde(default)]
    pub(crate) expected_turn: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ContractBlockerReport {
    pub(crate) issues: ContractIssueList,
}

impl ContractBlockerReport {
    pub(crate) fn is_ready(&self) -> bool {
        self.issues.is_empty()
    }
}

impl NovelCreationContract {
    pub(crate) fn parse_json_boundary(raw: &str) -> Option<Self> {
        let normalized = creation_contract_normalizer::normalize_creation_contract_boundary(raw)?;
        let value = normalized.value;
        let flat_structured = flat_structured_contract_from_boundary_value(&value);
        serde_json::from_value::<Self>(value.clone())
            .ok()
            .or_else(|| Self::from_flat_json(value))
            .map(|mut contract| {
                if novel_contract_v2_content_score(&flat_structured)
                    > novel_contract_v2_content_score(&contract.structured)
                {
                    contract.structured = flat_structured;
                }
                contract.normalize();
                contract
            })
    }

    fn from_flat_json(value: Value) -> Option<Self> {
        let object = value.as_object()?;
        let mut contract = Self {
            title: TitleContract {
                canonical_title: nested_string_field(object, "title", "canonical_title")
                    .or_else(|| nested_string_field(object, "title", "title"))
                    .unwrap_or_else(|| string_field(object, "title")),
                rationale: nested_string_field(object, "title", "rationale")
                    .or_else(|| nested_string_field(object, "title", "title_rationale"))
                    .unwrap_or_else(|| string_field(object, "title_rationale")),
                candidates: nested_string_array_field(object, "title", "candidates")
                    .unwrap_or_else(|| string_array_field(object, "title_candidates")),
                source: TitleSource::LlmContract,
            },
            language: string_field(object, "language"),
            genre: string_field(object, "genre"),
            brief: string_field(object, "brief"),
            target_units: usize_field(object, "target_units"),
            chapter_unit_target: usize_field(object, "chapter_unit_target"),
            max_chapters_per_turn: usize_field(object, "max_chapters_per_turn"),
            premise: string_field(object, "premise"),
            ending: EndingContract {
                desired_resolution: nested_string_field(object, "ending", "desired_resolution")
                    .or_else(|| nested_string_field(object, "ending", "ending_direction"))
                    .unwrap_or_else(|| string_field(object, "ending_direction")),
                final_state: nested_string_field(object, "ending", "final_state")
                    .unwrap_or_else(|| string_field(object, "final_state")),
                must_resolve: nested_string_array_field(object, "ending", "must_resolve")
                    .unwrap_or_else(|| string_array_field(object, "must_resolve")),
                allowed_open_questions: nested_string_array_field(
                    object,
                    "ending",
                    "allowed_open_questions",
                )
                .unwrap_or_else(|| string_array_field(object, "allowed_open_questions")),
            },
            protagonist_arc: string_field(object, "protagonist_arc"),
            world_imagery: string_field(object, "world_imagery"),
            main_causal_spine: string_field_aliases(
                object,
                &["main_causal_spine", "main_ca_spine", "main_spine"],
            ),
            characters: character_array_field(object, "characters"),
            themes: string_array_field(object, "themes"),
            world_rules: string_array_field(object, "world_rules"),
            style_rules: string_array_field(object, "style_rules"),
            must_avoid: string_array_field(object, "must_avoid"),
            outline: outline_field(object, "outline"),
            structured: serde_json::from_value(Value::Object(object.clone())).unwrap_or_default(),
        };
        contract.normalize();
        has_meaningful_contract_content(&contract).then_some(contract)
    }

    pub(crate) fn normalize(&mut self) {
        normalize_string(&mut self.title.canonical_title);
        normalize_string(&mut self.title.rationale);
        normalize_string(&mut self.language);
        self.language = normalize_contract_language(&self.language);
        normalize_string(&mut self.genre);
        normalize_string(&mut self.brief);
        normalize_string(&mut self.premise);
        normalize_string(&mut self.ending.desired_resolution);
        normalize_string(&mut self.ending.final_state);
        normalize_string(&mut self.protagonist_arc);
        normalize_string(&mut self.world_imagery);
        normalize_string(&mut self.main_causal_spine);
        normalize_string_vec(&mut self.title.candidates);
        normalize_string_vec(&mut self.ending.must_resolve);
        normalize_string_vec(&mut self.ending.allowed_open_questions);
        normalize_string_vec(&mut self.themes);
        normalize_world_rules_vec(&mut self.world_rules);
        normalize_string_vec(&mut self.style_rules);
        normalize_string_vec(&mut self.must_avoid);
        for character in &mut self.characters {
            normalize_string(&mut character.character_id);
            normalize_string(&mut character.canonical_name);
            normalize_string(&mut character.name_source);
            normalize_string(&mut character.role);
            normalize_string(&mut character.desire);
            normalize_string(&mut character.fear);
            normalize_string(&mut character.bottom_line);
            normalize_string(&mut character.arc_start);
            normalize_string(&mut character.arc_end);
            normalize_string(&mut character.planned_entry);
            normalize_string(&mut character.planned_exit);
            normalize_string_vec(&mut character.aliases);
            normalize_string_vec(&mut character.previous_names);
            let canonical_name = character.canonical_name.trim();
            character
                .aliases
                .retain(|name| name.trim() != canonical_name);
            character
                .previous_names
                .retain(|name| name.trim() != canonical_name);
        }
        for volume in &mut self.outline.volumes {
            normalize_string(&mut volume.title);
            normalize_string(&mut volume.objective);
            normalize_string(&mut volume.ending_change);
        }
        for chapter in &mut self.outline.near_chapters {
            normalize_string(&mut chapter.goal);
            normalize_string(&mut chapter.expected_turn);
        }
        normalize_string(&mut self.outline.raw_outline);
        self.chapter_unit_target =
            longform_policy::normalize_user_chapter_unit_target(self.chapter_unit_target);
        self.max_chapters_per_turn = normalize_model_turn_chapter_count(
            self.max_chapters_per_turn,
            self.target_units,
            self.chapter_unit_target,
        );
        self.clear_top_level_surface_pollution();
        self.clear_structured_surface_pollution();
        self.structured.normalize();
        self.resolve_relationship_character_ids();
        self.align_primary_name_authority_surfaces();
    }

    fn resolve_relationship_character_ids(&mut self) {
        let ids_by_name = self
            .characters
            .iter()
            .filter(|character| {
                !value_missing(&character.canonical_name) && !value_missing(&character.character_id)
            })
            .map(|character| {
                (
                    character.canonical_name.trim().to_string(),
                    character.character_id.trim().to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if ids_by_name.is_empty() {
            return;
        }
        for relation in &mut self.structured.relationship_ledger {
            let resolved = relation
                .characters
                .iter()
                .filter_map(|name| ids_by_name.get(name.trim()).cloned())
                .collect::<Vec<_>>();
            if resolved.len() == relation.characters.len() {
                relation.character_ids = resolved;
            }
        }
    }

    pub(crate) fn align_primary_name_authority_surfaces(&mut self) -> bool {
        let Some(primary_name) = self
            .characters
            .iter()
            .find(|character| character.role_looks_primary())
            .map(|character| character.canonical_name.trim().to_string())
            .filter(|name| !value_missing(name))
        else {
            return false;
        };
        let authority_names = self.character_authority_names();
        let mut changed = false;
        for value in [
            &mut self.brief,
            &mut self.premise,
            &mut self.protagonist_arc,
            &mut self.main_causal_spine,
            &mut self.outline.raw_outline,
        ] {
            changed |=
                align_primary_name_authority_text(value, &primary_name, &authority_names, false);
        }
        let repaired_title_rationale =
            repair_authority_name_tail_noise_in_text(&self.title.rationale, &authority_names);
        if repaired_title_rationale != self.title.rationale {
            self.title.rationale = repaired_title_rationale;
            changed = true;
        }
        for value in [
            &mut self.ending.desired_resolution,
            &mut self.ending.final_state,
            &mut self.world_imagery,
        ] {
            changed |=
                align_primary_name_authority_text(value, &primary_name, &authority_names, false);
        }
        for value in self
            .ending
            .must_resolve
            .iter_mut()
            .chain(self.ending.allowed_open_questions.iter_mut())
            .chain(self.themes.iter_mut())
            .chain(self.world_rules.iter_mut())
            .chain(self.style_rules.iter_mut())
            .chain(self.must_avoid.iter_mut())
        {
            changed |=
                align_primary_name_authority_text(value, &primary_name, &authority_names, false);
        }
        for character in &mut self.characters {
            for value in [
                &mut character.desire,
                &mut character.fear,
                &mut character.bottom_line,
                &mut character.arc_start,
                &mut character.arc_end,
            ] {
                changed |= align_primary_name_authority_text(
                    value,
                    &primary_name,
                    &authority_names,
                    false,
                );
            }
        }
        for volume in &mut self.outline.volumes {
            changed |= align_primary_name_authority_text(
                &mut volume.objective,
                &primary_name,
                &authority_names,
                false,
            );
            changed |= align_primary_name_authority_text(
                &mut volume.ending_change,
                &primary_name,
                &authority_names,
                false,
            );
        }
        for chapter in &mut self.outline.near_chapters {
            changed |= align_primary_name_authority_text(
                &mut chapter.goal,
                &primary_name,
                &authority_names,
                false,
            );
            changed |= align_primary_name_authority_text(
                &mut chapter.expected_turn,
                &primary_name,
                &authority_names,
                false,
            );
        }
        changed |= self.align_structured_primary_name_authority(&primary_name, &authority_names);
        changed
    }

    fn align_structured_primary_name_authority(
        &mut self,
        primary_name: &str,
        authority_names: &BTreeSet<String>,
    ) -> bool {
        let mut changed = false;

        let emotion = &mut self.structured.emotional_contract;
        for value in [
            &mut emotion.primary_emotion,
            &mut emotion.emotional_promise,
            &mut emotion.ending_emotional_state,
        ] {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }
        for value in emotion
            .emotional_beats
            .iter_mut()
            .chain(emotion.relief_beats.iter_mut())
            .chain(emotion.payoff_requirements.iter_mut())
        {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }

        for entry in &mut self.structured.emotional_state_ledger {
            changed |= align_primary_name_authority_text(
                &mut entry.character,
                primary_name,
                authority_names,
                false,
            );
            for value in [
                &mut entry.current_emotion,
                &mut entry.pressure,
                &mut entry.desire,
                &mut entry.fear,
                &mut entry.expected_next_shift,
                &mut entry.payoff_target,
            ] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
            for transition in &mut entry.transition_history {
                for value in [
                    &mut transition.from_emotion,
                    &mut transition.to_emotion,
                    &mut transition.trigger_event,
                    &mut transition.relationship_effect,
                    &mut transition.evidence,
                ] {
                    changed |= align_primary_name_authority_text(
                        value,
                        primary_name,
                        authority_names,
                        false,
                    );
                }
            }
        }

        for relation in &mut self.structured.relationship_ledger {
            for value in &mut relation.characters {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
            for value in [
                &mut relation.arc_type,
                &mut relation.relationship_type,
                &mut relation.stage,
                &mut relation.next_expected_stage,
                &mut relation.start_state,
                &mut relation.current_state,
                &mut relation.desired_end_state,
                &mut relation.evidence,
            ] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
            for value in relation
                .conflicts
                .iter_mut()
                .chain(relation.secrets.iter_mut())
                .chain(relation.turning_points.iter_mut())
            {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        for voice in &mut self.structured.character_voice_ledger {
            changed |= align_primary_name_authority_text(
                &mut voice.character,
                primary_name,
                authority_names,
                false,
            );
            changed |= align_primary_name_authority_text(
                &mut voice.voice_style,
                primary_name,
                authority_names,
                true,
            );
            for value in voice
                .catchphrases
                .iter_mut()
                .chain(voice.forbidden_expressions.iter_mut())
                .chain(voice.dialogue_rules.iter_mut())
            {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, true);
            }
        }

        let reader = &mut self.structured.reader_promise;
        for value in [
            &mut reader.core_hook,
            &mut reader.curiosity_engine,
            &mut reader.payoff_style,
        ] {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }
        for value in &mut reader.pleasure_points {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }

        let scene = &mut self.structured.scene_type_mix;
        for value in [
            &mut scene.action,
            &mut scene.dialogue,
            &mut scene.everyday,
            &mut scene.reveal,
            &mut scene.emotional,
            &mut scene.turning_point,
            &mut scene.balance_rule,
        ] {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }

        for beat in &mut self.structured.conflict_pressure_curve.global_curve {
            for value in [
                &mut beat.range,
                &mut beat.pressure_level,
                &mut beat.function,
            ] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }
        changed |= align_primary_name_authority_text(
            &mut self.structured.conflict_pressure_curve.release_strategy,
            primary_name,
            authority_names,
            false,
        );
        changed |= align_primary_name_authority_text(
            &mut self.structured.conflict_pressure_curve.peak_policy,
            primary_name,
            authority_names,
            false,
        );

        for motif in &mut self.structured.motif_ledger {
            changed |= align_primary_name_authority_text(
                &mut motif.motif,
                primary_name,
                authority_names,
                false,
            );
            changed |= align_primary_name_authority_text(
                &mut motif.meaning,
                primary_name,
                authority_names,
                false,
            );
            changed |= align_primary_name_authority_text(
                &mut motif.payoff_target,
                primary_name,
                authority_names,
                false,
            );
            for value in &mut motif.evolution {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        for reveal in &mut self.structured.reveal_schedule {
            for value in [
                &mut reveal.secret,
                &mut reveal.reader_knows,
                &mut reveal.protagonist_knows,
                &mut reveal.antagonist_knows,
                &mut reveal.reveal_window,
                &mut reveal.status,
            ] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        for quota in &mut self.structured.relationship_interaction_quotas {
            for value in &mut quota.characters {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
            for value in [
                &mut quota.relationship,
                &mut quota.cadence,
                &mut quota.required_interaction,
                &mut quota.next_due,
            ] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        let resource = &mut self.structured.resource_economy;
        for value in [
            &mut resource.currency,
            &mut resource.value_scale,
            &mut resource.class_impact,
        ] {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }
        for value in resource
            .resource_types
            .iter_mut()
            .chain(resource.income_sources.iter_mut())
            .chain(resource.cost_examples.iter_mut())
            .chain(resource.scarcity_rules.iter_mut())
            .chain(resource.trade_rules.iter_mut())
        {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }

        let power = &mut self.structured.power_progression;
        changed |= align_primary_name_authority_text(
            &mut power.system_name,
            primary_name,
            authority_names,
            false,
        );
        for value in power
            .levels
            .iter_mut()
            .chain(power.advancement_costs.iter_mut())
            .chain(power.bottlenecks.iter_mut())
            .chain(power.failure_consequences.iter_mut())
            .chain(power.anti_power_creep_rules.iter_mut())
        {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }
        for state in &mut power.character_current_levels {
            for value in [&mut state.character, &mut state.level, &mut state.evidence] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        let order = &mut self.structured.social_order;
        for value in [&mut order.rank_system, &mut order.class_structure] {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }
        for value in order
            .institutions
            .iter_mut()
            .chain(order.exam_or_promotion_rules.iter_mut())
            .chain(order.laws.iter_mut())
            .chain(order.authority_conflicts.iter_mut())
        {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }

        for value in self
            .structured
            .geography_model
            .regions
            .iter_mut()
            .chain(self.structured.geography_model.distance_rules.iter_mut())
            .chain(
                self.structured
                    .geography_model
                    .travel_constraints
                    .iter_mut(),
            )
            .chain(self.structured.geography_model.location_changes.iter_mut())
        {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }
        for location in &mut self.structured.geography_model.important_locations {
            for value in [&mut location.name, &mut location.role] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
            for value in &mut location.known_facts {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        let time = &mut self.structured.time_model;
        for value in [
            &mut time.calendar,
            &mut time.story_start_time,
            &mut time.elapsed_time,
        ] {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }
        for value in time
            .deadline_events
            .iter_mut()
            .chain(time.time_skip_rules.iter_mut())
        {
            changed |=
                align_primary_name_authority_text(value, primary_name, authority_names, false);
        }
        for age in &mut time.age_progression {
            for value in [&mut age.character, &mut age.start_age, &mut age.current_age] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        for artifact in &mut self.structured.artifact_ledger {
            for value in [
                &mut artifact.name,
                &mut artifact.owner,
                &mut artifact.origin,
                &mut artifact.ability,
                &mut artifact.cost_or_limit,
                &mut artifact.status,
            ] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        changed |= align_primary_name_authority_text(
            &mut self.structured.antagonist_pressure.primary_pressure,
            primary_name,
            authority_names,
            false,
        );
        for antagonist in &mut self.structured.antagonist_pressure.antagonists {
            for value in [
                &mut antagonist.name,
                &mut antagonist.goal,
                &mut antagonist.knowledge_state,
                &mut antagonist.current_move,
                &mut antagonist.defeat_condition,
            ] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
            for value in antagonist
                .resources
                .iter_mut()
                .chain(antagonist.escalation_plan.iter_mut())
            {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        for entry in &mut self.structured.payoff_matrix {
            for value in [
                &mut entry.promise,
                &mut entry.payoff_target,
                &mut entry.status,
            ] {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
            for value in &mut entry.evidence {
                changed |=
                    align_primary_name_authority_text(value, primary_name, authority_names, false);
            }
        }

        changed
    }

    fn character_authority_names(&self) -> BTreeSet<String> {
        self.characters
            .iter()
            .flat_map(|character| {
                std::iter::once(character.canonical_name.trim().to_string()).chain(
                    character
                        .aliases
                        .iter()
                        .map(|alias| alias.trim().to_string()),
                )
            })
            .filter(|name| !value_missing(name))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn validate(&self) -> ContractBlockerReport {
        self.validate_for_scope(
            super::super::creation_contract::ContractReadinessScope::LockedAuthorityContract,
        )
    }

    pub(crate) fn validate_for_scope(
        &self,
        scope: super::super::creation_contract::ContractReadinessScope,
    ) -> ContractBlockerReport {
        super::super::typed_contract_gate::validate_novel_creation_contract_for_scope(self, scope)
    }
    pub(crate) fn collect_surface_blockers(&self, issues: &mut ContractIssueList) {
        let cjk = self.contract_language_looks_cjk();
        issues.set_scope(
            "contract.title.surface",
            super::super::creation_contract::issue::ContractIssueKind::Skeleton,
            "title",
        );
        push_contract_surface_issue(issues, cjk, "书名", &self.title.canonical_title);
        push_contract_surface_issue(issues, cjk, "书名理由", &self.title.rationale);
        for (index, candidate) in self.title.candidates.iter().enumerate() {
            push_contract_surface_issue(issues, cjk, &format!("候选书名{}", index + 1), candidate);
        }
        push_contract_surface_issue(issues, cjk, "题材", &self.genre);
        push_contract_surface_issue(issues, cjk, "创作简述", &self.brief);
        issues.set_scope(
            "contract.skeleton.surface",
            super::super::creation_contract::issue::ContractIssueKind::Skeleton,
            "story_authority",
        );
        push_contract_surface_issue(issues, cjk, "故事前提", &self.premise);
        push_contract_surface_issue(issues, cjk, "终局方向", &self.ending.desired_resolution);
        push_contract_surface_issue(issues, cjk, "终局状态", &self.ending.final_state);
        for (index, value) in self.ending.must_resolve.iter().enumerate() {
            push_contract_surface_issue(issues, cjk, &format!("必须兑现{}", index + 1), value);
        }
        for (index, value) in self.ending.allowed_open_questions.iter().enumerate() {
            push_contract_surface_issue(issues, cjk, &format!("允许开放问题{}", index + 1), value);
        }
        push_contract_surface_issue(issues, cjk, "主角弧线", &self.protagonist_arc);
        push_contract_surface_issue(issues, cjk, "世界观意象", &self.world_imagery);
        push_contract_surface_issue(issues, cjk, "总主线因果链", &self.main_causal_spine);
        issues.set_scope(
            "contract.character_authority.surface",
            super::super::creation_contract::issue::ContractIssueKind::Characters,
            "characters",
        );
        for (index, character) in self.characters.iter().enumerate() {
            let prefix = format!("角色{}", index + 1);
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}姓名"),
                &character.canonical_name,
            );
            push_contract_surface_issue(issues, cjk, &format!("{prefix}身份"), &character.role);
            push_contract_surface_issue(issues, cjk, &format!("{prefix}欲望"), &character.desire);
            push_contract_surface_issue(issues, cjk, &format!("{prefix}恐惧"), &character.fear);
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}底线"),
                &character.bottom_line,
            );
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}弧线起点"),
                &character.arc_start,
            );
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}弧线终点"),
                &character.arc_end,
            );
        }
        issues.set_scope(
            "contract.governance.surface",
            super::super::creation_contract::issue::ContractIssueKind::Governance,
            "governance",
        );
        push_contract_string_list_surface_issues(issues, cjk, "主题", &self.themes);
        push_contract_string_list_surface_issues(issues, cjk, "世界规则", &self.world_rules);
        push_contract_string_list_surface_issues(issues, cjk, "风格规则", &self.style_rules);
        push_contract_string_list_surface_issues(issues, cjk, "必须避免", &self.must_avoid);
        self.collect_structured_surface_blockers(issues, cjk);
        issues.set_scope(
            "contract.outline.surface",
            super::super::creation_contract::issue::ContractIssueKind::Plot,
            "outline",
        );
        for (index, volume) in self.outline.volumes.iter().enumerate() {
            let prefix = format!("分卷{}", index + 1);
            push_contract_surface_issue(issues, cjk, &format!("{prefix}卷名"), &volume.title);
            push_contract_surface_issue(issues, cjk, &format!("{prefix}目标"), &volume.objective);
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}卷尾变化"),
                &volume.ending_change,
            );
        }
        for (index, chapter) in self.outline.near_chapters.iter().enumerate() {
            let prefix = format!("近期章节{}", index + 1);
            push_contract_surface_issue(issues, cjk, &format!("{prefix}目标"), &chapter.goal);
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}转折"),
                &chapter.expected_turn,
            );
            if contract_value_is_numeric_placeholder(&chapter.expected_turn) {
                issues.push(format!(
                    "ContractBlocker: {prefix}预期转折只有数字占位，必须写出本章不可逆变化"
                ));
            }
            if contract_value_is_chapter_label_placeholder(&chapter.expected_turn) {
                issues.push(format!(
                    "ContractBlocker: {prefix}预期转折只是章节标签，必须写出本章不可逆变化"
                ));
            }
        }
        push_contract_surface_issue(issues, cjk, "大纲", &self.outline.raw_outline);
        if contract_outline_looks_glued_control_blocks(&self.outline.raw_outline) {
            issues.push(
                "ContractBlocker: 小说合同大纲把分卷、章节目标和转折胶合在同一字段里，必须拆成结构化 volumes 与 near_chapters"
                    .to_string(),
            );
        }
        issues.set_scope(
            "contract.character_authority.primary_name",
            super::super::creation_contract::issue::ContractIssueKind::Characters,
            "characters",
        );
        self.collect_primary_name_authority_blockers(issues);
    }

    fn collect_structured_surface_blockers(&self, issues: &mut ContractIssueList, cjk: bool) {
        issues.set_scope(
            "contract.structured_governance.surface",
            super::super::creation_contract::issue::ContractIssueKind::Governance,
            "structured",
        );
        let structured = &self.structured;
        let resource = &structured.resource_economy;
        push_contract_surface_issue(issues, cjk, "资源货币", &resource.currency);
        push_structured_scalar_surface_issue(issues, cjk, "资源尺度", &resource.value_scale);
        push_contract_string_list_surface_issues(issues, cjk, "资源类型", &resource.resource_types);
        push_contract_string_list_surface_issues(issues, cjk, "资源来源", &resource.income_sources);
        push_contract_string_list_surface_issues(issues, cjk, "资源消耗", &resource.cost_examples);
        push_contract_string_list_surface_issues(issues, cjk, "稀缺规则", &resource.scarcity_rules);
        push_contract_string_list_surface_issues(issues, cjk, "交易规则", &resource.trade_rules);
        push_contract_surface_issue(issues, cjk, "阶层影响", &resource.class_impact);

        let emotion = &structured.emotional_contract;
        push_contract_surface_issue(issues, cjk, "主情绪", &emotion.primary_emotion);
        push_contract_surface_issue(issues, cjk, "情感承诺", &emotion.emotional_promise);
        push_contract_string_list_surface_issues(issues, cjk, "情感节拍", &emotion.emotional_beats);
        push_contract_string_list_surface_issues(
            issues,
            cjk,
            "情感兑现要求",
            &emotion.payoff_requirements,
        );
        push_contract_surface_issue(issues, cjk, "终局情绪落点", &emotion.ending_emotional_state);

        for (index, entry) in structured.emotional_state_ledger.iter().enumerate() {
            let prefix = format!("情绪账本{}", index + 1);
            push_contract_surface_issue(issues, cjk, &format!("{prefix}角色"), &entry.character);
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}当前情绪"),
                &entry.current_emotion,
            );
            push_contract_surface_issue(issues, cjk, &format!("{prefix}压力"), &entry.pressure);
            push_contract_surface_issue(issues, cjk, &format!("{prefix}欲望"), &entry.desire);
            push_contract_surface_issue(issues, cjk, &format!("{prefix}恐惧"), &entry.fear);
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}下一变化"),
                &entry.expected_next_shift,
            );
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}兑现目标"),
                &entry.payoff_target,
            );
        }

        for (index, relation) in structured.relationship_ledger.iter().enumerate() {
            let prefix = format!("关系账本{}", index + 1);
            push_contract_string_list_surface_issues(
                issues,
                cjk,
                &format!("{prefix}角色"),
                &relation.characters,
            );
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}弧线类型"),
                &relation.arc_type,
            );
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}关系类型"),
                &relation.relationship_type,
            );
            push_contract_surface_issue(issues, cjk, &format!("{prefix}阶段"), &relation.stage);
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}下一阶段"),
                &relation.next_expected_stage,
            );
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}起点"),
                &relation.start_state,
            );
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}现状"),
                &relation.current_state,
            );
            push_contract_surface_issue(
                issues,
                cjk,
                &format!("{prefix}终点"),
                &relation.desired_end_state,
            );
            push_contract_surface_issue(issues, cjk, &format!("{prefix}证据"), &relation.evidence);
            push_contract_string_list_surface_issues(
                issues,
                cjk,
                &format!("{prefix}冲突"),
                &relation.conflicts,
            );
            push_contract_string_list_surface_issues(
                issues,
                cjk,
                &format!("{prefix}秘密"),
                &relation.secrets,
            );
            push_contract_string_list_surface_issues(
                issues,
                cjk,
                &format!("{prefix}转折"),
                &relation.turning_points,
            );
        }

        let power = &structured.power_progression;
        push_structured_scalar_surface_issue(issues, cjk, "成长体系", &power.system_name);
        push_contract_string_list_surface_issues(issues, cjk, "成长等级", &power.levels);
        push_contract_string_list_surface_issues(issues, cjk, "进阶代价", &power.advancement_costs);
        push_contract_string_list_surface_issues(issues, cjk, "成长瓶颈", &power.bottlenecks);
        push_contract_string_list_surface_issues(
            issues,
            cjk,
            "失控后果",
            &power.failure_consequences,
        );
        push_contract_string_list_surface_issues(
            issues,
            cjk,
            "战力膨胀约束",
            &power.anti_power_creep_rules,
        );

        let order = &structured.social_order;
        push_contract_string_list_surface_issues(issues, cjk, "社会机构", &order.institutions);
        push_structured_scalar_surface_issue(issues, cjk, "等级秩序", &order.rank_system);
        push_contract_string_list_surface_issues(
            issues,
            cjk,
            "晋升规则",
            &order.exam_or_promotion_rules,
        );
        push_contract_string_list_surface_issues(issues, cjk, "法律规则", &order.laws);
        push_contract_surface_issue(issues, cjk, "阶层结构", &order.class_structure);
        push_contract_string_list_surface_issues(
            issues,
            cjk,
            "权力冲突",
            &order.authority_conflicts,
        );

        let narration = &structured.narration_contract;
        push_contract_surface_issue(issues, cjk, "叙事视角", &narration.pov);
        push_contract_surface_issue(issues, cjk, "叙事时态", &narration.tense);
        push_contract_surface_issue(issues, cjk, "叙事距离", &narration.narrative_distance);
        push_contract_surface_issue(issues, cjk, "对白风格", &narration.dialogue_style);
        push_contract_surface_issue(issues, cjk, "描写密度", &narration.description_density);
        push_contract_surface_issue(issues, cjk, "章节节奏", &narration.chapter_pacing);
        push_contract_string_list_surface_issues(
            issues,
            cjk,
            "禁止风格漂移",
            &narration.forbidden_style_drift,
        );
    }

    fn clear_structured_surface_pollution(&mut self) {
        let cjk = self.contract_language_looks_cjk();
        let resource = &mut self.structured.resource_economy;
        clear_polluted_contract_string(&mut resource.currency, cjk);
        clear_polluted_contract_string(&mut resource.value_scale, cjk);
        clear_polluted_contract_string_list(&mut resource.resource_types, cjk);
        clear_polluted_contract_string_list(&mut resource.income_sources, cjk);
        clear_polluted_contract_string_list(&mut resource.cost_examples, cjk);
        clear_polluted_contract_string_list(&mut resource.scarcity_rules, cjk);
        clear_polluted_contract_string_list(&mut resource.trade_rules, cjk);
        clear_polluted_contract_string(&mut resource.class_impact, cjk);

        let emotion = &mut self.structured.emotional_contract;
        clear_polluted_contract_string(&mut emotion.primary_emotion, cjk);
        clear_polluted_contract_string(&mut emotion.emotional_promise, cjk);
        clear_polluted_contract_string_list(&mut emotion.emotional_beats, cjk);
        clear_polluted_contract_string_list(&mut emotion.payoff_requirements, cjk);
        clear_polluted_contract_string(&mut emotion.ending_emotional_state, cjk);

        for entry in &mut self.structured.emotional_state_ledger {
            clear_polluted_contract_string(&mut entry.character, cjk);
            clear_polluted_contract_string(&mut entry.current_emotion, cjk);
            clear_polluted_contract_string(&mut entry.pressure, cjk);
            clear_polluted_contract_string(&mut entry.desire, cjk);
            clear_polluted_contract_string(&mut entry.fear, cjk);
            clear_polluted_contract_string(&mut entry.expected_next_shift, cjk);
            clear_polluted_contract_string(&mut entry.payoff_target, cjk);
        }
        self.structured.emotional_state_ledger.retain(|entry| {
            !value_missing(&entry.character)
                || !value_missing(&entry.current_emotion)
                || !value_missing(&entry.expected_next_shift)
        });

        for relation in &mut self.structured.relationship_ledger {
            clear_polluted_contract_string_list(&mut relation.characters, cjk);
            clear_polluted_contract_string(&mut relation.arc_type, cjk);
            clear_polluted_contract_string(&mut relation.relationship_type, cjk);
            clear_polluted_contract_string(&mut relation.stage, cjk);
            clear_polluted_contract_string(&mut relation.next_expected_stage, cjk);
            clear_polluted_contract_string(&mut relation.start_state, cjk);
            clear_polluted_contract_string(&mut relation.current_state, cjk);
            clear_polluted_contract_string(&mut relation.desired_end_state, cjk);
            clear_polluted_contract_string(&mut relation.evidence, cjk);
            clear_polluted_contract_string_list(&mut relation.conflicts, cjk);
            clear_polluted_contract_string_list(&mut relation.secrets, cjk);
            clear_polluted_contract_string_list(&mut relation.turning_points, cjk);
        }
        self.structured.relationship_ledger.retain(|relation| {
            relation.characters.len() >= 2
                || !value_missing(&relation.relationship_type)
                || !value_missing(&relation.desired_end_state)
        });

        let power = &mut self.structured.power_progression;
        clear_polluted_contract_string(&mut power.system_name, cjk);
        clear_polluted_contract_string_list(&mut power.levels, cjk);
        clear_polluted_contract_string_list(&mut power.advancement_costs, cjk);
        clear_polluted_contract_string_list(&mut power.bottlenecks, cjk);
        clear_polluted_contract_string_list(&mut power.failure_consequences, cjk);
        clear_polluted_contract_string_list(&mut power.anti_power_creep_rules, cjk);

        let order = &mut self.structured.social_order;
        clear_polluted_contract_string_list(&mut order.institutions, cjk);
        clear_polluted_contract_string(&mut order.rank_system, cjk);
        clear_polluted_contract_string_list(&mut order.exam_or_promotion_rules, cjk);
        clear_polluted_contract_string_list(&mut order.laws, cjk);
        clear_polluted_contract_string(&mut order.class_structure, cjk);
        clear_polluted_contract_string_list(&mut order.authority_conflicts, cjk);

        let narration = &mut self.structured.narration_contract;
        clear_polluted_contract_string(&mut narration.pov, cjk);
        clear_polluted_contract_string(&mut narration.tense, cjk);
        clear_polluted_contract_string(&mut narration.narrative_distance, cjk);
        clear_polluted_contract_string(&mut narration.dialogue_style, cjk);
        clear_polluted_contract_string(&mut narration.description_density, cjk);
        clear_polluted_contract_string(&mut narration.chapter_pacing, cjk);
        clear_polluted_contract_string_list(&mut narration.forbidden_style_drift, cjk);
    }

    fn clear_top_level_surface_pollution(&mut self) {
        let cjk = self.contract_language_looks_cjk();
        clear_polluted_contract_string(&mut self.genre, cjk);
        clear_polluted_contract_string(&mut self.brief, cjk);
        clear_polluted_contract_string(&mut self.premise, cjk);
        clear_polluted_contract_string(&mut self.ending.desired_resolution, cjk);
        clear_polluted_contract_string(&mut self.ending.final_state, cjk);
        clear_polluted_contract_string_list(&mut self.ending.must_resolve, cjk);
        clear_polluted_contract_string_list(&mut self.ending.allowed_open_questions, cjk);
        clear_polluted_contract_string(&mut self.protagonist_arc, cjk);
        clear_polluted_contract_string(&mut self.world_imagery, cjk);
        clear_polluted_contract_string(&mut self.main_causal_spine, cjk);
        for character in &mut self.characters {
            clear_polluted_contract_string(&mut character.canonical_name, cjk);
            clear_polluted_contract_string(&mut character.role, cjk);
            clear_polluted_contract_string(&mut character.desire, cjk);
            clear_polluted_contract_string(&mut character.fear, cjk);
            clear_polluted_contract_string(&mut character.bottom_line, cjk);
            clear_polluted_contract_string(&mut character.arc_start, cjk);
            clear_polluted_contract_string(&mut character.arc_end, cjk);
            clear_polluted_contract_string_list(&mut character.aliases, cjk);
        }
        self.characters
            .retain(|character| !value_missing(&character.canonical_name));
        clear_polluted_contract_string_list(&mut self.themes, cjk);
        clear_polluted_contract_string_list(&mut self.world_rules, cjk);
        clear_polluted_contract_string_list(&mut self.style_rules, cjk);
        clear_polluted_contract_string_list(&mut self.must_avoid, cjk);
    }

    fn collect_primary_name_authority_blockers(&self, issues: &mut ContractIssueList) {
        let primary_names = self
            .characters
            .iter()
            .filter(|character| character.role_looks_primary())
            .map(|character| character.canonical_name.trim())
            .filter(|name| !value_missing(name))
            .collect::<Vec<_>>();
        if primary_names.len() != 1 {
            return;
        }
        let primary_name = primary_names[0];
        for (label, value) in [
            ("故事前提", self.premise.as_str()),
            ("主角弧线", self.protagonist_arc.as_str()),
            ("总主线因果链", self.main_causal_spine.as_str()),
            ("大纲", self.outline.raw_outline.as_str()),
        ] {
            for mentioned in explicit_primary_names_in_contract_text(value) {
                if mentioned != primary_name {
                    issues.push(format!(
                        "ContractBlocker: {label}中的主角名 `{mentioned}` 与角色权威表主角 `{primary_name}` 不一致"
                    ));
                }
            }
        }
        let authority_names = self.character_authority_names();
        for character in &self.characters {
            for (label, value) in [
                ("角色欲望", character.desire.as_str()),
                ("角色恐惧", character.fear.as_str()),
                ("角色底线", character.bottom_line.as_str()),
                ("角色弧线起点", character.arc_start.as_str()),
                ("角色弧线终点", character.arc_end.as_str()),
            ] {
                let mentioned_names = explicit_primary_names_in_contract_text(value);
                for mentioned in mentioned_names {
                    if mentioned != primary_name
                        && !mentioned.contains(primary_name)
                        && !authority_names.contains(&mentioned)
                        && stale_primary_name_candidate_looks_like_person(&mentioned)
                    {
                        issues.push(format!(
                            "ContractBlocker: {label}中的主角行动名 `{mentioned}` 与角色权威表主角 `{primary_name}` 不一致"
                        ));
                    }
                }
            }
        }
    }

    fn contract_language_looks_cjk(&self) -> bool {
        novel_runner::is_chinese_language(&self.language)
            || self.story_basis_text().chars().any(is_cjk_unified)
    }

    pub(crate) fn story_basis_text(&self) -> String {
        [
            self.brief.as_str(),
            self.premise.as_str(),
            self.ending.desired_resolution.as_str(),
            self.ending.final_state.as_str(),
            self.protagonist_arc.as_str(),
            self.world_imagery.as_str(),
            self.main_causal_spine.as_str(),
            self.outline.raw_outline.as_str(),
        ]
        .into_iter()
        .chain(self.themes.iter().map(String::as_str))
        .chain(self.world_rules.iter().map(String::as_str))
        .chain(
            self.characters
                .iter()
                .map(|character| character.desire.as_str()),
        )
        .filter(|value| !value_missing(value))
        .collect::<Vec<_>>()
        .join("\n")
    }
}

fn flat_structured_contract_from_boundary_value(value: &Value) -> NovelContractV2 {
    value
        .as_object()
        .and_then(|object| serde_json::from_value(Value::Object(object.clone())).ok())
        .unwrap_or_default()
}

fn novel_contract_v2_content_score(contract: &NovelContractV2) -> usize {
    let Ok(value) = serde_json::to_value(contract) else {
        return 0;
    };
    novel_contract_v2_value_content_score(&value)
}

fn novel_contract_v2_value_content_score(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Bool(_) | Value::Number(_) => 1,
        Value::String(text) => (!value_missing(text)).into(),
        Value::Array(items) => items
            .iter()
            .map(novel_contract_v2_value_content_score)
            .sum(),
        Value::Object(object) => object
            .iter()
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "schema_version" | "revision" | "field_requirements"
                )
            })
            .map(|(_, value)| novel_contract_v2_value_content_score(value))
            .sum(),
    }
}

fn normalize_contract_language(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.contains("zh") || trimmed.contains("中文") || trimmed.contains("汉语") {
        return "zh-CN".to_string();
    }
    if lowered.contains("english") || lowered == "en" || lowered.starts_with("en-") {
        return "en".to_string();
    }
    trimmed.to_string()
}

fn push_contract_string_list_surface_issues(
    issues: &mut ContractIssueList,
    cjk: bool,
    label: &str,
    values: &[String],
) {
    for (index, value) in values.iter().enumerate() {
        push_contract_surface_issue(issues, cjk, &format!("{label}{}", index + 1), value);
    }
}

fn push_contract_surface_issue(
    issues: &mut ContractIssueList,
    cjk: bool,
    label: &str,
    value: &str,
) {
    let Some(reason) = contract_text_surface_issue(value, cjk) else {
        return;
    };
    issues.push(format!(
        "ContractBlocker: {label}含有{reason}，需要重新生成干净合同字段"
    ));
}

fn push_structured_scalar_surface_issue(
    issues: &mut ContractIssueList,
    cjk: bool,
    label: &str,
    value: &str,
) {
    let Some(reason) = contract_text_surface_issue_without_request_controls(value, cjk) else {
        return;
    };
    issues.push(format!(
        "ContractBlocker: {label}含有{reason}，需要重新生成干净合同字段"
    ));
}

fn clear_polluted_contract_string(value: &mut String, cjk: bool) {
    if contract_text_clearable_surface_issue(value, cjk).is_some() {
        value.clear();
    }
}

fn clear_polluted_contract_string_list(values: &mut Vec<String>, cjk: bool) {
    values.retain(|value| contract_text_clearable_surface_issue(value, cjk).is_none());
}

fn contract_text_clearable_surface_issue(value: &str, cjk: bool) -> Option<&'static str> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value_contains_json_field_residue(value) {
        return Some("JSON 字段或结构残片");
    }
    if surface_sanitizer::contains_legal_contract_residue(value) {
        return Some("合同条款或交付协议残片");
    }
    if surface_sanitizer::contains_creation_request_control_residue(value) {
        return Some("用户请求参数或流程说明残片");
    }
    if surface_sanitizer::contains_generic_contract_placeholder_residue(value) {
        return Some("通用合同占位句");
    }
    if contract_value_contains_slot_label_placeholder(value) {
        return Some("合同槽位名占位");
    }
    if contract_value_contains_embedded_field_label(value) {
        return Some("其他合同字段标签残片");
    }
    if contract_value_has_unbalanced_delimiters(value) {
        return Some("未闭合引号或书名号");
    }
    if value_contains_numbered_task_spec_residue(value) {
        return Some("任务规格编号残片");
    }
    if value_contains_markup_math_residue(value) {
        return Some("LaTeX/转义/数学格式残片");
    }
    if cjk && value.chars().any(is_hangul_or_jamo) {
        return Some("非目标语言脚本残片");
    }
    None
}

fn contract_text_surface_issue(value: &str, cjk: bool) -> Option<&'static str> {
    contract_text_surface_issue_inner(value, cjk, true)
}

fn contract_text_surface_issue_without_request_controls(
    value: &str,
    cjk: bool,
) -> Option<&'static str> {
    contract_text_surface_issue_inner(value, cjk, false)
}

fn contract_text_surface_issue_inner(
    value: &str,
    cjk: bool,
    include_request_controls: bool,
) -> Option<&'static str> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value_contains_json_field_residue(value) {
        return Some("JSON 字段或结构残片");
    }
    if surface_sanitizer::contains_legal_contract_residue(value) {
        return Some("合同条款或交付协议残片");
    }
    if include_request_controls
        && surface_sanitizer::contains_creation_request_control_residue(value)
    {
        return Some("用户请求参数或流程说明残片");
    }
    if surface_sanitizer::contains_generic_contract_placeholder_residue(value) {
        return Some("通用合同占位句");
    }
    if contract_value_contains_slot_label_placeholder(value) {
        return Some("合同槽位名占位");
    }
    if contract_value_contains_embedded_field_label(value) {
        return Some("其他合同字段标签残片");
    }
    if contract_value_has_unbalanced_delimiters(value) {
        return Some("未闭合引号或书名号");
    }
    if value_contains_numbered_task_spec_residue(value) {
        return Some("任务规格编号残片");
    }
    if value_contains_markup_math_residue(value) {
        return Some("LaTeX/转义/数学格式残片");
    }
    if cjk && value.chars().any(is_hangul_or_jamo) {
        return Some("非目标语言脚本残片");
    }
    if cjk && contract_value_has_mechanical_connector_chain(value) {
        return Some("机械连接词链");
    }
    None
}

fn explicit_primary_names_in_contract_text(value: &str) -> Vec<String> {
    let text = value.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let chars = text.chars().collect::<Vec<_>>();
    let markers = ["主角", "主人公"];
    let mut out = Vec::new();
    for marker in markers {
        let marker_chars = marker.chars().collect::<Vec<_>>();
        let mut index = 0;
        while index + marker_chars.len() < chars.len() {
            if chars[index..].starts_with(&marker_chars) {
                let mut cursor = index + marker_chars.len();
                let mut has_explicit_name_marker = false;
                while cursor < chars.len() && matches!(chars[cursor], ' ' | '\t') {
                    cursor += 1;
                }
                if cursor < chars.len() && matches!(chars[cursor], '：' | ':') {
                    has_explicit_name_marker = true;
                    cursor += 1;
                }
                while cursor < chars.len() && matches!(chars[cursor], ' ' | '\t') {
                    cursor += 1;
                }
                if cursor < chars.len() && chars[cursor] == '叫' {
                    has_explicit_name_marker = true;
                    cursor += 1;
                } else if cursor + 1 < chars.len()
                    && chars[cursor] == '名'
                    && matches!(chars[cursor + 1], '为' | '叫')
                {
                    has_explicit_name_marker = true;
                    cursor += 2;
                }
                let mut candidate = if has_explicit_name_marker {
                    if cursor < chars.len()
                        && primary_name_candidate_starts_with_action(chars[cursor])
                    {
                        index = cursor + 1;
                        continue;
                    }
                    let mut candidate = String::new();
                    while cursor < chars.len() && is_cjk_unified(chars[cursor]) {
                        candidate.push(chars[cursor]);
                        cursor += 1;
                        if candidate.chars().count() >= 4 {
                            break;
                        }
                    }
                    trim_primary_name_candidate_tail_with_following(
                        &candidate,
                        chars.get(cursor).copied(),
                    )
                } else if let Some((candidate, next_cursor)) =
                    direct_primary_name_after_marker(&chars, cursor)
                {
                    cursor = next_cursor;
                    candidate
                } else {
                    index = cursor.max(index + 1);
                    continue;
                };
                candidate = trim_primary_name_candidate_tail(&candidate);
                let candidate_len = candidate.chars().count();
                if (2..=4).contains(&candidate_len) && !out.iter().any(|known| known == &candidate)
                {
                    out.push(candidate);
                }
                index = cursor;
            } else {
                index += 1;
            }
        }
    }
    out
}

fn direct_primary_name_after_marker(chars: &[char], cursor: usize) -> Option<(String, usize)> {
    if cursor >= chars.len() || primary_name_candidate_starts_with_action(chars[cursor]) {
        return None;
    }
    let mut buffer = String::new();
    let mut offset = 0usize;
    while cursor + offset < chars.len() && is_cjk_unified(chars[cursor + offset]) && offset < 4 {
        buffer.push(chars[cursor + offset]);
        offset += 1;
        if offset < 2 {
            continue;
        }
        let following = chars.get(cursor + offset).copied();
        let candidate = trim_primary_name_candidate_tail_with_following(&buffer, following);
        if !stale_primary_name_candidate_looks_like_person(&candidate) {
            continue;
        }
        let next_index = cursor + offset;
        let shortened = candidate.chars().count() < buffer.chars().count();
        if shortened || direct_primary_name_has_boundary(chars, next_index) {
            return Some((candidate, next_index));
        }
    }
    None
}

fn direct_primary_name_has_boundary(chars: &[char], index: usize) -> bool {
    if index >= chars.len() {
        return true;
    }
    let ch = chars[index];
    if matches!(
        ch,
        ' ' | '\t'
            | '，'
            | ','
            | '。'
            | '.'
            | '；'
            | ';'
            | '：'
            | ':'
            | '、'
            | '在'
            | '从'
            | '因'
            | '被'
            | '把'
            | '将'
            | '与'
            | '和'
            | '为'
            | '名'
            | '凭'
            | '靠'
            | '意'
            | '入'
    ) {
        return true;
    }
    primary_authority_action_terms().iter().any(|term| {
        let term_chars = term.chars().collect::<Vec<_>>();
        chars[index..].starts_with(&term_chars)
    })
}

fn align_primary_name_authority_text(
    value: &mut String,
    primary_name: &str,
    authority_names: &BTreeSet<String>,
    _allow_implicit_action_context: bool,
) -> bool {
    if value_missing(value) || value_missing(primary_name) {
        return false;
    }
    let mut repaired = repair_authority_name_tail_noise_in_text(value, authority_names);
    for marker in ["主角", "主人公", "男主", "女主"] {
        let role_labeled = format!("{marker}{primary_name}");
        if repaired.contains(&role_labeled) {
            repaired = repaired.replace(&role_labeled, primary_name);
        }
    }
    let candidates = explicit_primary_names_in_contract_text(value);
    for stale in candidates {
        if stale == primary_name || authority_names.contains(&stale) {
            continue;
        }
        if primary_authority_name_with_glued_tail(&stale, primary_name) {
            repaired = repaired.replace(&stale, primary_name);
            continue;
        }
        if !stale_primary_name_candidate_looks_like_person(&stale) {
            continue;
        }
        repaired = repaired.replace(&stale, primary_name);
    }
    if repaired == *value {
        return false;
    }
    *value = repaired;
    true
}

fn repair_authority_name_tail_noise_in_text(
    value: &str,
    authority_names: &BTreeSet<String>,
) -> String {
    if value_missing(value) || authority_names.is_empty() {
        return value.to_string();
    }
    let mut names = authority_names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    names.sort_by_key(|name| std::cmp::Reverse(name.chars().count()));
    let chars = value.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        let mut repaired = false;
        for known in &names {
            let known_chars = known.chars().collect::<Vec<_>>();
            if known_chars.is_empty()
                || index + known_chars.len() >= chars.len()
                || chars[index..index + known_chars.len()] != known_chars[..]
            {
                continue;
            }
            let tail_index = index + known_chars.len();
            let tail = chars[tail_index];
            if !is_cjk_unified(tail) {
                continue;
            }
            let after_tail = chars[tail_index + 1..].iter().collect::<String>();
            if !authority_tail_looks_glued_in_text(known, tail, &after_tail) {
                continue;
            }
            out.push_str(known);
            index = tail_index + 1;
            repaired = true;
            break;
        }
        if !repaired {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn primary_authority_name_with_glued_tail(reference: &str, primary_name: &str) -> bool {
    if value_missing(reference) || value_missing(primary_name) || reference == primary_name {
        return false;
    }
    let Some(tail) = reference.trim().strip_prefix(primary_name.trim()) else {
        return false;
    };
    authority_tail_looks_glued_to_name(tail)
}

fn authority_tail_looks_glued_to_name(tail: &str) -> bool {
    let tail_len = tail.chars().count();
    tail_len == 1
        && tail.chars().all(is_cjk_unified)
        && !direct_primary_name_has_boundary(&tail.chars().collect::<Vec<_>>(), 0)
}

fn authority_tail_looks_glued_in_text(known: &str, tail: char, after_tail: &str) -> bool {
    if authority_tail_is_valid_following_word_start(tail)
        || authority_tail_starts_valid_action(tail, after_tail)
    {
        return false;
    }
    if known.chars().last().is_some_and(|last| last == tail) {
        return true;
    }
    if after_tail.chars().next().is_none_or(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                '，' | '。'
                    | '；'
                    | '、'
                    | ','
                    | ';'
                    | '-'
                    | '>'
                    | '→'
                    | '的'
                    | '地'
                    | '得'
                    | '为'
                    | '并'
                    | '而'
                    | '却'
                    | '也'
                    | '仍'
                    | '又'
                    | '若'
                    | '如'
                    | '将'
                    | '把'
                    | '被'
                    | '向'
                    | '与'
                    | '和'
            )
    }) {
        return true;
    }
    primary_authority_action_terms()
        .iter()
        .chain(relationship_action_terms().iter())
        .any(|term| after_tail.starts_with(term))
}

fn authority_tail_starts_valid_action(tail: char, after_tail: &str) -> bool {
    let following = format!("{tail}{after_tail}");
    primary_authority_action_terms()
        .iter()
        .chain(relationship_action_terms().iter())
        .any(|term| following.starts_with(term))
}

fn authority_tail_is_valid_following_word_start(tail: char) -> bool {
    primary_name_candidate_trailing_connector(tail)
        || matches!(
            tail,
            '的' | '地'
                | '得'
                | '和'
                | '与'
                | '及'
                | '或'
                | '但'
                | '却'
                | '而'
                | '都'
                | '也'
                | '又'
                | '并'
                | '仍'
                | '是'
                | '让'
                | '使'
                | '不'
                | '未'
                | '了'
        )
}

fn relationship_action_terms() -> &'static [&'static str] {
    &[
        "试图", "企图", "准备", "计划", "开始", "决定", "继续", "再次", "已经", "正在", "奉命",
        "掌握", "公开", "追查", "嫁祸", "背叛", "击败", "控制", "保护", "帮助", "利用", "牺牲",
        "反制", "瓦解", "继承", "启动", "成为", "留在", "怀疑", "携手", "合作", "联手", "平定",
        "推行", "遭遇", "升迁", "出征", "功高", "退让", "接管", "建立", "调查", "查明", "放弃",
        "卸任", "掌控", "协助", "面对", "应对", "达成", "取得",
    ]
}

fn stale_primary_name_candidate_looks_like_person(candidate: &str) -> bool {
    let len = candidate.chars().count();
    if !(2..=4).contains(&len) || !candidate.chars().any(is_cjk_unified) {
        return false;
    }
    let concept_terms = [
        "主角", "主人", "凡人", "修士", "宗门", "天道", "天地", "剑道", "法则", "规则", "灵脉",
        "灵气", "灵能", "债务", "账册", "证据", "公开", "世界", "终局", "主线", "修行", "境界",
        "本源", "弧线", "成长", "围绕", "核心", "剧情", "合同", "大纲", "情感", "关系", "爽点",
        "读者", "章节", "卷宗", "伏笔", "结局", "命名", "标题",
    ];
    if concept_terms.iter().any(|term| candidate.contains(term)) {
        return false;
    }
    if primary_name_candidate_contains_non_person_contract_term(candidate) {
        return false;
    }
    if primary_authority_action_terms()
        .iter()
        .any(|term| candidate == *term)
    {
        return false;
    }
    let chars = candidate.chars().collect::<Vec<_>>();
    if chars
        .windows(2)
        .any(|window| matches!(window, ['之', _] | ['的', _] | ['与', _] | ['和', _]))
    {
        return false;
    }
    true
}

fn primary_name_candidate_contains_non_person_contract_term(candidate: &str) -> bool {
    let non_person_terms = [
        "账本",
        "账簿",
        "账单",
        "账册",
        "系统",
        "道具",
        "神器",
        "法宝",
        "令牌",
        "钥匙",
        "证物",
        "契约",
        "协议",
        "阵法",
        "符文",
        "灵契",
        "残卷",
        "玉简",
        "芯片",
        "程序",
        "网络",
        "平台",
        "公司",
        "集团",
        "学院",
        "学校",
        "城市",
        "矿脉",
        "节点",
        "回收站",
        "实验室",
        "办公室",
        "交易所",
        "数据库",
        "档案",
        "账目",
    ];
    non_person_terms.iter().any(|term| candidate.contains(term))
}

fn primary_authority_action_terms() -> &'static [&'static str] {
    &[
        "突破", "飞升", "入道", "开道", "渡劫", "晋阶", "进阶", "觉醒", "证道", "改写", "公开",
        "夺回", "守住", "承担", "重塑", "重建", "击败", "超脱", "揭开", "揭露", "发现", "查明",
        "统领",
    ]
}

fn trim_primary_name_candidate_tail(candidate: &str) -> String {
    let mut out = candidate.trim().to_string();
    while out.chars().count() > 2
        && out
            .chars()
            .last()
            .is_some_and(primary_name_candidate_trailing_connector)
    {
        out.pop();
    }
    for marker in [
        "意外", "发现", "觉醒", "获得", "掌控", "进入", "公开", "击败", "建立", "成为", "遭遇",
        "面对", "带着", "凭借", "围绕", "推动", "承担", "经历", "完成", "实现",
    ] {
        if let Some(index) = out.find(marker) {
            if index > 0 {
                out.truncate(index);
            }
        }
    }
    out
}

fn trim_primary_name_candidate_tail_with_following(
    candidate: &str,
    following: Option<char>,
) -> String {
    let mut out = trim_primary_name_candidate_tail(candidate);
    let Some(next) = following else {
        return out;
    };
    if out.chars().count() <= 2 {
        return out;
    }
    let Some(last) = out.chars().last() else {
        return out;
    };
    if primary_name_candidate_tail_continues_action(last, next) {
        out.pop();
        out = trim_primary_name_candidate_tail(&out);
    }
    out
}

fn primary_name_candidate_tail_continues_action(last: char, next: char) -> bool {
    [
        "成为", "成长", "成功", "建立", "掌控", "获得", "发现", "遭遇", "面对", "带着", "凭借",
        "推动", "承担", "经历", "完成", "实现", "打破", "击败", "夺回", "守住", "重塑", "重建",
        "揭开", "揭露", "查明", "统领", "原本", "原来", "原先",
    ]
    .iter()
    .any(|term| {
        let mut chars = term.chars();
        chars.next() == Some(last) && chars.next() == Some(next)
    })
}

fn primary_name_candidate_trailing_connector(ch: char) -> bool {
    matches!(
        ch,
        '在' | '于'
            | '从'
            | '向'
            | '与'
            | '和'
            | '因'
            | '被'
            | '把'
            | '将'
            | '对'
            | '为'
            | '凭'
            | '靠'
            | '遭'
            | '受'
            | '需'
            | '要'
            | '能'
            | '会'
            | '已'
            | '正'
    )
}

fn primary_name_candidate_starts_with_action(ch: char) -> bool {
    matches!(
        ch,
        '被' | '从'
            | '在'
            | '因'
            | '由'
            | '向'
            | '与'
            | '和'
            | '及'
            | '把'
            | '将'
            | '遭'
            | '受'
            | '凭'
            | '靠'
            | '追'
            | '查'
            | '改'
            | '守'
            | '救'
            | '找'
            | '拿'
            | '赢'
            | '破'
            | '对'
            | '为'
            | '需'
            | '要'
            | '能'
            | '会'
            | '已'
            | '曾'
            | '正'
    )
}

fn contract_value_has_mechanical_connector_chain(value: &str) -> bool {
    ["然后", "接着", "随后", "再然后"]
        .iter()
        .any(|marker| value.matches(marker).count() >= 2)
}

fn contract_value_is_numeric_placeholder(value: &str) -> bool {
    let compact = value
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '“' | '”' | '：' | ':' | '.' | '。'));
    !compact.is_empty()
        && compact
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '一' | '二' | '三' | '四' | '五'))
        && compact.chars().count() <= 3
}

fn contract_value_is_chapter_label_placeholder(value: &str) -> bool {
    let compact = value
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '“' | '”' | '：' | ':' | '.' | '。'))
        .replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    let lowered = compact.to_ascii_lowercase();
    if lowered.starts_with("chapter") {
        return lowered["chapter".len()..]
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '_' | '#'));
    }
    compact.starts_with('第') && compact.ends_with('章') && compact.chars().count() <= 5
}

fn contract_outline_looks_glued_control_blocks(outline: &str) -> bool {
    outline.lines().any(|line| {
        let compact = line.replace(char::is_whitespace, "");
        if compact.chars().count() < 120 {
            return false;
        }
        let structural_markers = ["第", "卷", "章", "本章目标", "预期转折", "卷尾变化"]
            .iter()
            .filter(|marker| compact.contains(**marker))
            .count();
        let chapter_or_volume_refs = compact.matches('章').count() + compact.matches('卷').count();
        structural_markers >= 4 && chapter_or_volume_refs >= 4
    })
}

fn value_contains_json_field_residue(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    let trimmed = value.trim();
    trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.ends_with('}')
        || trimmed.ends_with(']')
        || value.contains("\":")
        || value.contains("\\\"")
        || [
            "canonical_title",
            "title_rationale",
            "main_causal_spine",
            "desired_resolution",
            "final_state",
            "protagonist_arc",
            "world_imagery",
            "structured_contract",
        ]
        .iter()
        .any(|marker| lowered.contains(marker))
}

fn value_contains_numbered_task_spec_residue(value: &str) -> bool {
    let compact = value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if cjk_sentence_ends_with_orphan_numbered_marker(&compact) {
        return true;
    }
    let has_numbered_shape = compact.contains("2.")
        || compact.contains("3.")
        || compact.contains("4.")
        || compact.contains("5.")
        || compact.contains("第二条")
        || compact.contains("第三条")
        || compact.contains("第四条")
        || compact.contains("第五条")
        || compact.contains("二、")
        || compact.contains("三、")
        || compact.contains("四、")
        || compact.contains("五、");
    if !has_numbered_shape {
        return false;
    }
    let has_units =
        compact.contains("作品字数") || compact.contains("小说字数") || compact.contains("总字数");
    let has_chapters =
        compact.contains("章节数量") || compact.contains("每章约") || compact.contains("每章目标");
    let has_task_spec = [
        "作品需",
        "作品需要",
        "小说需",
        "小说需要",
        "具备较强",
        "可读性",
        "逻辑性",
        "吸引读者",
        "持续阅读",
        "创作要求",
        "写作要求",
        "内容要求",
        "不得违反",
        "法律法规",
        "公序良俗",
        "提交审阅",
        "修改意见",
    ]
    .iter()
    .any(|marker| compact.contains(marker));
    has_units && has_chapters || has_task_spec
}

fn cjk_sentence_ends_with_orphan_numbered_marker(compact: &str) -> bool {
    [
        "。二", "。三", "。四", "。五", "；二", "；三", "；四", "；五", ".2", ".3", ".4", ".5",
    ]
    .iter()
    .any(|marker| compact.ends_with(marker))
}

fn value_contains_markup_math_residue(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.contains("\\rightarrow")
        || lowered.contains("rightarrow")
        || lowered.contains("ightarrow")
        || lowered.contains("\\l")
        || lowered.contains("\\ ^{}")
        || lowered.contains("\\^{}")
        || (value.contains('$') && value.matches('$').count() >= 2)
}

fn is_cjk_unified(ch: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&ch)
}

fn is_hangul_or_jamo(ch: char) -> bool {
    ('\u{1100}'..='\u{11FF}').contains(&ch)
        || ('\u{3130}'..='\u{318F}').contains(&ch)
        || ('\u{AC00}'..='\u{D7AF}').contains(&ch)
}

impl OutlineContract {
    pub(crate) fn has_stage_or_near_chapter_plan(&self) -> bool {
        !self.volumes.is_empty() || !self.near_chapters.is_empty()
    }
}

impl CharacterContract {
    pub(crate) fn role_looks_primary(&self) -> bool {
        let role = self.role.to_ascii_lowercase();
        self.role.contains("主角")
            || self.role.contains("主人公")
            || self.role.contains("男主")
            || self.role.contains("女主")
            || role.contains("protagonist")
            || role.contains("main character")
    }

    pub(crate) fn to_draft_line(&self) -> String {
        format!(
            "character_id: {}; name: {}; aliases: {}; previous_names: {}; role: {}; desire: {}; fear: {}; bottom_line: {}; arc_start: {}; arc_end: {}; planned_entry: {}; planned_exit: {}; name_source: {}",
            self.character_id,
            self.canonical_name,
            self.aliases.join("|"),
            self.previous_names.join("|"),
            self.role,
            self.desire,
            self.fear,
            self.bottom_line,
            self.arc_start,
            self.arc_end,
            self.planned_entry,
            self.planned_exit,
            self.name_source,
        )
    }
}

pub(crate) fn value_missing(value: &str) -> bool {
    let compact = value.trim();
    if compact.is_empty() {
        return true;
    }
    let lowered = compact.to_ascii_lowercase();
    [
        "未指定",
        "待补",
        "待补充",
        "待完善",
        "待定",
        "暂无",
        "不详",
        "未明",
        "未明欲望",
        "未明恐惧",
        "未明底线",
        "unknown",
        "not specified",
        "unspecified",
        "placeholder",
        "(none)",
    ]
    .iter()
    .any(|marker| compact.contains(marker) || lowered.contains(marker))
        || contract_value_is_slot_label_placeholder(compact)
}

fn contract_value_is_slot_label_placeholder(value: &str) -> bool {
    let compact = compact_contract_slot_placeholder_text(value);
    !compact.is_empty()
        && contract_slot_placeholder_labels()
            .iter()
            .any(|label| compact == *label)
}

fn contract_value_contains_slot_label_placeholder(value: &str) -> bool {
    let compact = compact_contract_slot_placeholder_text(value);
    if compact.is_empty() {
        return false;
    }
    if contract_value_is_slot_label_placeholder(&compact) {
        return true;
    }
    contract_slot_placeholder_labels()
        .iter()
        .filter(|label| label.chars().count() >= 4)
        .any(|label| value_contains_quoted_slot_placeholder(value, label))
}

fn contract_value_contains_embedded_field_label(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    embedded_contract_field_labels()
        .iter()
        .any(|label| compact.contains(label))
}

fn embedded_contract_field_labels() -> &'static [&'static str] {
    &[
        "欲望：",
        "欲望:",
        "恐惧：",
        "恐惧:",
        "底线：",
        "底线:",
        "弧线起点：",
        "弧线起点:",
        "弧线终点：",
        "弧线终点:",
    ]
}

fn contract_value_has_unbalanced_delimiters(value: &str) -> bool {
    delimiter_count_mismatch(value, '“', '”')
        || delimiter_count_mismatch(value, '‘', '’')
        || delimiter_count_mismatch(value, '《', '》')
        || delimiter_count_mismatch(value, '「', '」')
        || delimiter_count_mismatch(value, '『', '』')
}

fn delimiter_count_mismatch(value: &str, open: char, close: char) -> bool {
    value.matches(open).count() != value.matches(close).count()
}

fn value_contains_quoted_slot_placeholder(value: &str, label: &str) -> bool {
    [
        format!("`{label}`"),
        format!("'{label}'"),
        format!("\"{label}\""),
        format!("“{label}”"),
        format!("‘{label}’"),
        format!("《{label}》"),
        format!("「{label}」"),
        format!("『{label}』"),
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

fn compact_contract_slot_placeholder_text(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch| {
            matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '《'
                    | '》'
                    | '['
                    | ']'
                    | '【'
                    | '】'
                    | '('
                    | ')'
                    | '（'
                    | '）'
                    | ':'
                    | '：'
                    | '.'
                    | '。'
                    | ','
                    | '，'
                    | ';'
                    | '；'
                    | '-'
                    | ' '
            )
        })
        .replace(char::is_whitespace, "")
}

fn contract_slot_placeholder_labels() -> &'static [&'static str] {
    &[
        "创作简述",
        "故事前提",
        "终局方向",
        "终局状态",
        "结局目标",
        "主角弧线",
        "世界观意象",
        "总主线因果链",
        "主线因果链",
        "情感承诺",
        "终局情绪落点",
        "读者期待",
        "爽点合同",
        "核心主题",
        "世界规则",
        "角色权威表",
        "关系账本",
        "情感线",
        "关系线",
        "分卷规划",
        "章节规划",
        "近期章节",
        "章节目标",
        "预期转折",
        "大纲",
    ]
}

fn normalize_model_turn_chapter_count(
    value: Option<usize>,
    target_units: Option<usize>,
    chapter_unit_target: Option<usize>,
) -> Option<usize> {
    let value = value.filter(|value| *value > 0).unwrap_or(1);
    if value_looks_like_total_chapter_count(value, target_units, chapter_unit_target) {
        return Some(1);
    }
    Some(value.clamp(1, 5))
}

fn value_looks_like_total_chapter_count(
    value: usize,
    target_units: Option<usize>,
    chapter_unit_target: Option<usize>,
) -> bool {
    if value < 8 {
        return false;
    }
    let (Some(target_units), Some(chapter_unit_target)) = (target_units, chapter_unit_target)
    else {
        return value > 5;
    };
    if target_units == 0 || chapter_unit_target == 0 {
        return value > 5;
    }
    let estimated = longform_policy::expected_chapter_count(target_units, chapter_unit_target)
        .expect("positive contract targets have an expected chapter count");
    value.abs_diff(estimated) <= 2 || value > 5
}

fn normalize_string(value: &mut String) {
    *value = normalize_contract_scalar(value);
}

fn normalize_string_vec(values: &mut Vec<String>) {
    let mut normalized: Vec<String> = Vec::new();
    for value in std::mem::take(values) {
        let value = normalize_contract_scalar(&value);
        if value.is_empty() || normalized.iter().any(|known| known.as_str() == value) {
            continue;
        }
        normalized.push(value);
    }
    *values = normalized;
}

fn normalize_world_rules_vec(values: &mut Vec<String>) {
    let mut normalized: Vec<String> = Vec::new();
    let mut pending_fragment: Option<String> = None;
    for value in std::mem::take(values) {
        for rule in split_world_rule_segments(&value) {
            let rule = normalize_world_rule_segment(&rule);
            if rule.is_empty() || world_rule_segment_is_heading(&rule) {
                continue;
            }
            if pending_fragment.is_some()
                && typed_contract_gate::world_rule_clause_completes_pending(&rule)
            {
                let mut previous = pending_fragment
                    .take()
                    .expect("pending world-rule fragment was checked above");
                previous.push('；');
                previous.push_str(&rule);
                if !typed_contract_gate::world_rule_looks_truncated_or_not_actionable(&previous) {
                    normalized.push(previous);
                }
                continue;
            }
            if typed_contract_gate::world_rule_clause_depends_on_previous(&rule) {
                if let Some(previous) = normalized.last_mut() {
                    previous.push('；');
                    previous.push_str(&rule);
                    continue;
                }
            }
            pending_fragment = None;
            if typed_contract_gate::world_rule_looks_truncated_or_not_actionable(&rule) {
                pending_fragment = Some(rule);
                continue;
            }
            if normalized.iter().any(|known| known.as_str() == rule) {
                continue;
            }
            normalized.push(rule);
        }
    }
    *values = normalized;
}

fn world_rule_segment_is_heading(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() || compact.chars().count() > 16 {
        return false;
    }
    let describes_action = [
        "必须", "不能", "只能", "只有", "若", "如果", "一旦", "否则", "会", "将", "需", "可",
        "触发", "导致", "消耗", "失去", "获得",
    ]
    .iter()
    .any(|marker| compact.contains(marker));
    !describes_action
        && [
            "代价", "限制", "规则", "机制", "条件", "后果", "稀缺", "门槛", "法则",
        ]
        .iter()
        .any(|suffix| compact.ends_with(suffix))
}

fn split_world_rule_segments(value: &str) -> Vec<String> {
    let value = normalize_contract_scalar(value);
    if value.is_empty() {
        return Vec::new();
    }
    let parts = value
        .split(['；', ';', '。'])
        .map(|part| part.trim_matches(|ch: char| ch.is_whitespace() || matches!(ch, '，' | ',')))
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.len() >= 2 {
        parts
    } else {
        vec![value]
    }
}

fn normalize_world_rule_segment(value: &str) -> String {
    let mut rule = normalize_contract_scalar(value);
    if rule.is_empty() {
        return rule;
    }
    if let Some(stripped) = strip_numbered_world_rule_label(&rule) {
        let stripped = normalize_contract_scalar(stripped);
        if stripped.is_empty() {
            return String::new();
        }
        if typed_contract_gate::world_rule_looks_truncated_or_not_actionable(&stripped) {
            return String::new();
        }
        rule = stripped;
    }
    rule
}

fn strip_numbered_world_rule_label(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    let rest = trimmed
        .strip_prefix("规则")
        .or_else(|| trimmed.strip_prefix("rule"))
        .or_else(|| trimmed.strip_prefix("Rule"))?;
    let rest = rest.trim_start();
    let digit_len = rest
        .char_indices()
        .take_while(|(_, ch)| {
            ch.is_ascii_digit()
                || matches!(
                    ch,
                    '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
                )
        })
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    let rest = rest.get(digit_len..)?.trim_start();
    let rest = rest
        .strip_prefix(':')
        .or_else(|| rest.strip_prefix('：'))
        .or_else(|| rest.strip_prefix('.'))
        .or_else(|| rest.strip_prefix('、'))?;
    Some(rest.trim())
}

fn normalize_contract_scalar(value: &str) -> String {
    let mut out = value
        .replace("\\n", "")
        .replace("\\r", "")
        .replace("\\t", " ")
        .replace(['\n', '\r'], "")
        .replace('\t', " ");
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    let closed = surface_sanitizer::close_trailing_unbalanced_cjk_delimiters(out.trim());
    surface_sanitizer::collapse_adjacent_repeated_cjk_phrases(&closed)
}

fn has_meaningful_contract_content(contract: &NovelCreationContract) -> bool {
    !value_missing(&contract.title.canonical_title)
        || !value_missing(&contract.premise)
        || !value_missing(&contract.ending.desired_resolution)
        || !value_missing(&contract.main_causal_spine)
        || !contract.characters.is_empty()
}

fn string_field(object: &serde_json::Map<String, Value>, key: &str) -> String {
    object.get(key).map(string_from_value).unwrap_or_default()
}

fn string_field_aliases(object: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| object.get(*key).map(string_from_value))
        .unwrap_or_default()
}

fn nested_string_field(
    object: &serde_json::Map<String, Value>,
    parent: &str,
    key: &str,
) -> Option<String> {
    object
        .get(parent)
        .and_then(Value::as_object)
        .and_then(|inner| inner.get(key))
        .map(string_from_value)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn usize_field(object: &serde_json::Map<String, Value>, key: &str) -> Option<usize> {
    object.get(key).and_then(usize_from_value)
}

fn string_array_field(object: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    object
        .get(key)
        .map(string_array_from_value)
        .unwrap_or_default()
}

fn nested_string_array_field(
    object: &serde_json::Map<String, Value>,
    parent: &str,
    key: &str,
) -> Option<Vec<String>> {
    object
        .get(parent)
        .and_then(Value::as_object)
        .and_then(|inner| inner.get(key))
        .map(string_array_from_value)
}

fn string_array_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::trim))
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        Value::String(text) => text
            .split(['\n', '；', ';', '、'])
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn string_from_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        _ => String::new(),
    }
}

fn character_array_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Vec<CharacterContract> {
    let mut characters = object
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if let Ok(character) = serde_json::from_value::<CharacterContract>(item.clone())
                    {
                        return Some(character);
                    }
                    item.as_str()
                        .map(super::super::creation_contract::draft_character_line_to_contract)
                })
                .collect()
        })
        .unwrap_or_default();
    normalize_character_contracts(&mut characters);
    characters
}

fn normalize_character_contracts(characters: &mut Vec<CharacterContract>) {
    characters.retain(|character| {
        let name = character.canonical_name.trim();
        !value_missing(name)
            && !contract_value_is_slot_label_placeholder(name)
            && character_contract_has_authority_payload(character)
    });
    normalize_character_contract_roles(characters, true);
    let mut seen = BTreeSet::new();
    characters.retain(|character| seen.insert(character.canonical_name.trim().to_string()));
}

pub(crate) fn normalize_character_contract_roles(
    characters: &mut [CharacterContract],
    infer_missing_roles: bool,
) {
    for character in characters.iter_mut() {
        normalize_string(&mut character.role);
        if let Some(role) = canonical_machine_character_role(&character.role) {
            character.role = role.to_string();
        }
    }

    if infer_missing_roles {
        let has_primary = characters.iter().any(CharacterContract::role_looks_primary);
        for (index, character) in characters.iter_mut().enumerate() {
            if value_missing(&character.role)
                || character_role_is_generic_placeholder(&character.role)
            {
                character.role = inferred_character_role(character, index, has_primary);
            }
        }
    }

    let mut primary_seen = false;
    for character in characters.iter_mut() {
        if !character.role_looks_primary() {
            continue;
        }
        if primary_seen {
            character.role = if character.role.contains("女主") || character.role.contains("男主")
            {
                "关键关系对象".to_string()
            } else {
                "关键角色".to_string()
            };
        } else {
            primary_seen = true;
        }
    }
}

fn canonical_machine_character_role(role: &str) -> Option<&'static str> {
    let key = role
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match key.as_str() {
        "femalelead" | "femaleprotagonist" => Some("女主"),
        "malelead" | "maleprotagonist" => Some("男主"),
        "protagonist" | "maincharacter" | "leadcharacter" => Some("主角"),
        "antagonist" | "villain" | "rival" => Some("关键对手"),
        "loveinterest" | "romanticlead" => Some("关键关系对象"),
        "mentor" => Some("导师"),
        "companion" | "ally" | "supportingcharacter" | "deuteragonist" => Some("关键同伴"),
        _ => None,
    }
}

fn inferred_character_role(
    character: &CharacterContract,
    index: usize,
    has_primary: bool,
) -> String {
    if !has_primary && index == 0 {
        return "主角".to_string();
    }
    let text = [
        character.role.as_str(),
        character.desire.as_str(),
        character.fear.as_str(),
        character.bottom_line.as_str(),
        character.arc_start.as_str(),
        character.arc_end.as_str(),
    ]
    .join(" ");
    let lowered = text.to_ascii_lowercase();
    if text.contains("反派")
        || text.contains("对手")
        || text.contains("敌")
        || text.contains("垄断")
        || text.contains("压制")
        || text.contains("维护")
        || text.contains("阻止")
        || lowered.contains("antagonist")
        || lowered.contains("rival")
    {
        return "关键对手".to_string();
    }
    if text.contains("导师") || text.contains("师父") || text.contains("老师") {
        return "导师".to_string();
    }
    if text.contains("恋")
        || text.contains("爱情")
        || text.contains("信任")
        || text.contains("关系")
        || text.contains("共同")
    {
        return "关键关系对象".to_string();
    }
    "关键同伴".to_string()
}

fn character_role_is_generic_placeholder(role: &str) -> bool {
    let compact = role.replace(char::is_whitespace, "");
    matches!(
        compact.as_str(),
        "角色" | "人物" | "关键角色" | "主要角色" | "配角" | "重要角色"
    )
}

fn character_contract_has_authority_payload(character: &CharacterContract) -> bool {
    !value_missing(&character.role)
        || !value_missing(&character.desire)
        || !value_missing(&character.fear)
        || !value_missing(&character.bottom_line)
        || !value_missing(&character.arc_start)
        || !value_missing(&character.arc_end)
}

fn outline_field(object: &serde_json::Map<String, Value>, key: &str) -> OutlineContract {
    let Some(value) = object.get(key) else {
        return OutlineContract::default();
    };
    if let Ok(outline) = serde_json::from_value::<OutlineContract>(value.clone()) {
        return outline;
    }
    if let Some(inner) = value.as_object() {
        return OutlineContract {
            volumes: outline_volumes_from_object(inner),
            near_chapters: outline_near_chapters_from_object(inner),
            raw_outline: string_field_aliases(inner, &["raw_outline", "raw", "summary"]),
        };
    }
    OutlineContract {
        raw_outline: value.as_str().unwrap_or_default().trim().to_string(),
        ..Default::default()
    }
}

fn outline_volumes_from_object(object: &serde_json::Map<String, Value>) -> Vec<VolumeContract> {
    object
        .get("volumes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if let Ok(volume) = serde_json::from_value::<VolumeContract>(item.clone()) {
                        return Some(volume);
                    }
                    let object = item.as_object()?;
                    Some(VolumeContract {
                        title: string_field(object, "title"),
                        objective: string_field(object, "objective"),
                        ending_change: string_field_aliases(
                            object,
                            &["ending_change", "ending", "final_change"],
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn outline_near_chapters_from_object(
    object: &serde_json::Map<String, Value>,
) -> Vec<ChapterSeedContract> {
    object
        .get("near_chapters")
        .or_else(|| object.get("chapters"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if let Ok(chapter) = serde_json::from_value::<ChapterSeedContract>(item.clone())
                    {
                        return Some(chapter);
                    }
                    let object = item.as_object()?;
                    Some(ChapterSeedContract {
                        number: object.get("number").and_then(usize_from_value),
                        goal: string_field_aliases(object, &["goal", "title", "objective"]),
                        expected_turn: string_field_aliases(
                            object,
                            &["expected_turn", "turn", "change", "payoff"],
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn usize_from_value(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .or_else(|| value.as_str()?.trim().parse::<usize>().ok())
}

#[cfg(test)]
mod tests;
