use super::*;

#[cfg(test)]
pub(crate) fn apply_generated_contract_to_creation_draft(
    draft: &mut SessionCreationDraftState,
    contract_text: &str,
) -> bool {
    submit_generated_contract_candidate_to_draft(draft, contract_text).is_ready()
}

pub(crate) fn apply_strong_novel_contract_to_creation_draft(
    draft: &mut SessionCreationDraftState,
    contract: &mut NovelCreationContract,
) -> bool {
    if draft.artifact_kind != "fiction" {
        return false;
    }
    prune_explicit_non_character_draft_authority(draft);
    prune_explicit_non_character_contract_characters(draft, contract);
    contract.normalize();
    complete_minimum_character_slots(&mut contract.characters, draft);
    if !draft_has_character_authority(draft) {
        let governance = patch::govern_initial_character_names(&mut contract.characters, draft);
        patch::rewrite_novel_contract_names(contract, governance.replacements());
        patch::canonicalize_novel_contract_to_character_authority(contract);
        governance.lock_authority(&mut contract.characters);
    } else {
        align_contract_characters_to_existing_draft_authority(draft, contract);
        patch::canonicalize_novel_contract_to_character_authority(contract);
    }
    let contract_ready = contract
        .validate_for_scope(ContractReadinessScope::DisplayContract)
        .is_ready();
    let before = serde_json::to_value(&*draft).ok();
    if contract_ready && !contract.title.canonical_title.trim().is_empty() {
        draft.title = contract.title.canonical_title.clone();
    } else {
        merge_missing_aware_string(&mut draft.title, &contract.title.canonical_title);
    }
    if !contract.language.trim().is_empty() {
        draft.language = contract.language.clone();
    }
    draft.genre = merge_short_field(&draft.genre, &contract.genre);
    replace_contract_generated_brief(&mut draft.brief, &contract.brief);
    if draft.target_units_user_specified {
        contract.target_units = draft.target_units;
    } else {
        draft.target_units = contract.target_units.or(draft.target_units);
    }
    if draft.chapter_unit_target_user_specified {
        contract.chapter_unit_target = draft.chapter_unit_target;
    } else {
        draft.chapter_unit_target = contract.chapter_unit_target.or(draft.chapter_unit_target);
    }
    draft.max_chapters_per_turn = draft
        .max_chapters_per_turn
        .or(contract.max_chapters_per_turn);
    let contract_ending_direction = first_non_empty_string(&[
        contract.ending.desired_resolution.as_str(),
        contract.ending.final_state.as_str(),
    ]);
    if contract_ready {
        draft.fiction_premise = contract.premise.clone();
        draft.fiction_ending_direction = contract_ending_direction;
        draft.fiction_protagonist_arc = contract.protagonist_arc.clone();
        draft.fiction_world_imagery = contract.world_imagery.clone();
        draft.fiction_main_causal_spine = contract.main_causal_spine.clone();
    } else {
        merge_missing_aware_string(&mut draft.fiction_premise, &contract.premise);
        merge_missing_aware_string(
            &mut draft.fiction_ending_direction,
            &contract_ending_direction,
        );
        merge_missing_aware_string(
            &mut draft.fiction_protagonist_arc,
            &contract.protagonist_arc,
        );
        merge_missing_aware_string(&mut draft.fiction_world_imagery, &contract.world_imagery);
        merge_missing_aware_string(
            &mut draft.fiction_main_causal_spine,
            &contract.main_causal_spine,
        );
    }
    if contract_ready && !contract.title.rationale.trim().is_empty() {
        draft.fiction_title_rationale = contract.title.rationale.clone();
    } else {
        merge_missing_aware_string(
            &mut draft.fiction_title_rationale,
            &contract.title.rationale,
        );
    }
    if !contract.themes.is_empty() {
        draft.fiction_themes = contract.themes.clone();
    }
    if !contract.characters.is_empty() {
        complete_minimum_character_slots(&mut contract.characters, draft);
        let character_lines = contract
            .characters
            .iter()
            .map(|character| character.to_draft_line())
            .collect::<Vec<_>>();
        let governed = if draft_has_character_authority(draft) {
            align_contract_characters_to_existing_draft_authority(draft, contract);
            contract_character_lines_with_existing_authority_sources(draft, &contract.characters)
        } else {
            character_lines.clone()
        };
        let governed = normalize_governed_character_lines(governed, draft);
        draft.fiction_characters = governed;
    }
    contract.normalize();
    if !contract.world_rules.is_empty() {
        draft.fiction_world_rules = contract.world_rules.clone();
    }
    if !contract.style_rules.is_empty() {
        draft.fiction_style_rules = contract.style_rules.clone();
    }
    if !contract.must_avoid.is_empty() {
        draft.fiction_must_avoid = contract.must_avoid.clone();
    }
    let visible_governance =
        patch::visible_governance_fields_from_contract_v2(&contract.structured);
    if draft.fiction_themes.is_empty() && !visible_governance.themes.is_empty() {
        draft.fiction_themes = visible_governance.themes;
    }
    if draft.fiction_style_rules.is_empty() && !visible_governance.style_rules.is_empty() {
        draft.fiction_style_rules = visible_governance.style_rules;
    }
    if draft.fiction_must_avoid.is_empty() && !visible_governance.must_avoid.is_empty() {
        draft.fiction_must_avoid = visible_governance.must_avoid;
    }
    let outline = strong_contract_outline_text(contract);
    if contract_ready {
        draft.fiction_outline = outline;
    } else {
        merge_missing_aware_string(&mut draft.fiction_outline, &outline);
    }
    contract.normalize();
    if contract_ready || novel_contract_v2_has_content(&contract.structured) {
        patch::canonicalize_contract_v2_to_character_lines(
            &mut contract.structured,
            &draft.fiction_characters,
        );
        draft.set_contract_v2(contract.structured.clone());
    }

    before != serde_json::to_value(&*draft).ok()
}

fn draft_has_character_authority(draft: &SessionCreationDraftState) -> bool {
    let forbidden_names = forbidden_naming_authority(draft)
        .character_names
        .into_iter()
        .collect::<BTreeSet<_>>();
    draft.fiction_characters.iter().any(|line| {
        let character = draft_character_line_to_contract(line);
        !value_missing(&character.canonical_name)
            && !forbidden_names.contains(character.canonical_name.trim())
            && !value_missing(&character.role)
            && fiction_contract_character_name_is_valid(&character.canonical_name)
            && character_line_has_locked_name_authority(draft, line)
    })
}

fn align_contract_characters_to_existing_draft_authority(
    draft: &SessionCreationDraftState,
    contract: &mut NovelCreationContract,
) {
    let forbidden_names = forbidden_naming_authority(draft)
        .character_names
        .into_iter()
        .collect::<BTreeSet<_>>();
    let existing = draft
        .fiction_characters
        .iter()
        .filter(|line| character_line_has_locked_name_authority(draft, line))
        .map(|line| draft_character_line_to_contract(line))
        .filter(|character| {
            !value_missing(&character.canonical_name)
                && !forbidden_names.contains(character.canonical_name.trim())
        })
        .collect::<Vec<_>>();
    if existing.is_empty() {
        return;
    }

    let mut aligned = Vec::new();
    let mut new_characters = Vec::new();
    let mut replacements = BTreeMap::new();
    for mut incoming in contract.characters.drain(..) {
        if let Some(known) = existing
            .iter()
            .find(|known| {
                !value_missing(&known.character_id)
                    && known.character_id.trim() == incoming.character_id.trim()
            })
            .or_else(|| {
                existing
                    .iter()
                    .find(|known| known.canonical_name.trim() == incoming.canonical_name.trim())
            })
            .or_else(|| {
                existing
                    .iter()
                    .find(|known| patch::character_contract_roles_match(known, &incoming))
            })
        {
            let incoming_name = incoming.canonical_name.trim().to_string();
            let replacement_is_valid = governed_character_name_replacement_is_valid(
                incoming_name.as_str(),
                known.canonical_name.trim(),
            );
            if replacement_is_valid {
                replacements.insert(incoming_name.clone(), known.canonical_name.clone());
            }
            if known.name_source.trim() == "generated_by_writing_tool_policy"
                && replacement_is_valid
                && !incoming
                    .previous_names
                    .iter()
                    .any(|name| name.trim() == incoming_name)
            {
                incoming.previous_names.push(incoming_name);
            }
            incoming.character_id = known.character_id.clone();
            incoming.canonical_name = known.canonical_name.clone();
            incoming.name_source = known.name_source.clone();
            fill_missing_contract_character_field(&mut incoming.role, &known.role);
            fill_missing_contract_character_field(&mut incoming.desire, &known.desire);
            fill_missing_contract_character_field(&mut incoming.fear, &known.fear);
            fill_missing_contract_character_field(&mut incoming.bottom_line, &known.bottom_line);
            fill_missing_contract_character_field(&mut incoming.arc_start, &known.arc_start);
            fill_missing_contract_character_field(&mut incoming.arc_end, &known.arc_end);
            fill_missing_contract_character_field(
                &mut incoming.planned_entry,
                &known.planned_entry,
            );
            fill_missing_contract_character_field(&mut incoming.planned_exit, &known.planned_exit);
            if !aligned.iter().any(|character: &CharacterContract| {
                character.canonical_name.trim() == incoming.canonical_name.trim()
            }) {
                aligned.push(incoming);
            }
        } else {
            new_characters.push(incoming);
        }
    }

    if !new_characters.is_empty() {
        let used_names = existing
            .iter()
            .map(|character| character.canonical_name.trim().to_string())
            .filter(|name| !value_missing(name))
            .collect::<BTreeSet<_>>();
        let governance = patch::govern_character_name_candidates(
            &mut new_characters,
            draft,
            used_names,
            "strong-contract-new-character-slot",
        );
        replacements.extend(governance.replacements().clone());
        governance.lock_authority(&mut new_characters);
        aligned.extend(new_characters);
    }

    for known in existing {
        if !aligned.iter().any(|character: &CharacterContract| {
            character.canonical_name.trim() == known.canonical_name.trim()
        }) {
            aligned.push(known);
        }
    }
    contract.characters = aligned;
    replace_contract_character_mentions(contract, &replacements);
}

fn contract_character_lines_with_existing_authority_sources(
    draft: &SessionCreationDraftState,
    characters: &[CharacterContract],
) -> Vec<String> {
    characters
        .iter()
        .map(|character| {
            let mut character = character.clone();
            character.name_source = draft
                .fiction_characters
                .iter()
                .find(|existing| {
                    draft_character_line_to_contract(existing)
                        .canonical_name
                        .trim()
                        == character.canonical_name.trim()
                })
                .and_then(|existing| character_line_name_source(existing))
                .or_else(|| {
                    (!character.name_source.trim().is_empty())
                        .then(|| character.name_source.trim().to_string())
                })
                .unwrap_or_default();
            character.to_draft_line()
        })
        .collect()
}

pub(crate) fn character_line_has_locked_name_authority(
    draft: &SessionCreationDraftState,
    line: &str,
) -> bool {
    let source = character_line_name_source(line).unwrap_or_default();
    matches!(source.trim(), "generated_by_writing_tool_policy" | "user")
        || (!draft.project_path.trim().is_empty() && source.trim() == "contract_authority")
}

fn fill_missing_contract_character_field(target: &mut String, fallback: &str) {
    let fallback = fallback.trim();
    if value_missing(target) && !value_missing(fallback) {
        *target = fallback.to_string();
    }
}

fn normalize_governed_character_lines(
    lines: Vec<String>,
    draft: &SessionCreationDraftState,
) -> Vec<String> {
    let mut characters = lines
        .iter()
        .map(|line| draft_character_line_to_contract(line))
        .collect::<Vec<_>>();
    complete_minimum_character_slots(&mut characters, draft);
    characters
        .into_iter()
        .map(|character| character.to_draft_line())
        .collect()
}

pub(crate) fn character_line_name_source(line: &str) -> Option<String> {
    for marker in ["name_source:", "name_source：", "source:", "source："] {
        let Some((_, tail)) = line.split_once(marker) else {
            continue;
        };
        let value = tail
            .trim()
            .split([';', '；', ',', '，', '\n', '\r'])
            .next()
            .unwrap_or(tail)
            .trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

pub(crate) fn novel_contract_v2_has_content(contract: &NovelContractV2) -> bool {
    !contract.resource_economy.resource_types.is_empty()
        || !contract.resource_economy.cost_examples.is_empty()
        || !contract.resource_economy.income_sources.is_empty()
        || !contract.resource_economy.scarcity_rules.is_empty()
        || !contract.resource_economy.trade_rules.is_empty()
        || !value_missing(&contract.resource_economy.currency)
        || !value_missing(&contract.resource_economy.value_scale)
        || !value_missing(&contract.resource_economy.class_impact)
        || !value_missing(&contract.emotional_contract.primary_emotion)
        || !value_missing(&contract.emotional_contract.emotional_promise)
        || !contract.emotional_contract.emotional_beats.is_empty()
        || !contract.emotional_contract.relief_beats.is_empty()
        || !contract.emotional_contract.payoff_requirements.is_empty()
        || !value_missing(&contract.emotional_contract.ending_emotional_state)
        || !contract.emotional_state_ledger.is_empty()
        || !contract.relationship_ledger.is_empty()
        || !contract.power_progression.levels.is_empty()
        || !contract.power_progression.advancement_costs.is_empty()
        || !contract.power_progression.bottlenecks.is_empty()
        || !contract.power_progression.failure_consequences.is_empty()
        || !contract.power_progression.anti_power_creep_rules.is_empty()
        || !contract
            .power_progression
            .character_current_levels
            .is_empty()
        || !value_missing(&contract.power_progression.system_name)
        || !contract.social_order.institutions.is_empty()
        || !contract.social_order.exam_or_promotion_rules.is_empty()
        || !contract.social_order.laws.is_empty()
        || !contract.social_order.authority_conflicts.is_empty()
        || !value_missing(&contract.social_order.rank_system)
        || !value_missing(&contract.social_order.class_structure)
        || !contract.geography_model.regions.is_empty()
        || !contract.geography_model.important_locations.is_empty()
        || !contract.geography_model.distance_rules.is_empty()
        || !contract.geography_model.travel_constraints.is_empty()
        || !contract.geography_model.location_changes.is_empty()
        || !contract.time_model.deadline_events.is_empty()
        || !contract.time_model.age_progression.is_empty()
        || !contract.time_model.time_skip_rules.is_empty()
        || !value_missing(&contract.time_model.calendar)
        || !value_missing(&contract.time_model.story_start_time)
        || !value_missing(&contract.time_model.elapsed_time)
        || !contract.artifact_ledger.is_empty()
        || !contract.antagonist_pressure.antagonists.is_empty()
        || !value_missing(&contract.antagonist_pressure.primary_pressure)
        || !contract.payoff_matrix.is_empty()
        || !value_missing(&contract.narration_contract.pov)
        || !value_missing(&contract.narration_contract.tense)
        || !value_missing(&contract.narration_contract.narrative_distance)
        || !value_missing(&contract.narration_contract.dialogue_style)
        || !value_missing(&contract.narration_contract.description_density)
        || !value_missing(&contract.narration_contract.chapter_pacing)
        || !contract.narration_contract.forbidden_style_drift.is_empty()
        || !value_missing(&contract.scene_type_mix.action)
        || !value_missing(&contract.scene_type_mix.dialogue)
        || !value_missing(&contract.scene_type_mix.everyday)
        || !value_missing(&contract.scene_type_mix.reveal)
        || !value_missing(&contract.scene_type_mix.emotional)
        || !value_missing(&contract.scene_type_mix.turning_point)
        || !value_missing(&contract.scene_type_mix.balance_rule)
        || !contract.character_voice_ledger.is_empty()
        || !value_missing(&contract.reader_promise.core_hook)
        || !contract.reader_promise.pleasure_points.is_empty()
        || !value_missing(&contract.reader_promise.curiosity_engine)
        || !value_missing(&contract.reader_promise.payoff_style)
        || !contract.chapter_ending_rotation.planned_rotation.is_empty()
        || !value_missing(&contract.chapter_ending_rotation.avoid_repetition_rule)
        || !contract.conflict_pressure_curve.global_curve.is_empty()
        || !value_missing(&contract.conflict_pressure_curve.release_strategy)
        || !value_missing(&contract.conflict_pressure_curve.peak_policy)
        || !contract.motif_ledger.is_empty()
        || !contract.reveal_schedule.is_empty()
        || !contract.relationship_interaction_quotas.is_empty()
}

pub(crate) fn merge_missing_aware_string(target: &mut String, incoming: &str) {
    let incoming = incoming.trim();
    if value_missing(incoming) {
        return;
    }
    if value_missing(target) || target.trim() != incoming {
        *target = incoming.to_string();
    }
}

pub(crate) fn replace_contract_generated_brief(target: &mut String, incoming: &str) {
    let incoming = sanitize_creation_brief_value(incoming);
    if value_missing(&incoming) {
        return;
    }
    *target = incoming;
}

pub(crate) fn strong_contract_outline_text(contract: &NovelCreationContract) -> String {
    let mut lines = Vec::new();
    if !contract.outline.raw_outline.trim().is_empty() {
        lines.push(contract.outline.raw_outline.trim().to_string());
    }
    for (index, volume) in contract.outline.volumes.iter().enumerate() {
        let title = volume.title.trim();
        let objective = volume.objective.trim();
        let ending_change = volume.ending_change.trim();
        let mut line = format!("第{}卷", index + 1);
        if !title.is_empty() {
            line.push_str(&format!("《{title}》"));
        }
        if !objective.is_empty() {
            line.push_str(&format!("：{objective}"));
        }
        if !ending_change.is_empty() {
            line.push_str(&format!("；卷尾变化：{ending_change}"));
        }
        lines.push(line);
    }
    for chapter in &contract.outline.near_chapters {
        let number = chapter.number.unwrap_or_else(|| lines.len() + 1);
        let goal = chapter.goal.trim();
        let turn = chapter.expected_turn.trim();
        if goal.is_empty() && turn.is_empty() {
            continue;
        }
        let mut line = format!("第{number}章");
        if !goal.is_empty() {
            line.push_str(&format!(" 本章目标：{goal}"));
        }
        if !turn.is_empty() {
            line.push_str(&format!("；预期转折：{turn}"));
        }
        lines.push(line);
    }
    dedup_compact_contract_values(lines, 80, 1200).join("\n")
}

pub(crate) fn strong_contract_outline_summary_text(contract: &NovelCreationContract) -> String {
    let raw = contract.outline.raw_outline.trim();
    if !raw.is_empty() {
        return raw.to_string();
    }
    let basis = contract.story_basis_text();
    if !basis.trim().is_empty() {
        return compact_creation_text(&basis, 520);
    }
    "围绕本次小说合同推进全书主线、人物弧线和终局兑现。".to_string()
}

pub(crate) fn normalize_fiction_creation_draft_after_contract_change(
    draft: &mut SessionCreationDraftState,
) {
    if draft.artifact_kind != "fiction" {
        return;
    }
    let authority_changed = prune_explicit_non_character_draft_authority(draft);
    reconcile_title_from_model_rationale(draft);
    preserve_explicit_user_primary_role_authority(draft);
    canonicalize_fiction_contract_v2_to_current_character_authority(draft);
    if authority_changed {
        rebuild_current_contract_from_visible_draft(draft);
    }
}

fn preserve_explicit_user_primary_role_authority(draft: &mut SessionCreationDraftState) -> bool {
    let Some(expected_role) = explicit_gendered_primary_role_from_user_story_authority(draft)
    else {
        return false;
    };
    let Some(primary_index) = draft
        .fiction_characters
        .iter()
        .position(|line| draft_character_line_role_looks_primary(line))
    else {
        return false;
    };
    let mut primary = draft_character_line_to_contract(&draft.fiction_characters[primary_index]);
    if primary.role == expected_role {
        return false;
    }
    primary.role = expected_role.to_string();
    draft.fiction_characters[primary_index] = primary.to_draft_line();
    true
}

fn explicit_gendered_primary_role_from_user_story_authority(
    draft: &SessionCreationDraftState,
) -> Option<&'static str> {
    draft
        .planning_notes
        .iter()
        .filter_map(|note| note.strip_prefix("用户故事核心权威："))
        .filter_map(|authority| {
            [
                ("女主人公", "女主"),
                ("男主人公", "男主"),
                ("女主", "女主"),
                ("男主", "男主"),
            ]
            .into_iter()
            .filter_map(|(marker, role)| authority.find(marker).map(|index| (index, role)))
            .min_by_key(|(index, _)| *index)
            .map(|(_, role)| role)
        })
        .last()
}

fn prune_explicit_non_character_draft_authority(draft: &mut SessionCreationDraftState) -> bool {
    let before = draft.fiction_characters.len();
    draft.fiction_characters.retain(|line| {
        let character = draft_character_line_to_contract(line);
        !planning_notes_explicitly_exclude_character(
            &draft.planning_notes,
            &character.canonical_name,
        )
    });
    draft.fiction_characters.len() != before
}

fn prune_explicit_non_character_contract_characters(
    draft: &SessionCreationDraftState,
    contract: &mut NovelCreationContract,
) -> bool {
    let before = contract.characters.len();
    contract.characters.retain(|character| {
        let name = character.canonical_name.trim();
        !planning_notes_explicitly_exclude_character(&draft.planning_notes, name)
            && (!model_candidate_name_explicitly_collective(name)
                || super::patch::draft_explicitly_names_character(draft, name))
    });
    contract.characters.len() != before
}

fn model_candidate_name_explicitly_collective(name: &str) -> bool {
    name.chars().count() >= 3
        && typed_contract_gate::reference_looks_like_collective_or_organization(name)
}

pub(super) fn planning_notes_explicitly_exclude_character(notes: &[String], name: &str) -> bool {
    let names = non_character_constraint_name_aliases(name);
    if names.is_empty() {
        return false;
    }
    notes.iter().any(|note| {
        let note = compact_non_character_constraint_surface(note);
        names
            .iter()
            .any(|name| note_explicitly_classifies_name_as_non_character(&note, name))
    })
}

fn note_explicitly_classifies_name_as_non_character(note: &str, name: &str) -> bool {
    const NEGATIONS: &[&str] = &[
        "不是人物",
        "并非人物",
        "不是角色",
        "并非角色",
        "不作为人物",
        "不作为角色",
        "不属于人物",
        "不属于角色",
        "isnotacharacter",
        "isnotaperson",
        "isntacharacter",
        "isntaperson",
    ];
    let mut rest = note;
    while let Some(index) = rest.find(name) {
        let after = &rest[index + name.len()..];
        if NEGATIONS.iter().any(|negation| {
            after.find(negation).is_some_and(|offset| {
                // Keep the assertion bound to the named subject. This accepts natural
                // classifications such as "X is a protocol, explicitly not a character"
                // without treating a later sentence about another subject as evidence.
                after[..offset].chars().count() <= 48
            })
        }) {
            return true;
        }
        rest = &after[after
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .unwrap_or(after.len())..];
    }
    false
}

fn non_character_constraint_name_aliases(value: &str) -> Vec<String> {
    let name = compact_non_character_constraint_surface(value);
    if name.is_empty() {
        return Vec::new();
    }
    let mut aliases = vec![name.clone()];
    for suffix in [
        "协议", "系统", "算法", "程序", "项目", "计划", "机构", "组织", "公司", "集团", "平台",
        "装置", "设备", "代号", "编号",
    ] {
        if let Some(base) = name.strip_suffix(suffix) {
            if !base.is_empty() && !aliases.iter().any(|alias| alias == base) {
                aliases.push(base.to_string());
            }
        }
    }
    let mut ascii_identifier = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            ascii_identifier.push(ch);
        } else if ascii_identifier.len() >= 2 {
            break;
        } else {
            ascii_identifier.clear();
        }
    }
    if ascii_identifier.len() >= 2 && !aliases.iter().any(|alias| alias == &ascii_identifier) {
        aliases.push(ascii_identifier);
    }
    aliases
}

fn compact_non_character_constraint_surface(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            !ch.is_whitespace()
                && !matches!(
                    ch,
                    '(' | ')'
                        | '（'
                        | '）'
                        | '['
                        | ']'
                        | '【'
                        | '】'
                        | '“'
                        | '”'
                        | '‘'
                        | '’'
                        | '\''
                        | '"'
                        | '，'
                        | ','
                        | '。'
                        | '；'
                        | ';'
                        | '：'
                        | ':'
                )
        })
        .collect::<String>()
        .to_ascii_lowercase()
}

fn canonicalize_fiction_contract_v2_to_current_character_authority(
    draft: &mut SessionCreationDraftState,
) {
    if draft.fiction_characters.is_empty() {
        return;
    }
    let character_lines = draft.fiction_characters.clone();
    let mut contract = draft.contract_v2();
    patch::canonicalize_contract_v2_to_character_lines(&mut contract, &character_lines);
    draft.set_contract_v2(contract);
    patch::canonicalize_draft_story_surfaces_to_character_lines(draft, &character_lines);
}

fn reconcile_title_from_model_rationale(draft: &mut SessionCreationDraftState) -> bool {
    if !value_missing(&draft.title) || value_missing(&draft.fiction_title_rationale) {
        return false;
    }
    let quoted = quoted_book_title_like_segments(&draft.fiction_title_rationale);
    let [title] = quoted.as_slice() else {
        return false;
    };
    if !naming::title_rationale_is_concrete(&draft.fiction_title_rationale, title) {
        return false;
    }
    draft.title = title.clone();
    true
}

#[cfg(test)]
pub(crate) fn repair_creation_draft_title_metadata(draft: &mut SessionCreationDraftState) -> bool {
    if draft.artifact_kind != "fiction" || !creation_draft_title_needs_metadata_repair(draft) {
        return false;
    }
    let previous_title = draft.title.trim().to_string();
    if !previous_title.is_empty() {
        draft.diagnostics = merge_list(
            &draft.diagnostics,
            &[format!(
                "书名《{previous_title}》未通过合同命名质量门；请根据终局方向、大纲、世界观意象和主角弧线重新生成书名，不要由工具本地拼词。"
            )],
        );
    } else {
        draft.diagnostics = merge_list(
            &draft.diagnostics,
            &[
                "书名尚未锁定；请根据终局方向、大纲、世界观意象和主角弧线生成正式书名。"
                    .to_string(),
            ],
        );
    }
    draft.title.clear();
    draft.fiction_title_rationale.clear();
    true
}

#[cfg(test)]
pub(crate) fn creation_draft_title_needs_metadata_repair(
    draft: &SessionCreationDraftState,
) -> bool {
    let title = draft.title.trim();
    let evidence = creation_draft_title_story_evidence(draft);
    let title_decision = naming::select_book_title_candidate_decision(
        [naming::BookTitleCandidate::new(
            title,
            draft.fiction_title_rationale.as_str(),
        )],
        &naming::BookTitleEvidence::new("书名", &evidence),
    );
    title.is_empty()
        || fiction_title_is_temporary_placeholder(title)
        || title_surface_is_meta_discussion(title)
        || !title_decision.accepted
}

pub(crate) fn non_empty_list_from_value(value: &str) -> Vec<String> {
    let value = value.trim();
    if value.is_empty() {
        Vec::new()
    } else {
        vec![value.to_string()]
    }
}

pub(crate) fn align_fiction_contract_text_to_governed_characters(
    draft: &mut SessionCreationDraftState,
    original_characters: &[String],
    governed_characters: &[String],
) {
    for (original, governed) in original_characters.iter().zip(governed_characters.iter()) {
        let Some(old_name) = character_name_from_contract_line(original) else {
            continue;
        };
        let Some(new_name) = character_name_from_contract_line(governed) else {
            continue;
        };
        if !governed_character_name_replacement_is_valid(&old_name, &new_name) {
            continue;
        }
        let mut contract = draft.contract_v2();
        let Ok(mut value) = serde_json::to_value(&contract) else {
            continue;
        };
        replace_json_contract_character_mentions(&mut value, &old_name, &new_name);
        if let Ok(updated) = serde_json::from_value::<NovelContractV2>(value) {
            contract = updated;
            draft.set_contract_v2(contract);
        }
    }
}

pub(crate) fn governed_character_name_replacement_is_valid(old_name: &str, new_name: &str) -> bool {
    old_name != new_name
        && fiction_contract_character_name_is_replaceable_source(old_name)
        && fiction_contract_character_name_is_valid(new_name)
}

fn replace_contract_character_mentions(
    contract: &mut NovelCreationContract,
    replacements: &BTreeMap<String, String>,
) {
    patch::rewrite_novel_contract_names(contract, replacements);
}

fn replace_json_contract_character_mentions(value: &mut Value, old_name: &str, new_name: &str) {
    if !governed_source_name_is_unambiguous(old_name) {
        return;
    }
    match value {
        Value::String(text) => {
            if text.contains(old_name) {
                *text = text.replace(old_name, new_name);
            }
        }
        Value::Array(items) => {
            for item in items {
                replace_json_contract_character_mentions(item, old_name, new_name);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                if matches!(
                    key.as_str(),
                    "canonical_name"
                        | "character_id"
                        | "name_source"
                        | "aliases"
                        | "previous_names"
                ) {
                    continue;
                }
                replace_json_contract_character_mentions(item, old_name, new_name);
            }
        }
        _ => {}
    }
}

fn governed_source_name_is_unambiguous(name: &str) -> bool {
    if name.chars().count() < 3 {
        return false;
    }
    let language = if name
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
    {
        "zh-CN"
    } else {
        "en"
    };
    naming::audit_character_name_candidate(name, language).accepted
}

pub(crate) fn sanitize_creation_draft_control_noise(draft: &mut SessionCreationDraftState) {
    draft.genre = sanitize_creation_genre_value(&draft.genre);
    draft.brief = sanitize_creation_brief_value(&draft.brief);
    draft.fiction_premise = sanitize_creation_brief_value(&draft.fiction_premise);
    draft.thesis_or_premise = sanitize_creation_brief_value(&draft.thesis_or_premise);
    draft.fiction_themes =
        sanitize_creation_contract_list(std::mem::take(&mut draft.fiction_themes));
    draft.fiction_world_rules =
        sanitize_creation_contract_list(std::mem::take(&mut draft.fiction_world_rules));
    draft.fiction_style_rules =
        sanitize_creation_contract_list(std::mem::take(&mut draft.fiction_style_rules));
    draft.fiction_must_avoid =
        sanitize_creation_contract_list(std::mem::take(&mut draft.fiction_must_avoid));
    draft.fiction_outline = sanitize_creation_outline_value(&draft.fiction_outline);
    draft.planning_notes = draft
        .planning_notes
        .iter()
        .filter_map(|item| {
            let sanitized = if let Some(authority) = item.strip_prefix("用户故事核心权威：")
            {
                let authority = sanitize_creation_brief_value(authority);
                if authority.trim().is_empty() {
                    return None;
                }
                format!("用户故事核心权威：{}", authority.trim())
            } else if let Some(revision) =
                item.strip_prefix(super::draft_lifecycle::PENDING_EXPLICIT_CONTRACT_REVISION_PREFIX)
            {
                let (patch_type, revision) =
                    super::draft_lifecycle::parse_pending_explicit_contract_revision(revision)?;
                let revision = sanitize_creation_brief_value(revision);
                if revision.trim().is_empty() {
                    return None;
                }
                super::draft_lifecycle::pending_explicit_contract_revision_note(
                    patch_type,
                    revision.trim(),
                )
            } else if item.starts_with(CREATION_EXECUTION_SCOPE_NOTE_PREFIX) {
                item.trim().to_string()
            } else if creation_planning_note_is_quality_feedback(item) {
                item.trim().to_string()
            } else {
                sanitize_creation_brief_value(item)
            };
            (!sanitized.trim().is_empty()).then_some(sanitized)
        })
        .collect();
}

pub(crate) fn sanitize_creation_contract_list(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        // These values are already typed story-contract fields.  The brief
        // sanitizer also removes user workflow and length directives, so it
        // can corrupt valid rules such as "每章必须……".  Preserve the
        // typed value and apply only the contract control-noise filter below.
        let item = value.trim().to_string();
        if item.trim().is_empty() || creation_contract_list_item_is_control_noise(&item) {
            continue;
        }
        if !out.iter().any(|existing| existing == &item) {
            out.push(item);
        }
    }
    out
}

pub(crate) fn sanitize_creation_outline_value(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !creation_contract_list_item_is_control_noise(line))
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn creation_contract_list_item_is_control_noise(value: &str) -> bool {
    let text = value.trim();
    if text.is_empty() || surface_sanitizer::line_is_assistant_surface_noise(text) {
        return true;
    }
    surface_sanitizer::contains_legal_contract_residue(text)
}

#[cfg(test)]
pub(crate) fn creation_draft_approval_readiness_issues(
    draft: &SessionCreationDraftState,
) -> Vec<String> {
    let mut issues = Vec::new();
    if draft.artifact_kind != "fiction" {
        return issues;
    }
    let strong_contract = strong_novel_contract_from_creation_draft(draft);
    let strong_report = strong_contract.validate();
    if !strong_report.is_ready() {
        issues.extend(strong_report.issues.messages());
    }

    issues.extend(creation_draft_visible_approval_readiness_issues(draft));

    issues.sort();
    issues.dedup();
    issues
}

pub(crate) fn creation_draft_visible_approval_readiness_issues(
    draft: &SessionCreationDraftState,
) -> Vec<String> {
    let mut issues = Vec::new();
    if draft.artifact_kind != "fiction" {
        return issues;
    }

    if draft.title.trim().is_empty() {
        issues.push("小说合同尚未形成可锁定书名".to_string());
    } else if fiction_title_is_temporary_placeholder(&draft.title) {
        issues.push("小说合同书名仍是内部临时占位，必须由 LLM 根据合同生成正式书名".to_string());
    } else if let Some(issue) = naming::title_contract_basis_issue(
        &draft.title,
        "书名",
        &draft.fiction_title_rationale,
        &creation_draft_title_story_evidence(draft),
    ) {
        issues.push(issue);
    }

    if !draft
        .fiction_characters
        .iter()
        .any(|line| draft_character_line_role_looks_primary(line))
    {
        issues.push("小说合同尚未形成主角权威锚点".to_string());
    }
    if !draft.fiction_characters.iter().any(|line| {
        !draft_character_line_role_looks_primary(line)
            && character_name_from_contract_line(line)
                .as_deref()
                .is_some_and(fiction_contract_character_name_is_valid)
    }) {
        issues.push(
            "小说合同角色权威表缺少非主角关键角色、关系对象或对手，不能支撑冲突和关系线"
                .to_string(),
        );
    }
    if draft
        .fiction_characters
        .iter()
        .map(|line| draft_character_line_to_contract(line))
        .filter(CharacterContract::role_looks_primary)
        .count()
        > 1
    {
        issues.push(
            "小说合同角色权威表包含多个主角槽位，必须先收敛为一个主角或由用户明确要求多主角"
                .to_string(),
        );
    }

    if draft
        .fiction_characters
        .iter()
        .any(|line| fiction_character_line_has_placeholder_name(line))
    {
        issues.push("小说合同角色名仍是临时占位，必须在合同草案中生成稳定角色名".to_string());
    }

    if value_missing(&draft.fiction_ending_direction) {
        issues.push("小说合同缺少终局方向，书名无法从结局倒推".to_string());
    }
    if value_missing(&draft.fiction_protagonist_arc) {
        issues.push("小说合同缺少主角弧线，书名和长期剧情锚点不足".to_string());
    }
    if value_missing(&draft.fiction_world_imagery) {
        issues.push("小说合同缺少世界观意象，书名无法和题材气质稳定绑定".to_string());
    }
    if value_missing(&draft.fiction_main_causal_spine) {
        issues.push("小说合同缺少总主线因果链，长篇推进容易漂移".to_string());
    }
    if value_missing(&draft.fiction_title_rationale) {
        issues.push("小说合同缺少命名理由，无法验证书名是否来自剧情和结局".to_string());
    }
    let outline = draft.fiction_outline.trim();
    let typed_outline = strong_novel_contract_from_creation_draft(draft).outline;
    if outline.is_empty() {
        issues.push("小说合同尚未形成逐章规划或分卷/阶段大纲".to_string());
    } else if !typed_outline.has_stage_or_near_chapter_plan()
        && !fiction_outline_has_stage_or_recent_chapter_plan(outline)
    {
        issues.push("小说合同尚未形成分卷/阶段安排或近期章节包".to_string());
    }

    issues.sort();
    issues.dedup();
    issues
}

pub(crate) fn creation_draft_title_story_evidence(draft: &SessionCreationDraftState) -> String {
    [
        draft.genre.as_str(),
        draft.brief.as_str(),
        draft.fiction_premise.as_str(),
        draft.fiction_ending_direction.as_str(),
        draft.fiction_protagonist_arc.as_str(),
        draft.fiction_world_imagery.as_str(),
        draft.fiction_main_causal_spine.as_str(),
        draft.fiction_outline.as_str(),
    ]
    .into_iter()
    .filter(|value| !value.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

pub(crate) fn strong_novel_contract_from_creation_draft(
    draft: &SessionCreationDraftState,
) -> NovelCreationContract {
    if let Some(mut contract) = draft
        .current_contract
        .as_ref()
        .and_then(|value| serde_json::to_string(value).ok())
        .and_then(|raw| NovelCreationContract::parse_json_boundary(&raw))
    {
        prune_explicit_non_character_contract_characters(draft, &mut contract);
        contract.normalize();
        if !contract.characters.is_empty()
            || !value_missing(&contract.title.canonical_title)
            || !value_missing(&contract.premise)
            || !value_missing(&contract.main_causal_spine)
        {
            return contract;
        }
    }

    strong_novel_contract_from_visible_creation_draft(draft)
}

pub(crate) fn strong_novel_contract_from_visible_creation_draft(
    draft: &SessionCreationDraftState,
) -> NovelCreationContract {
    let ending = EndingContract {
        desired_resolution: draft.fiction_ending_direction.clone(),
        final_state: String::new(),
        must_resolve: non_empty_list_from_value(&draft.fiction_main_causal_spine),
        allowed_open_questions: Vec::new(),
    };
    let mut characters = draft
        .fiction_characters
        .iter()
        .map(|line| draft_character_line_to_contract(line))
        .collect::<Vec<_>>();
    apply_project_arc_to_primary_character(
        &mut characters,
        &draft.fiction_protagonist_arc,
        &draft.fiction_ending_direction,
        &draft.fiction_main_causal_spine,
        &draft.fiction_must_avoid,
    );
    normalize_draft_primary_character_slots(&mut characters);
    let derived_outline =
        patch_normalizer::derive_plot_contract_from_outline_text(&draft.fiction_outline);
    let derived_outline_has_typed_segments = !derived_outline.volumes.is_empty()
        || !derived_outline.near_chapters.is_empty()
        || !derived_outline.payoff_matrix.is_empty();
    let near_chapters = if derived_outline.near_chapters.is_empty() {
        collect_explicit_chapter_plan_titles(&draft.fiction_outline)
            .into_iter()
            .enumerate()
            .map(|(index, goal)| ChapterSeedContract {
                number: Some(index + 1),
                goal,
                expected_turn: String::new(),
            })
            .collect()
    } else {
        derived_outline.near_chapters
    };
    let raw_outline = if !derived_outline_has_typed_segments {
        draft.fiction_outline.clone()
    } else {
        patch_normalizer::strip_plot_control_segments_from_outline_text(&draft.fiction_outline)
    };
    let outline = OutlineContract {
        raw_outline,
        volumes: derived_outline.volumes,
        near_chapters,
        ..Default::default()
    };
    let mut structured = draft.contract_v2();
    if structured.payoff_matrix.is_empty() && !derived_outline.payoff_matrix.is_empty() {
        structured.payoff_matrix = derived_outline.payoff_matrix;
    }
    complete_minimum_character_slots(&mut characters, draft);
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: draft.title.clone(),
            rationale: draft.fiction_title_rationale.clone(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: draft.language.clone(),
        genre: draft.genre.clone(),
        brief: draft.brief.clone(),
        target_units: draft.target_units,
        chapter_unit_target: draft.chapter_unit_target,
        max_chapters_per_turn: draft.max_chapters_per_turn,
        premise: draft.fiction_premise.clone(),
        ending,
        protagonist_arc: draft.fiction_protagonist_arc.clone(),
        world_imagery: draft.fiction_world_imagery.clone(),
        main_causal_spine: draft.fiction_main_causal_spine.clone(),
        characters,
        themes: draft.fiction_themes.clone(),
        world_rules: draft.fiction_world_rules.clone(),
        style_rules: draft.fiction_style_rules.clone(),
        must_avoid: draft.fiction_must_avoid.clone(),
        outline,
        structured,
    };
    let _ = sanitize_structured_world_rules_seed(&mut contract);
    contract.normalize();
    contract
}

pub(crate) fn complete_minimum_character_slots(
    characters: &mut [CharacterContract],
    draft: &SessionCreationDraftState,
) {
    let has_primary = characters.iter().any(CharacterContract::role_looks_primary);
    for (index, character) in characters.iter_mut().enumerate() {
        if value_missing(&character.role) || character_role_is_generic_placeholder(&character.role)
        {
            character.role = inferred_character_role(character, index, has_primary);
        }
        if typed_contract_gate::character_anchor_uses_generic_placeholder(&character.desire) {
            character.desire.clear();
        }
        if typed_contract_gate::character_anchor_looks_like_storyline_or_truncated_surface(
            &character.desire,
        ) {
            character.desire.clear();
        }
        if typed_contract_gate::character_anchor_uses_generic_placeholder(&character.fear) {
            character.fear.clear();
        }
        if typed_contract_gate::character_anchor_looks_like_storyline_or_truncated_surface(
            &character.fear,
        ) {
            character.fear.clear();
        }
        if typed_contract_gate::character_anchor_uses_generic_placeholder(&character.bottom_line) {
            character.bottom_line.clear();
        }
        if typed_contract_gate::character_anchor_looks_like_storyline_or_truncated_surface(
            &character.bottom_line,
        ) {
            character.bottom_line.clear();
        }
        if character.role_looks_primary() {
            if character.arc_start.trim().is_empty() {
                character.arc_start = arc_start_from_project_arc(&draft.fiction_protagonist_arc);
            }
            if character.arc_end.trim().is_empty() {
                character.arc_end = arc_end_from_project_arc(
                    &draft.fiction_protagonist_arc,
                    &draft.fiction_ending_direction,
                );
            }
            if value_missing(&character.arc_start) {
                character.arc_start.clear();
            }
            if value_missing(&character.arc_end) {
                character.arc_end.clear();
            }
        }
    }
}

fn character_role_is_generic_placeholder(role: &str) -> bool {
    let compact = role.replace(char::is_whitespace, "");
    matches!(
        compact.as_str(),
        "角色" | "人物" | "关键角色" | "主要角色" | "配角" | "重要角色"
    )
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

pub(crate) fn draft_character_line_to_contract(line: &str) -> CharacterContract {
    let lowered = line.to_ascii_lowercase();
    let name = character_name_from_contract_line(line).unwrap_or_default();
    let explicit_role =
        contract_line_detail_value(line, &["role", "角色", "角色定位", "身份", "定位"]);
    let role_basis = if explicit_role.trim().is_empty() {
        line
    } else {
        explicit_role.as_str()
    };
    let role = draft_character_role_from_basis(role_basis, &lowered);
    CharacterContract {
        character_id: contract_line_detail_value(line, &["character_id", "character id", "角色ID"]),
        canonical_name: name,
        name_source: character_line_name_source(line).unwrap_or_default(),
        aliases: character_identity_values_from_contract_line(line, &["aliases", "别名"]),
        previous_names: character_identity_values_from_contract_line(
            line,
            &["previous_names", "previous names", "历史姓名", "旧名"],
        ),
        role,
        desire: contract_line_detail_value(line, &["desire", "欲望"]),
        fear: contract_line_detail_value(line, &["fear", "恐惧"]),
        bottom_line: contract_line_detail_value(line, &["bottom_line", "bottom line", "底线"]),
        arc_start: contract_line_detail_value(line, &["arc_start", "弧线起点", "起点"]),
        arc_end: contract_line_detail_value(line, &["arc_end", "弧线终点", "终点"]),
        planned_entry: contract_line_detail_value(
            line,
            &["planned_entry", "planned entry", "计划登场"],
        ),
        planned_exit: contract_line_detail_value(
            line,
            &["planned_exit", "planned exit", "计划离场"],
        ),
        ..Default::default()
    }
}

fn character_identity_values_from_contract_line(line: &str, keys: &[&str]) -> Vec<String> {
    contract_line_detail_value(line, keys)
        .split('|')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn draft_character_line_role_looks_primary(line: &str) -> bool {
    draft_character_line_to_contract(line).role_looks_primary()
}

pub(crate) fn draft_character_role_from_basis(
    role_basis: &str,
    fallback_lowered_line: &str,
) -> String {
    let lowered_role = role_basis.to_ascii_lowercase();
    if role_basis.contains("女主") || role_basis.contains("女主人公") {
        "女主"
    } else if role_basis.contains("男主") || role_basis.contains("男主人公") {
        "男主"
    } else if role_basis.contains("主角")
        || role_basis.contains("主人公")
        || lowered_role.contains("protagonist")
    {
        "主角"
    } else if role_basis.contains("反派")
        || role_basis.contains("对手")
        || lowered_role.contains("antagonist")
    {
        "对手"
    } else if role_basis.contains("导师") || lowered_role.contains("mentor") {
        "导师"
    } else if role_basis.contains("关键关系")
        || role_basis.contains("关系对象")
        || lowered_role.contains("loveinterest")
        || lowered_role.contains("romanticlead")
        || lowered_role.contains("relationship")
    {
        "关键关系对象"
    } else if role_basis.contains("同伴")
        || role_basis.contains("盟友")
        || lowered_role.contains("companion")
        || lowered_role.contains("ally")
    {
        "同伴"
    } else if fallback_lowered_line.contains("protagonist") {
        "主角"
    } else if fallback_lowered_line.contains("antagonist") {
        "对手"
    } else {
        "角色"
    }
    .to_string()
}

pub(crate) fn apply_project_arc_to_primary_character(
    characters: &mut [CharacterContract],
    protagonist_arc: &str,
    ending_direction: &str,
    main_causal_spine: &str,
    must_avoid: &[String],
) {
    let Some(primary) = characters
        .iter_mut()
        .find(|character| character.role_looks_primary())
    else {
        return;
    };
    let arc = protagonist_arc.trim();
    if arc.is_empty() {
        return;
    }
    if primary.arc_start.trim().is_empty() {
        primary.arc_start = arc_start_from_project_arc(arc);
    }
    if primary.arc_end.trim().is_empty() {
        primary.arc_end = arc_end_from_project_arc(arc, ending_direction);
    }
    let _ = (main_causal_spine, must_avoid);
    if typed_contract_gate::character_anchor_uses_generic_placeholder(&primary.desire) {
        primary.desire.clear();
    }
    if typed_contract_gate::character_anchor_uses_generic_placeholder(&primary.fear) {
        primary.fear.clear();
    }
    if typed_contract_gate::character_anchor_uses_generic_placeholder(&primary.bottom_line) {
        primary.bottom_line.clear();
    }
}

pub(crate) fn normalize_draft_primary_character_slots(characters: &mut [CharacterContract]) {
    let Some(primary_index) = characters
        .iter()
        .position(|character| character.role_looks_primary())
    else {
        return;
    };
    let primary_name = characters[primary_index].canonical_name.clone();
    for (index, character) in characters.iter_mut().enumerate() {
        if index == primary_index || !character.role_looks_primary() {
            continue;
        }
        if character.canonical_name == primary_name {
            character.role = inferred_character_role(character, index, true);
        }
    }
}

pub(crate) fn arc_start_from_project_arc(arc: &str) -> String {
    project_arc_parts(arc, "").0
}

pub(crate) fn arc_end_from_project_arc(arc: &str, ending_direction: &str) -> String {
    project_arc_parts(arc, ending_direction).1
}

pub(crate) fn project_arc_parts(arc: &str, ending_direction: &str) -> (String, String) {
    let arc = arc.trim();
    if arc.is_empty() {
        return (String::new(), ending_direction.trim().to_string());
    }
    for marker in ["成长为", "转变为", "变成", "成为"] {
        if let Some((head, tail)) = arc.split_once(marker) {
            return (
                normalize_project_arc_start_endpoint(head),
                normalize_project_arc_endpoint(tail),
            );
        }
    }
    if let Some((head, tail)) = arc
        .strip_prefix('从')
        .and_then(|rest| rest.split_once('到'))
    {
        return (
            normalize_project_arc_start_endpoint(head),
            normalize_project_arc_endpoint(tail),
        );
    }
    let ending = ending_direction.trim();
    (
        normalize_project_arc_start_endpoint(arc),
        if ending.is_empty() {
            normalize_project_arc_endpoint(arc)
        } else {
            ending.to_string()
        },
    )
}

fn normalize_project_arc_start_endpoint(value: &str) -> String {
    let head = value
        .split(['，', ',', '；', ';', '。', '.', '\n'])
        .next()
        .unwrap_or(value);
    normalize_project_arc_endpoint(head)
}

fn normalize_project_arc_endpoint(value: &str) -> String {
    value
        .trim_start_matches('从')
        .trim_start_matches(['，', ',', '；', ';', ' '])
        .trim_end_matches(['，', ',', '；', ';', '。', '.', ' '])
        .trim()
        .to_string()
}

pub(crate) fn contract_line_detail_value(line: &str, labels: &[&str]) -> String {
    let parts = if line.contains(['；', ';']) {
        line.split(['；', ';']).collect::<Vec<_>>()
    } else {
        line.split(['，', ',']).collect::<Vec<_>>()
    };
    for part in parts {
        let part = part
            .trim()
            .trim_start_matches(['-', '*', '•', '·', ' '])
            .trim();
        for label in labels {
            let Some(value) = part.strip_prefix(label) else {
                continue;
            };
            if !value.is_empty() && !value.starts_with(['：', ':', '=', '-', ' ', '\t']) {
                continue;
            }
            return value
                .trim_start_matches(['：', ':', '=', '-', ' ', '\t'])
                .trim()
                .to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_sanitizer_preserves_numbered_typed_chapter_lines() {
        let outline = "第1章 本章目标：姜听岚确认日期倒退三个月；预期转折：主角确认发生回溯\n第2章 本章目标：姜听岚走访失踪案现场；预期转折：主角发现钟表逆转\n第3章 本章目标：姜听岚追查怀表来源；预期转折：锁定关键线索人物";

        let sanitized = sanitize_creation_outline_value(outline);

        assert_eq!(sanitized.lines().count(), 3);
        assert!(sanitized.contains("第1章 本章目标"));
        assert!(sanitized.contains("第2章 本章目标"));
        assert!(sanitized.contains("第3章 本章目标"));
        let derived = patch_normalizer::derive_plot_contract_from_outline_text(&sanitized);
        assert_eq!(
            derived
                .near_chapters
                .iter()
                .map(|chapter| chapter.number)
                .collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3)]
        );
    }

    #[test]
    fn structured_character_line_preserves_commas_inside_field_values() {
        let line = "name: 钟云安; role: 同伴; bottom_line: 无论同伴变成何种形态，必守其身后一步; name_source: generated_by_writing_tool_policy";

        let character = draft_character_line_to_contract(line);

        assert_eq!(
            character.bottom_line,
            "无论同伴变成何种形态，必守其身后一步"
        );
    }

    #[test]
    fn legacy_comma_delimited_character_line_remains_parseable() {
        let line = "name: 钟云安, role: 同伴, bottom_line: 必须守住同伴身后一步";

        let character = draft_character_line_to_contract(line);

        assert_eq!(character.canonical_name, "钟云安");
        assert_eq!(character.role, "同伴");
        assert_eq!(character.bottom_line, "必须守住同伴身后一步");
    }

    #[test]
    fn field_words_inside_earlier_values_do_not_shadow_later_character_fields() {
        let line = "name: 钟栖澜; role: 女主; desire: 通过严谨数据获得职业尊严; fear: 因妥协而失去专业底线; bottom_line: 绝不签署与实际数据不符的验收报告; arc_start: 回避冲突; arc_end: 推动变革; name_source: generated_by_writing_tool_policy";

        let character = draft_character_line_to_contract(line);

        assert_eq!(character.fear, "因妥协而失去专业底线");
        assert_eq!(character.bottom_line, "绝不签署与实际数据不符的验收报告");
        assert_eq!(character.arc_start, "回避冲突");
        assert_eq!(character.arc_end, "推动变革");
    }

    #[test]
    fn strong_contract_scale_replaces_stale_draft_projection() {
        let mut draft = build_initial_creation_draft(
            "session-contract-scale-authority",
            "fiction",
            "写现实主义悬疑小说。",
        )
        .expect("draft");
        draft.target_units = Some(2500);
        draft.chapter_unit_target = Some(2500);
        let mut contract = NovelCreationContract {
            target_units: Some(100_000),
            chapter_unit_target: Some(2500),
            ..Default::default()
        };

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut contract);

        assert_eq!(draft.target_units, Some(100_000));
        assert_eq!(draft.chapter_unit_target, Some(2500));
    }

    #[test]
    fn strong_contract_scale_preserves_explicit_user_units() {
        let mut draft = build_initial_creation_draft(
            "session-explicit-scale-authority",
            "fiction",
            "写现实主义悬疑小说，总字数10万字，每章2500字。",
        )
        .expect("draft");
        let mut contract = NovelCreationContract {
            target_units: Some(1_000_000),
            chapter_unit_target: Some(5000),
            ..Default::default()
        };

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut contract);

        assert_eq!(draft.target_units, Some(100_000));
        assert_eq!(draft.chapter_unit_target, Some(2500));
        assert_eq!(contract.target_units, Some(100_000));
        assert_eq!(contract.chapter_unit_target, Some(2500));
    }

    #[test]
    fn model_character_names_are_locally_governed_before_ready_authority() {
        let mut draft = build_initial_creation_draft(
            "session-character-anchor-rewrite",
            "fiction",
            "写太空歌剧友情冒险小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        let mut contract = NovelCreationContract {
            title: TitleContract {
                canonical_title: "量子信标：失落的航路".to_string(),
                rationale: "来自量子信标、失落航路和终局重新开辟航线。".to_string(),
                ..Default::default()
            },
            language: "zh-CN".to_string(),
            genre: "太空歌剧友情冒险".to_string(),
            brief: "两名飞行员探索失踪前哨站并建立友谊。".to_string(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2500),
            max_chapters_per_turn: Some(1),
            premise: "保守派导航员与激进派突击手被迫组队探索失踪前哨站。".to_string(),
            ending: EndingContract {
                desired_resolution: "两人解开前哨站真相并开启新航路。".to_string(),
                final_state: "旧航路恢复，新的友谊联盟成立。".to_string(),
                ..Default::default()
            },
            protagonist_arc: "从独自求稳到愿意信任同伴。".to_string(),
            world_imagery: "量子信标、环形空间站残骸、深空引擎轰鸣。".to_string(),
            main_causal_spine: "信标异常->双人组队->前哨站危机->揭开真相->开启新航路".to_string(),
            characters: vec![
                CharacterContract {
                    canonical_name: "裴照白".to_string(),
                    role: "主角".to_string(),
                    desire: "维持秩序与精准".to_string(),
                    fear: "失控与意外".to_string(),
                    bottom_line: "不牺牲无辜平民换取胜利".to_string(),
                    arc_start: "独自求稳".to_string(),
                    arc_end: "愿意信任同伴".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "梁望禾".to_string(),
                    role: "同伴".to_string(),
                    desire: "打破旧秩序".to_string(),
                    fear: "裴照白与停滞".to_string(),
                    bottom_line: "不回头逃避代价".to_string(),
                    arc_start: "只信任突击".to_string(),
                    arc_end: "学会配合导航节奏".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "温照序".to_string(),
                    role: "同伴".to_string(),
                    desire: "守住前哨站秘密".to_string(),
                    fear: "核心数据泄露".to_string(),
                    bottom_line: "不出卖沉睡守卫者".to_string(),
                    arc_start: "隐瞒真相".to_string(),
                    arc_end: "交出关键权限".to_string(),
                    ..Default::default()
                },
            ],
            themes: vec!["信任".to_string(), "冒险".to_string()],
            world_rules: vec!["量子信标连续同步会造成神经过载。".to_string()],
            style_rules: vec!["第三人称有限视角。".to_string()],
            must_avoid: vec!["不要重命名角色。".to_string()],
            outline: OutlineContract {
                raw_outline: "进入前哨站，发现能源危机，终局开辟新航路。".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut contract);

        let visible = draft.fiction_characters.join("\n");
        let canonical_names = draft
            .fiction_characters
            .iter()
            .filter_map(|line| character_name_from_contract_line(line))
            .collect::<Vec<_>>();
        assert!(!canonical_names.iter().any(|name| name == "裴照白"));
        assert!(!canonical_names.iter().any(|name| name == "梁望禾"));
        assert!(!canonical_names.iter().any(|name| name == "温照序"));
        assert!(
            visible.contains("name_source: generated_by_writing_tool_policy"),
            "ready contract should expose locally governed names as locked authority: {visible}"
        );
        let previous_names = contract
            .characters
            .iter()
            .flat_map(|character| character.previous_names.iter().map(String::as_str))
            .collect::<Vec<_>>();
        assert!(previous_names.contains(&"裴照白"), "{previous_names:?}");
        assert!(previous_names.contains(&"梁望禾"), "{previous_names:?}");
        assert!(previous_names.contains(&"温照序"), "{previous_names:?}");
    }

    #[test]
    fn pending_candidate_cannot_replace_incoming_character_authority() {
        let mut draft = build_initial_creation_draft(
            "session-pending-character-authority",
            "fiction",
            "写雪原求生悬疑小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        let pending = NovelCreationContract {
            characters: vec![
                CharacterContract {
                    canonical_name: "阮闻宁".to_string(),
                    role: "主角".to_string(),
                    desire: "找出失踪电台的真相".to_string(),
                    fear: "队伍困死在雪原".to_string(),
                    bottom_line: "不伪造求救坐标".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "季砚遥".to_string(),
                    role: "对手".to_string(),
                    desire: "独占撤离频段".to_string(),
                    fear: "旧事故记录被公开".to_string(),
                    bottom_line: "不交出主发射机".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        draft.pending_contract_candidate = Some(serde_json::json!({
            "normalized": pending,
            "issues": ["角色锚点需要修复"]
        }));
        let mut incoming = NovelCreationContract {
            characters: vec![
                CharacterContract {
                    canonical_name: "南栖舟".to_string(),
                    role: "主角".to_string(),
                    arc_end: "成为愿意承担判断责任的领队".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "钟衡遥".to_string(),
                    role: "对手".to_string(),
                    bottom_line: "绝不销毁任何尚未核验的事故记录".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut incoming);

        let visible = draft.fiction_characters.join("\n");
        assert!(!visible.contains("阮闻宁"), "{visible}");
        assert!(!visible.contains("季砚遥"), "{visible}");
        assert!(
            visible.contains("previous_names: 南栖舟")
                && visible.contains("previous_names: 钟衡遥")
                && visible.contains("name_source: generated_by_writing_tool_policy"),
            "incoming typed contract must ignore a stale pending candidate and establish local naming authority: {visible}"
        );
    }

    #[test]
    fn repeated_contract_projection_preserves_character_authority_sources() {
        let mut draft = build_initial_creation_draft(
            "session-repeated-character-projection",
            "fiction",
            "写高山救援悬疑小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        let mut contract = NovelCreationContract {
            language: "zh-CN".to_string(),
            genre: "高山救援悬疑".to_string(),
            characters: vec![
                CharacterContract {
                    canonical_name: "林远".to_string(),
                    role: "主角".to_string(),
                    desire: "查清失踪队伍真相".to_string(),
                    fear: "再次判断失误".to_string(),
                    bottom_line: "不抛弃仍有生命迹象的队员".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "苏婉".to_string(),
                    role: "关键同伴".to_string(),
                    desire: "带回完整救援记录".to_string(),
                    fear: "证据被暴风雪掩埋".to_string(),
                    bottom_line: "不篡改现场数据".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut contract);
        let first_authority = draft.fiction_characters.clone();
        assert!(first_authority
            .iter()
            .all(|line| character_line_has_locked_name_authority(&draft, line)));

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut contract);

        assert_eq!(draft.fiction_characters, first_authority);
    }

    #[test]
    fn later_full_contract_locally_governs_previously_unseen_model_characters() {
        let mut draft = build_initial_creation_draft(
            "session-later-full-contract-character",
            "fiction",
            "写现实主义职业小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 陶照声; role: 主角; desire: 抢救受损档案; fear: 档案不可逆丢失; bottom_line: 不伪造修复记录; name_source: generated_by_writing_tool_policy".to_string(),
        ];
        let mut contract = NovelCreationContract {
            language: "zh-CN".to_string(),
            premise: "陈默与林秀兰合作抢救受损档案。".to_string(),
            characters: vec![
                CharacterContract {
                    canonical_name: "陈默".to_string(),
                    role: "主角".to_string(),
                    desire: "抢救受损档案".to_string(),
                    fear: "档案不可逆丢失".to_string(),
                    bottom_line: "不伪造修复记录".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "林秀兰".to_string(),
                    role: "关键同伴".to_string(),
                    desire: "留下完整声轨".to_string(),
                    fear: "旧放映记录消失".to_string(),
                    bottom_line: "不用猜测填补缺失声轨".to_string(),
                    ..Default::default()
                },
            ],
            outline: OutlineContract {
                raw_outline: "陈默说服林秀兰参与档案抢救。".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut contract);

        let visible = draft.fiction_characters.join("\n");
        let canonical_names = draft
            .fiction_characters
            .iter()
            .filter_map(|line| character_name_from_contract_line(line))
            .collect::<BTreeSet<_>>();
        assert!(canonical_names.contains("陶照声"), "{visible}");
        assert!(!canonical_names.contains("陈默"), "{visible}");
        assert!(!canonical_names.contains("林秀兰"), "{visible}");
        assert!(
            visible.contains("previous_names: 陈默")
                && visible.contains("林秀兰")
                && visible.contains("name_source: generated_by_writing_tool_policy"),
            "{visible}"
        );
        assert!(!draft.fiction_premise.contains("陈默"));
        assert!(!draft.fiction_premise.contains("林秀兰"));
        assert!(!draft.fiction_outline.contains("陈默"));
        assert!(!draft.fiction_outline.contains("林秀兰"));
    }

    #[test]
    fn explicit_non_character_constraint_prunes_entity_from_character_authority() {
        let mut draft = build_initial_creation_draft(
            "session-non-character-entity",
            "fiction",
            "写近未来海洋悬疑小说，清洗协议代号叫K-7（它不是人物姓名），每章2500字，一共10万字。",
        )
        .expect("draft");
        let mut contract = NovelCreationContract {
            language: "zh-CN".to_string(),
            premise: "林深与苏青追查K-7清洗协议。".to_string(),
            characters: vec![
                CharacterContract {
                    canonical_name: "林深".to_string(),
                    role: "主角".to_string(),
                    desire: "找出日志缺失真相".to_string(),
                    fear: "同伴记忆被覆盖".to_string(),
                    bottom_line: "不拿站员生命做实验".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "苏青".to_string(),
                    role: "关键同伴".to_string(),
                    desire: "保存完整观测记录".to_string(),
                    fear: "历史被彻底抹除".to_string(),
                    bottom_line: "不伪造任何数据".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "K-7协议".to_string(),
                    role: "对手".to_string(),
                    desire: "维持清洗循环".to_string(),
                    fear: "协议被关闭".to_string(),
                    bottom_line: "必须执行清洗".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut contract);

        assert!(draft
            .fiction_characters
            .iter()
            .all(|line| !line.contains("K-7")));
        assert!(draft.fiction_premise.contains("K-7"));
        assert!(contract
            .characters
            .iter()
            .all(|character| character.canonical_name != "K-7协议"));
    }

    #[test]
    fn explicit_non_character_constraint_allows_natural_entity_explanation() {
        let mut draft = build_initial_creation_draft(
            "session-natural-non-character-entity",
            "fiction",
            "写近未来海洋悬疑小说，K-7是一份协议编号，明确不是人物姓名或角色，每章2500字，一共10万字。",
        )
        .expect("draft");
        let mut contract = NovelCreationContract {
            language: "zh-CN".to_string(),
            characters: vec![
                CharacterContract {
                    canonical_name: "宋知川".to_string(),
                    role: "主角".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "K-7".to_string(),
                    role: "对手".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut contract);

        assert!(draft
            .fiction_characters
            .iter()
            .all(|line| !line.contains("name: K-7")));
        assert!(contract
            .characters
            .iter()
            .all(|character| character.canonical_name != "K-7"));
    }

    #[test]
    fn model_collective_candidate_is_pruned_before_local_name_governance() {
        let mut draft = build_initial_creation_draft(
            "session-model-collective-candidate",
            "fiction",
            "从零写一本战争小说，每章5000字，一共100万字。",
        )
        .expect("draft");
        let mut contract = NovelCreationContract {
            language: "zh-CN".to_string(),
            premise: "许晏川反抗帝国议会并终结战争。".to_string(),
            characters: vec![
                CharacterContract {
                    canonical_name: "许晏川".to_string(),
                    role: "主角".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "陆栖原".to_string(),
                    role: "导师".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "帝国议会".to_string(),
                    role: "关键对手".to_string(),
                    arc_start: "掌控边境防线的庞大帝国".to_string(),
                    arc_end: "签署停战协议的妥协政府".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut contract);

        assert!(contract
            .characters
            .iter()
            .all(|character| character.canonical_name != "帝国议会"
                && !character
                    .previous_names
                    .iter()
                    .any(|name| name == "帝国议会")));
        assert!(draft
            .fiction_characters
            .iter()
            .all(|line| !line.contains("previous_names: 帝国议会")));
        assert!(contract.premise.contains("帝国议会"));
    }

    #[test]
    fn project_arc_split_prefers_growth_marker_over_nested_to_phrase() {
        let arc = "从迷信传统、逃避现实的落魄匠人，成长为敢于颠覆科技秩序、理解人性本质的守夜人，完成从“制器”到“造魂”的认知飞跃";

        assert_eq!(
            arc_start_from_project_arc(arc),
            "迷信传统、逃避现实的落魄匠人"
        );
        assert_eq!(
            arc_end_from_project_arc(arc, "终局兜底"),
            "敢于颠覆科技秩序、理解人性本质的守夜人，完成从“制器”到“造魂”的认知飞跃"
        );
    }

    #[test]
    fn character_role_normalization_preserves_explicit_gender_authority() {
        assert_eq!(draft_character_role_from_basis("女主", ""), "女主");
        assert_eq!(draft_character_role_from_basis("男主人公", ""), "男主");
        assert_eq!(draft_character_role_from_basis("主人公", ""), "主角");
        assert_eq!(
            draft_character_role_from_basis("关键关系对象", ""),
            "关键关系对象"
        );
        assert_eq!(
            draft_character_role_from_basis("关系对象", ""),
            "关键关系对象"
        );
        assert_eq!(draft_character_role_from_basis("盟友", ""), "同伴");
    }

    #[test]
    fn contract_normalization_restores_explicit_user_primary_role_authority() {
        let mut draft = build_initial_creation_draft(
            "session-explicit-gendered-primary",
            "fiction",
            "写一部都市职场言情小说。女主是一名建筑修复师，男主是档案馆员。",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            CharacterContract {
                canonical_name: "闻星衡".to_string(),
                role: "主角".to_string(),
                ..Default::default()
            }
            .to_draft_line(),
            CharacterContract {
                canonical_name: "裴望川".to_string(),
                role: "关键关系对象".to_string(),
                ..Default::default()
            }
            .to_draft_line(),
        ];

        normalize_fiction_creation_draft_after_contract_change(&mut draft);

        let primary = draft_character_line_to_contract(&draft.fiction_characters[0]);
        assert_eq!(primary.role, "女主");
    }

    #[test]
    fn contract_normalization_does_not_invent_unspecified_primary_gender() {
        let mut draft = build_initial_creation_draft(
            "session-unspecified-primary-gender",
            "fiction",
            "写一部都市职场小说。主角是一名建筑修复师。",
        )
        .expect("draft");
        draft.fiction_characters = vec![CharacterContract {
            canonical_name: "闻星衡".to_string(),
            role: "主角".to_string(),
            ..Default::default()
        }
        .to_draft_line()];

        normalize_fiction_creation_draft_after_contract_change(&mut draft);

        let primary = draft_character_line_to_contract(&draft.fiction_characters[0]);
        assert_eq!(primary.role, "主角");
    }

    #[test]
    fn minimum_character_slots_do_not_rewrite_explicit_character_roles() {
        let draft = build_initial_creation_draft(
            "session-explicit-role-slots",
            "fiction",
            "从零写一本都市小说，总字数10万字，每章2500字。",
        )
        .expect("draft");
        let mut characters = vec![
            CharacterContract {
                canonical_name: "候选甲".to_string(),
                role: "主角".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "候选乙".to_string(),
                role: "导师".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "候选丙".to_string(),
                role: "关键同伴".to_string(),
                ..Default::default()
            },
        ];

        complete_minimum_character_slots(&mut characters, &draft);

        assert_eq!(characters[0].role, "主角");
        assert_eq!(characters[1].role, "导师");
        assert_eq!(characters[2].role, "关键同伴");
    }

    #[test]
    fn contract_list_sanitizer_preserves_per_chapter_narrative_rule() {
        let rule = "场景锚定律：每章必须至少包含一个具象化的都市感官细节（如红色数字屏的闪烁、暴雨中玻璃幕墙的倒影），以强化现实都市的沉浸感";

        assert_eq!(
            sanitize_creation_contract_list(vec![rule.to_string()]),
            vec![rule.to_string()]
        );
    }
}
